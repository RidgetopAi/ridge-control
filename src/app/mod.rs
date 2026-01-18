// App module - split into submodules for maintainability
// - mod.rs: App struct, constructors, accessors
// - event_loop.rs: Main run() loop and PTY polling
// - rendering.rs: All UI drawing (draw method)
// - handlers.rs: Event handlers and action dispatch

#![allow(dead_code)]

mod agent_state;
mod event_loop;
mod handlers;
pub(crate) mod pty_state;
mod rendering;
mod ui_state;

use self::agent_state::AgentRuntimeState;
use self::pty_state::PtyState;
use self::ui_state::UiState;

use std::io::{self};
use std::path::PathBuf;
use std::time::Instant;

use arboard::Clipboard;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use ratatui::layout::Rect;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::action::{Action, ContextMenuTarget};
use crate::cli::Cli;
use crate::components::activity_stream::ActivityStream;
use crate::components::config_panel::ConfigPanel;
use crate::components::settings_editor::SettingsEditor;
use crate::components::context_menu::ContextMenuItem;
use crate::components::log_viewer::LogViewer;
use crate::components::menu::Menu;
use crate::components::process_monitor::ProcessMonitor;
use crate::components::spinner_manager::SpinnerKey;
use crate::components::stream_viewer::StreamViewer;

use crate::config::{ConfigManager, ConfigWatcherMode, KeyStore, SecretString, SessionData, SessionManager};
use crate::error::{Result, RidgeError};
use crate::input::focus::FocusArea;
use crate::input::mode::InputMode;
use crate::llm::{
    BlockType, LLMManager, LLMEvent, StreamChunk, StreamDelta, StopReason,
    ToolExecutor, ToolExecutionCheck, PendingToolUse, ToolUse,
};
use crate::streams::{StreamEvent, StreamManager, StreamsConfig, ConnectionState};
use crate::tabs::TabId;
use crate::agent::{
    AgentEngine, AgentEvent, ConfirmationRequiredExecutor, ContextManager, DiskThreadStore,
    ModelCatalog, DefaultTokenCounter, TokenCounter, SystemPromptBuilder,
    SubagentManager, AgentToolOrchestrator,
    MandrelClient,
};
use crate::lsp::LspManager;

pub(super) const TICK_INTERVAL_MS: u64 = 500;

pub struct App {
    should_quit: bool,
    // UI state extracted to UiState (Order 8.2)
    ui: UiState,
    // PTY/terminal state extracted to PtyState (Order 8.3)
    pty: PtyState,
    // Agent/LLM/Tool state extracted to AgentRuntimeState (Order 8.4)
    agent: AgentRuntimeState,
    // Process monitor
    process_monitor: ProcessMonitor,
    // Stream management
    stream_manager: StreamManager,
    stream_viewer: StreamViewer,
    show_stream_viewer: bool,
    selected_stream_index: Option<usize>,
    // Timing
    last_tick: Instant,
    // Configuration system
    config_manager: ConfigManager,
    config_watcher: Option<ConfigWatcherMode>,
    // Secure key storage (TRC-011)
    keystore: Option<KeyStore>,
    // Session persistence (TRC-012)
    session_manager: Option<SessionManager>,
    // Log viewer with auto-scroll (TRC-013)
    log_viewer: LogViewer,
    show_log_viewer: bool,
    // Config panel (TRC-014)
    config_panel: ConfigPanel,
    show_config_panel: bool,
    // Settings editor (TS-012)
    settings_editor: SettingsEditor,
    show_settings_editor: bool,
    // T2.3: MandrelClient for cross-session memory (shared service)
    mandrel_client: Arc<RwLock<MandrelClient>>,
    // P3-T3.1: LspManager for semantic code navigation (shared service)
    lsp_manager: Arc<RwLock<LspManager>>,
    // SIRK/Forge: ActivityStream for spindles visualization
    activity_stream: Option<ActivityStream>,
}

impl App {
    pub fn new() -> Result<Self> {
        // Get terminal size for PTY initialization
        let (term_width, term_height) = crossterm::terminal::size()
            .map_err(|e| RidgeError::Terminal(e.to_string()))?;
        let area = Rect::new(0, 0, term_width, term_height);
        let (term_cols, term_rows) = PtyState::calculate_terminal_size(area);

        // Initialize PtyState (handles raw mode, alternate screen, terminal, tab_manager)
        let pty = PtyState::new(term_cols, term_rows)?;

        let clipboard = Clipboard::new().ok();

        let streams_config = StreamsConfig::load();
        let mut stream_manager = StreamManager::new();
        stream_manager.load_streams(&streams_config);

        let mut menu = Menu::new();
        let stream_count = stream_manager.clients().len();
        menu.set_stream_count(stream_count);
        
        // Initialize selected_stream_index to 0 if streams exist
        let initial_stream_index = if stream_count > 0 { Some(0) } else { None };

        // Initialize secure key storage (TRC-011)
        let keystore = match KeyStore::new() {
            Ok(ks) => Some(ks),
            Err(e) => {
                tracing::warn!("Failed to initialize keystore: {}", e);
                None
            }
        };
        
        // Get working directory for tool executor
        let working_dir = std::env::current_dir().unwrap_or_else(|_| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
        });
        let mut tool_executor = ToolExecutor::new(working_dir);

        // Initialize configuration system
        let config_manager = ConfigManager::new()?;

        // T2.3: Initialize MandrelClient for cross-session memory
        let mandrel_config = config_manager.mandrel_config().clone();
        let mandrel_client = Arc::new(RwLock::new(MandrelClient::new(mandrel_config)));
        if config_manager.mandrel_config().enabled {
            tracing::info!(
                "Mandrel integration enabled: url={}, project={}",
                config_manager.mandrel_config().base_url,
                config_manager.mandrel_config().project
            );
            tool_executor.set_mandrel_client(mandrel_client.clone());
        } else {
            tracing::info!("Mandrel integration disabled");
        }

        // P3-T3.1: Initialize LspManager for semantic code navigation
        let lsp_config = config_manager.lsp_config().clone();
        let working_dir_for_lsp = std::env::current_dir().unwrap_or_else(|_| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
        });
        let lsp_manager = Arc::new(RwLock::new(LspManager::new(lsp_config, working_dir_for_lsp)));
        if config_manager.lsp_config().enabled {
            tracing::info!("LSP integration enabled");
            tool_executor.set_lsp_manager(lsp_manager.clone());
        } else {
            tracing::info!("LSP integration disabled");
        }

        // Set up config watcher if enabled
        let config_watcher = if config_manager.app_config().general.watch_config {
            let debounce_ms = config_manager.app_config().general.config_watch_debounce_ms;
            match ConfigWatcherMode::notify(config_manager.config_dir(), debounce_ms) {
                Ok(watcher) => Some(watcher),
                Err(e) => {
                    tracing::warn!("Failed to set up notify watcher, falling back to tick-based: {}", e);
                    Some(ConfigWatcherMode::tick(
                        config_manager.config_dir().to_path_buf(),
                        5000,
                    ))
                }
            }
        } else {
            None
        };
        
        // Phase 3: Initialize token counting infrastructure
        let model_catalog = std::sync::Arc::new(ModelCatalog::new());
        let token_counter: std::sync::Arc<dyn TokenCounter> = std::sync::Arc::new(DefaultTokenCounter::new(model_catalog.clone()));

        // Phase 2: Initialize AgentEngine (TP2-002-04)
        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let context_manager = std::sync::Arc::new(ContextManager::new(model_catalog.clone(), token_counter.clone()));
        let prompt_builder = SystemPromptBuilder::ridge_control();
        let agent_tool_executor: std::sync::Arc<dyn AgentToolOrchestrator> = std::sync::Arc::new(ConfirmationRequiredExecutor);
        let thread_store = match DiskThreadStore::new() {
            Ok(store) => std::sync::Arc::new(store),
            Err(e) => {
                tracing::warn!("Failed to create DiskThreadStore: {}, using default path", e);
                // Fallback: try with temp directory
                std::sync::Arc::new(DiskThreadStore::with_path(std::env::temp_dir().join("ridge-control-threads"))
                    .expect("Failed to create fallback thread store"))
            }
        };
        
        // Create separate LLMManager for AgentEngine (with same provider registrations)
        let llm_config = config_manager.llm_config();
        let mut agent_llm_manager = LLMManager::new();
        if let Some(ref ks) = keystore {
            agent_llm_manager.register_from_keystore(ks);
        }
        // Apply same provider/model settings
        agent_llm_manager.set_provider(&llm_config.defaults.provider);
        agent_llm_manager.set_model(&llm_config.defaults.model);
        tracing::info!(
            "Loaded LLM settings: provider={}, model={}",
            llm_config.defaults.provider,
            llm_config.defaults.model
        );
        
        // Configure AgentEngine with tool definitions so continuation requests include tools
        let tool_defs = tool_executor.tool_definitions_for_llm();
        tracing::info!("App: Creating AgentConfig with {} tools", tool_defs.len());
        for tool in &tool_defs {
            tracing::debug!("  Tool defined: {}", tool.name);
        }
        let agent_config = crate::agent::AgentConfig {
            tools: tool_defs,
            ..Default::default()
        };
        
        let mut agent_engine = AgentEngine::new(
            agent_llm_manager,
            context_manager,
            prompt_builder,
            agent_tool_executor,
            thread_store,
            agent_event_tx,
        ).with_config(agent_config);
        
        // TP2-002-FIX-01: Take the internal LLM event receiver for polling in run()
        let agent_llm_event_rx = agent_engine.take_llm_event_rx();

        // Initialize session manager (TRC-012)
        let session_manager = match SessionManager::new() {
            Ok(sm) => Some(sm),
            Err(e) => {
                tracing::warn!("Failed to initialize session manager: {}", e);
                None
            }
        };

        // T2.2: Initialize SubagentManager
        let subagent_manager = {
            let subagent_config = config_manager.subagent_config().clone();
            let mut manager = SubagentManager::new(subagent_config);
            // Set available tools
            manager.set_tools(tool_executor.tool_definitions_for_llm());
            Some(manager)
        };

        // Create UiState with extracted UI fields (Order 8.2)
        let ui = UiState::new(menu, clipboard);

        // Create AgentRuntimeState with extracted agent/LLM/tool fields (Order 8.4)
        let agent = AgentRuntimeState::new(
            agent_engine,
            agent_event_rx,
            agent_llm_event_rx,
            model_catalog,
            token_counter,
            tool_executor,
            subagent_manager,
        );

        Ok(Self {
            should_quit: false,
            ui,
            pty,
            agent,
            process_monitor: ProcessMonitor::new(),
            stream_manager,
            stream_viewer: StreamViewer::new(),
            show_stream_viewer: false,
            selected_stream_index: initial_stream_index,
            last_tick: Instant::now(),
            config_manager,
            config_watcher,
            keystore,
            session_manager,
            log_viewer: LogViewer::new(),
            show_log_viewer: false,
            config_panel: ConfigPanel::new(),
            show_config_panel: false,
            settings_editor: SettingsEditor::new(),
            show_settings_editor: false,
            mandrel_client,
            lsp_manager,
            activity_stream: None,
        })
    }

    /// Mark the UI as needing a redraw and record activity for adaptive polling
    #[inline]
    fn mark_dirty(&mut self) {
        self.ui.mark_dirty();
    }

    /// Create App with CLI arguments (TRC-018)
    pub fn with_cli(cli: &Cli) -> Result<Self> {
        let mut app = Self::new()?;
        
        // TRC-018: Set dangerous mode from CLI flag
        if cli.dangerously_allow_all {
            app.agent.set_dangerous_mode(true);
            tracing::warn!("DANGEROUS MODE ENABLED: All tool executions will be auto-approved");
        }
        
        // Set working directory if provided
        if let Some(ref working_dir) = cli.working_dir {
            app.agent.tool_executor = ToolExecutor::new(working_dir.clone());
            if app.agent.dangerous_mode {
                app.agent.tool_executor.set_dangerous_mode(true);
            }
            // Preserve Mandrel client when recreating tool_executor
            if app.config_manager.mandrel_config().enabled {
                app.agent.tool_executor.set_mandrel_client(app.mandrel_client.clone());
            }
            // Preserve LspManager when recreating tool_executor
            if app.config_manager.lsp_config().enabled {
                app.agent.tool_executor.set_lsp_manager(app.lsp_manager.clone());
            }
        }
        
        // Register API keys from CLI (override keystore/config)
        if let Some(ref key) = cli.anthropic_api_key {
            app.agent.agent_engine.llm_manager_mut().register_anthropic(key.clone());
        }
        if let Some(ref key) = cli.openai_api_key {
            app.agent.agent_engine.llm_manager_mut().register_openai(key.clone());
        }
        if let Some(ref key) = cli.gemini_api_key {
            app.agent.agent_engine.llm_manager_mut().register_gemini(key.clone());
        }
        if let Some(ref key) = cli.grok_api_key {
            app.agent.agent_engine.llm_manager_mut().register_grok(key.clone());
        }
        if let Some(ref key) = cli.groq_api_key {
            app.agent.agent_engine.llm_manager_mut().register_groq(key.clone());
        }
        
        Ok(app)
    }

    /// Spawn PTY for the main tab (TRC-005)
    /// This is called once at startup for backward compatibility
    pub fn spawn_pty(&mut self) -> Result<()> {
        self.pty.spawn_main_pty()
    }

    /// Restore session from disk (TRC-012)
    /// Should be called after spawn_pty() to restore additional tabs
    pub fn restore_session(&mut self) -> Result<()> {
        let Some(ref session_manager) = self.session_manager else {
            return Ok(());
        };

        let session = session_manager.load();
        
        // Skip if only main tab (default session)
        if session.tabs.len() <= 1 {
            tracing::debug!("No additional tabs to restore");
            return Ok(());
        }

        // Restore tabs from session
        let tab_iter = session.tabs.iter().map(|t| (t.name.clone(), t.is_main));
        let new_tab_ids = self.pty.tab_manager.restore_from_session(tab_iter, session.active_tab_index);

        // Spawn PTY for each restored tab
        for tab_id in new_tab_ids {
            if let Err(e) = self.spawn_pty_for_tab(tab_id) {
                tracing::error!("Failed to spawn PTY for restored tab {}: {}", tab_id, e);
            }
        }

        tracing::info!(
            "Restored {} tabs from session, active: {}",
            session.tabs.len(),
            session.active_tab_index
        );

        Ok(())
    }

    /// Save current session to disk (TRC-012)
    fn save_session(&self) {
        let Some(ref session_manager) = self.session_manager else {
            return;
        };

        let session = SessionData::from_tabs(
            self.pty.tab_manager.tabs_for_session(),
            self.pty.tab_manager.active_index(),
        );

        if let Err(e) = session_manager.save(&session) {
            tracing::error!("Failed to save session: {}", e);
        }
    }

    /// Spawn PTY for a new tab (TRC-005)
    fn spawn_pty_for_tab(&mut self, tab_id: TabId) -> Result<()> {
        self.pty.spawn_pty_for_tab(tab_id)
    }

    // NOTE: poll_pty_events() and run() moved to event_loop.rs

    fn handle_llm_event(&mut self, event: LLMEvent) {
        match event {
            LLMEvent::Chunk(chunk) => {
                match chunk {
                    StreamChunk::BlockStart { block_type, tool_id, tool_name, .. } => {
                        // TRC-017: Track what type of block we're receiving
                        self.agent.current_block_type = Some(block_type);
                        
                        // If this is a tool use block, capture the tool id and name
                        if block_type == BlockType::ToolUse {
                            if let Some(id) = tool_id {
                                self.agent.current_tool_id = Some(id);
                            }
                            self.agent.current_tool_name = tool_name;
                            self.agent.current_tool_input.clear();
                        }
                    }
                    StreamChunk::Delta(delta) => {
                        match delta {
                            StreamDelta::Text(text) => {
                                self.agent.llm_response_buffer.push_str(&text);
                            }
                            StreamDelta::Thinking(text) => {
                                // TRC-017: Route thinking to separate buffer
                                self.agent.thinking_buffer.push_str(&text);
                            }
                            StreamDelta::ToolInput { input_json, .. } => {
                                // Accumulate tool input JSON
                                self.agent.current_tool_input.push_str(&input_json);
                            }
                        }
                    }
                    StreamChunk::BlockStop { .. } => {
                        // TRC-017: When a thinking block stops, finalize the thinking content
                        if self.agent.current_block_type == Some(BlockType::Thinking) {
                            // Thinking block completed - it will be stored with the message
                            // when the full response completes
                        }
                        
                        // When a tool use block stops, we have the complete tool use
                        // AgentEngine owns tool orchestration - just clear local state
                        if let (Some(id), Some(_name)) = (self.agent.current_tool_id.take(), self.agent.current_tool_name.take()) {
                            self.agent.current_tool_input.clear();
                            tracing::debug!(
                                "BlockStop: tool {} handled by AgentEngine, cleared local state",
                                id
                            );
                        }
                        
                        // Clear current block type
                        self.agent.current_block_type = None;
                    }
                    StreamChunk::Stop { .. } => {
                        // Clear buffers on stop - AgentEngine tracks conversation via thread
                        self.agent.llm_response_buffer.clear();
                        self.agent.thinking_buffer.clear();
                        self.agent.current_block_type = None;
                    }
                    _ => {}
                }
            }
            LLMEvent::Complete => {
                // Clear buffers on complete - AgentEngine tracks conversation via thread
                self.agent.llm_response_buffer.clear();
                self.agent.thinking_buffer.clear();
                self.agent.current_block_type = None;
            }
            LLMEvent::Error(err) => {
                // TRC-023: Notify on LLM error
                self.ui.notification_manager.error_with_message("LLM Error", err.to_string());
                self.agent.llm_response_buffer.clear();
                self.agent.thinking_buffer.clear();
                self.agent.current_block_type = None;
                self.agent.current_tool_id = None;
                self.agent.current_tool_name = None;
                self.agent.current_tool_input.clear();
            }
            LLMEvent::ToolUseDetected(tool_use) => {
                // AgentEngine handles tool orchestration via AgentEvent::ToolUseRequested
                // This event is ignored - just log for debugging
                tracing::debug!(
                    "ToolUseDetected: tool {} handled by AgentEngine",
                    tool_use.id
                );
            }
        }
    }
    
    /// Handle AgentEngine events (TP2-002-05, TP2-002-06)
    /// Routes AgentEvent variants to appropriate handlers with full UI updates.
    fn handle_agent_event(&mut self, event: AgentEvent) {
        use crate::agent::AgentState;
        
        match event {
            AgentEvent::StateChanged(state) => {
                tracing::debug!("AgentEngine state changed: {:?}", state);
                
                // Manage spinner based on state transitions
                match state {
                    AgentState::PreparingRequest | AgentState::StreamingResponse => {
                        // Start LLM loading spinner
                        self.ui.spinner_manager.start(
                            SpinnerKey::LlmLoading,
                            Some("Thinking...".to_string()),
                        );
                    }
                    AgentState::ExecutingTools => {
                        // Update spinner label for tool execution
                        self.ui.spinner_manager.set_label(
                            &SpinnerKey::LlmLoading,
                            Some("Executing tools...".to_string()),
                        );
                    }
                    AgentState::FinalizingTurn => {
                        // Update spinner for finalization
                        self.ui.spinner_manager.set_label(
                            &SpinnerKey::LlmLoading,
                            Some("Finalizing...".to_string()),
                        );
                    }
                    AgentState::Idle | AgentState::AwaitingUserInput => {
                        // Stop spinner when idle or waiting for input
                        self.ui.spinner_manager.stop(&SpinnerKey::LlmLoading);
                        // Re-enable auto-scroll for next response
                        self.agent.conversation_viewer.set_auto_scroll(true);
                    }
                    AgentState::Error => {
                        // Stop spinner on error
                        self.ui.spinner_manager.stop(&SpinnerKey::LlmLoading);
                    }
                }
            }
            AgentEvent::Chunk(chunk) => {
                // Invalidate token cache - content is changing
                self.agent.cached_token_count = None;
                // Forward to existing LLM event handler for streaming display
                self.handle_llm_event(LLMEvent::Chunk(chunk));
            }
            AgentEvent::ToolUseRequested(tool_use) => {
                // Track tool in current batch with batch ID
                let batch_id = match self.agent.expected_tool_batch {
                    Some((id, count)) => {
                        // Existing batch - increment count
                        self.agent.expected_tool_batch = Some((id, count + 1));
                        id
                    }
                    None => {
                        // New batch - increment batch ID and start fresh
                        self.agent.current_batch_id += 1;
                        self.agent.expected_tool_batch = Some((self.agent.current_batch_id, 1));
                        self.agent.collected_results.clear(); // Clear any stale results
                        self.agent.current_batch_id
                    }
                };
                
                // Map this tool to its batch
                self.agent.tool_batch_map.insert(tool_use.id.clone(), batch_id);
                
                let (_, expected) = self.agent.expected_tool_batch.unwrap();
                tracing::info!(
                    "⚡ TOOL_REQUESTED: id={} name={}, batch={}, expected_count={}",
                    tool_use.id, tool_use.name, batch_id, expected
                );
                // Forward to existing tool use handler
                self.handle_tool_use_request(tool_use);
            }
            AgentEvent::ToolExecuted { tool_use_id, success } => {
                tracing::debug!("Tool {} executed: success={}", tool_use_id, success);
                
                // Update conversation viewer's tool status
                // Note: The actual result is set via complete_tool when tool result arrives
                // This event signals execution finished, so we log it but the UI update
                // comes through the tool result path
                if !success {
                    // If execution failed, the tool call widget should show error state
                    // However, the actual error details come via ToolResult
                    // Just log here - the complete_tool call handles UI state
                    tracing::warn!("Tool {} execution reported failure", tool_use_id);
                }
            }
            AgentEvent::TurnComplete { stop_reason, usage } => {
                tracing::debug!("Agent turn complete: {:?}, usage: {:?}", stop_reason, usage);
                
                // Invalidate token cache - turn complete means messages finalized
                self.agent.cached_token_count = None;
                
                // Stop any running spinners
                self.ui.spinner_manager.stop(&SpinnerKey::LlmLoading);
                
                // Re-enable auto-scroll for next response
                self.agent.conversation_viewer.set_auto_scroll(true);
                
                // Show usage info as notification if available
                if let Some(ref u) = usage {
                    tracing::info!(
                        "Turn usage: {} input, {} output tokens",
                        u.input_tokens,
                        u.output_tokens
                    );
                }
                
                // Handle stop reason
                match stop_reason {
                    StopReason::EndTurn => {
                        // Normal completion - no notification needed
                    }
                    StopReason::ToolUse => {
                        // Tool use requested - spinner already managed by StateChanged
                    }
                    StopReason::MaxTokens => {
                        self.ui.notification_manager.warning("Response truncated (max tokens reached)");
                    }
                    StopReason::StopSequence => {
                        // Normal stop sequence - no notification needed
                    }
                    StopReason::ContentFilter => {
                        self.ui.notification_manager.warning("Response filtered by content policy");
                    }
                }
            }
            AgentEvent::Error(err) => {
                // Stop spinners on error
                self.ui.spinner_manager.stop(&SpinnerKey::LlmLoading);
                self.ui.notification_manager.error_with_message("Agent Error", err);
                // Clear streaming buffers (mirrors LLMEvent::Error cleanup)
                self.agent.llm_response_buffer.clear();
                self.agent.thinking_buffer.clear();
                self.agent.current_block_type = None;
                self.agent.current_tool_id = None;
                self.agent.current_tool_name = None;
                self.agent.current_tool_input.clear();
            }
            AgentEvent::ContextTruncated { segments_dropped, tokens_used, budget } => {
                tracing::info!(
                    "Context truncated: dropped {} segments, using {}/{} tokens",
                    segments_dropped, tokens_used, budget
                );
                
                // Invalidate token cache - context has been trimmed
                self.agent.cached_token_count = None;
                
                // Notify user that context was truncated
                self.ui.notification_manager.warning(format!(
                    "Context trimmed: {} older segments removed ({}/{})",
                    segments_dropped, tokens_used, budget
                ));
            }
        }
    }

    /// Refresh subagent model commands in command palette (T2.1b)
    fn refresh_subagent_commands(&mut self) {
        // Build map of provider -> available models
        let mut available_models = std::collections::HashMap::new();
        for provider in self.agent.model_catalog.providers() {
            let models: Vec<String> = self.agent.model_catalog
                .models_for_provider(provider)
                .iter()
                .map(|s| s.to_string())
                .collect();
            available_models.insert(provider.to_string(), models);
        }

        let subagent_config = self.config_manager.subagent_config().clone();
        self.ui.command_palette.set_subagent_models(&subagent_config, &available_models);
    }

    fn handle_tool_use_request(&mut self, tool_use: ToolUse) {
        // Register tool use in conversation viewer for UI tracking (TRC-016)
        self.agent.conversation_viewer.register_tool_use(tool_use.clone());
        
        let tool_id = tool_use.id.clone();
        
        // Check if the tool can be executed
        let check = self.agent.tool_executor.can_execute(&tool_use, false);
        
        match check {
            ToolExecutionCheck::Allowed => {
                // No confirmation needed, execute directly
                let pending = PendingToolUse::new(tool_use, check);
                self.execute_tool(pending);
            }
            ToolExecutionCheck::RequiresConfirmation => {
                // Show confirmation dialog
                let pending = PendingToolUse::new(tool_use, check);
                self.agent.pending_tools.insert(tool_id.clone(), pending.clone());
                self.agent.confirming_tool_id = Some(tool_id);
                self.ui.confirm_dialog.show(pending);
                self.ui.input_mode = InputMode::Confirm {
                    title: "Tool Execution".to_string(),
                    message: "Confirm tool use?".to_string(),
                };
            }
            ToolExecutionCheck::RequiresDangerousMode
            | ToolExecutionCheck::PathNotAllowed
            | ToolExecutionCheck::UnknownTool => {
                // Show dialog explaining why it can't run
                let pending = PendingToolUse::new(tool_use, check);
                self.agent.pending_tools.insert(tool_id.clone(), pending.clone());
                self.agent.confirming_tool_id = Some(tool_id);
                self.ui.confirm_dialog.show(pending);
                self.ui.input_mode = InputMode::Confirm {
                    title: "Tool Blocked".to_string(),
                    message: "Tool cannot execute".to_string(),
                };
            }
        }
    }
    
    fn execute_tool(&mut self, pending: PendingToolUse) {
        // Clear any streaming buffer content - AgentEngine tracks conversation via thread
        self.agent.llm_response_buffer.clear();

        // Update tool state to Running in conversation viewer (TRC-016)
        self.agent.conversation_viewer.start_tool_execution(&pending.tool.id);
        
        let tool_id = pending.tool.id.clone();
        
        // Execute the tool asynchronously
        let tool = pending.tool.clone();
        let working_dir = std::env::current_dir().unwrap_or_else(|_| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
        });
        
        // We need to create a new executor for the async task
        let dangerous_mode = self.agent.tool_executor.registry().is_dangerous_mode();
        let mandrel_client = self.mandrel_client.clone();
        let mandrel_enabled = self.config_manager.mandrel_config().enabled;
        let lsp_manager = self.lsp_manager.clone();
        let lsp_enabled = self.config_manager.lsp_config().enabled;

        // Spawn the tool execution with its own result channel
        let (result_tx, result_rx) = mpsc::unbounded_channel();
        self.agent.tool_result_rxs.insert(tool_id.clone(), result_rx);

        tracing::info!("⚡ EXECUTE_TOOL: id={} name={}, active_receivers={}",
            tool_id, pending.tool.name, self.agent.tool_result_rxs.len());

        tokio::spawn(async move {
            let mut executor = ToolExecutor::new(working_dir);
            executor.set_dangerous_mode(dangerous_mode);
            // Set Mandrel client for cross-session memory tools
            if mandrel_enabled {
                executor.set_mandrel_client(mandrel_client);
            }
            // Set LspManager for semantic code navigation tools
            if lsp_enabled {
                executor.set_lsp_manager(lsp_manager);
            }

            let result = executor.execute(&tool).await;
            let _ = result_tx.send(result);
        });
        
        // Store the pending tool in the HashMap for reference
        self.agent.pending_tools.insert(tool_id, pending);
    }

    fn handle_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Connected(id) => {
                let stream_name = self.stream_manager.get_client(&id)
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| id.clone());
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(ConnectionState::Connected);
                    // TRC-025: Record successful connection in health
                    client.health_mut().record_connected();
                }
                // TRC-023: Notify on stream connect
                self.ui.notification_manager.success_with_message("Stream Connected", stream_name);
            }
            StreamEvent::Disconnected(id, reason) => {
                let stream_name = self.stream_manager.get_client(&id)
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| id.clone());
                let should_reconnect = self.stream_manager.get_client(&id)
                    .map(|c| c.reconnect_enabled() && c.health().should_reconnect())
                    .unwrap_or(false);
                
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(ConnectionState::Disconnected);
                }
                
                // TRC-025: Start auto-reconnect if enabled and was connected before
                if should_reconnect {
                    self.stream_manager.start_reconnect(&id);
                } else {
                    // TRC-023: Notify on stream disconnect
                    let msg = reason.as_deref().unwrap_or("Disconnected");
                    self.ui.notification_manager.info_with_message("Stream Disconnected", format!("{}: {}", stream_name, msg));
                }
            }
            StreamEvent::Data(id, data) => {
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.push_data(data);
                }
            }
            StreamEvent::Error(id, msg) => {
                let stream_name = self.stream_manager.get_client(&id)
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| id.clone());
                let should_reconnect = self.stream_manager.get_client(&id)
                    .map(|c| c.reconnect_enabled() && c.health().should_reconnect())
                    .unwrap_or(false);
                
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(ConnectionState::Failed);
                    // TRC-025: Record error in health
                    client.health_mut().record_error(msg.clone());
                }
                
                // TRC-025: Start auto-reconnect if enabled
                if should_reconnect {
                    self.stream_manager.start_reconnect(&id);
                } else {
                    // TRC-023: Notify on stream error (only if not reconnecting)
                    self.ui.notification_manager.error_with_message("Stream Error", format!("{}: {}", stream_name, msg));
                }
            }
            StreamEvent::StateChanged(id, state) => {
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(state);
                }
            }
            StreamEvent::ReconnectAttempt(id, attempt) => {
                // TRC-025: Notify about reconnection attempt
                let stream_name = self.stream_manager.get_client(&id)
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| id.clone());
                self.ui.notification_manager.info_with_message(
                    "Reconnecting",
                    format!("{} (attempt {})", stream_name, attempt)
                );
            }
            StreamEvent::ReconnectGaveUp(id) => {
                // TRC-025: Notify when reconnection gives up
                let stream_name = self.stream_manager.get_client(&id)
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| id.clone());
                self.ui.notification_manager.warning_with_message(
                    "Connection Failed",
                    format!("{}: Max retries reached. Use 'r' to retry manually.", stream_name)
                );
            }
        }
    }

    // NOTE: draw() method is in rendering.rs


    // NOTE: Event handlers (handle_event, handle_key, handle_mouse, handle_right_click, dispatch) are in handlers.rs

    pub fn llm_response_buffer(&self) -> &str {
        &self.agent.llm_response_buffer
    }
    
    /// Get the current streaming thinking buffer (TRC-017)
    pub fn thinking_buffer(&self) -> &str {
        &self.agent.thinking_buffer
    }
    
    /// Toggle thinking block collapse state (TRC-017)
    pub fn toggle_thinking_collapse(&mut self) {
        self.agent.collapse_thinking = !self.agent.collapse_thinking;
    }
    
    pub fn config(&self) -> &ConfigManager {
        &self.config_manager
    }
    
    /// TRC-028: Reload streams from configuration file and update menu
    /// This is called when streams.toml changes (hot-reload) or when StreamRefresh is triggered
    fn reload_streams_from_config(&mut self) {
        let config = StreamsConfig::load();
        let old_count = self.stream_manager.clients().len();
        
        self.stream_manager.load_streams(&config);
        let new_count = self.stream_manager.clients().len();
        
        // Update menu stream count
        self.ui.menu.set_stream_count(new_count);
        
        // Reset selected_stream_index if out of bounds or no streams
        if new_count == 0 {
            self.selected_stream_index = None;
        } else if self.selected_stream_index.map_or(true, |idx| idx >= new_count) {
            self.selected_stream_index = Some(0);
        }
        
        // Notify user of the reload
        if old_count != new_count {
            self.ui.notification_manager.info(format!(
                "Streams reloaded: {} → {} configured",
                old_count, new_count
            ));
        } else {
            self.ui.notification_manager.info(format!(
                "Streams reloaded: {} configured",
                new_count
            ));
        }
        
        tracing::info!("TRC-028: Reloaded {} streams from config", new_count);
    }
    
    /// TRC-020: Build context menu items based on target
    fn build_context_menu_items(&self, target: &ContextMenuTarget) -> Vec<ContextMenuItem> {
        match target {
            ContextMenuTarget::Tab(tab_index) => {
                let tab_count = self.pty.tab_manager.count();
                
                let mut items = vec![
                    ContextMenuItem::new("New Tab", Action::TabCreate)
                        .with_shortcut("Ctrl+T"),
                ];
                
                // Can only close if not the only tab
                if tab_count > 1 {
                    items.push(
                        ContextMenuItem::new("Close Tab", Action::TabCloseIndex(*tab_index))
                            .with_shortcut("Ctrl+W")
                    );
                } else {
                    items.push(
                        ContextMenuItem::new("Close Tab", Action::TabCloseIndex(*tab_index))
                            .with_shortcut("Ctrl+W")
                            .disabled()
                    );
                }
                
                items.push(ContextMenuItem::separator());
                
                // Navigation
                if *tab_index > 0 {
                    items.push(ContextMenuItem::new("Move Left", Action::TabMove { 
                        from: *tab_index, 
                        to: tab_index.saturating_sub(1) 
                    }));
                }
                if *tab_index < tab_count.saturating_sub(1) {
                    items.push(ContextMenuItem::new("Move Right", Action::TabMove { 
                        from: *tab_index, 
                        to: tab_index + 1 
                    }));
                }
                
                items.push(ContextMenuItem::separator());
                items.push(ContextMenuItem::new("Rename...", Action::TabStartRename).with_shortcut("Ctrl+R"));
                
                items
            }
            
            ContextMenuTarget::Process(pid) => {
                vec![
                    ContextMenuItem::new(format!("Kill Process ({})", pid), Action::ProcessKillRequest(*pid))
                        .with_shortcut("k"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Refresh", Action::ProcessRefresh)
                        .with_shortcut("r"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Sort by PID", Action::ProcessSetSort(crate::action::SortColumn::Pid)),
                    ContextMenuItem::new("Sort by Name", Action::ProcessSetSort(crate::action::SortColumn::Name)),
                    ContextMenuItem::new("Sort by CPU", Action::ProcessSetSort(crate::action::SortColumn::Cpu)),
                    ContextMenuItem::new("Sort by Memory", Action::ProcessSetSort(crate::action::SortColumn::Memory)),
                ]
            }
            
            ContextMenuTarget::Stream(stream_idx) => {
                let stream_count = self.stream_manager.clients().len();
                if *stream_idx >= stream_count {
                    return vec![
                        ContextMenuItem::new("No streams configured", Action::None).disabled(),
                    ];
                }
                
                let is_connected = self.stream_manager.clients()
                    .get(*stream_idx)
                    .map(|c| matches!(c.state(), ConnectionState::Connected))
                    .unwrap_or(false);
                
                let mut items = Vec::new();
                
                if is_connected {
                    items.push(ContextMenuItem::new("Disconnect", Action::StreamDisconnect(*stream_idx))
                        .with_shortcut("d"));
                    items.push(ContextMenuItem::new("View Stream", Action::StreamViewerShow(*stream_idx))
                        .with_shortcut("v"));
                } else {
                    items.push(ContextMenuItem::new("Connect", Action::StreamConnect(*stream_idx))
                        .with_shortcut("c"));
                }
                
                items.push(ContextMenuItem::separator());
                items.push(ContextMenuItem::new("Refresh All", Action::StreamRefresh)
                    .with_shortcut("r"));
                
                items
            }
            
            ContextMenuTarget::Terminal => {
                let has_selection = self.pty.tab_manager
                    .active_pty_session()
                    .map(|s| s.terminal().has_selection())
                    .unwrap_or(false);
                
                vec![
                    if has_selection {
                        ContextMenuItem::new("Copy", Action::Copy).with_shortcut("Ctrl+C")
                    } else {
                        ContextMenuItem::new("Copy", Action::Copy).with_shortcut("Ctrl+C").disabled()
                    },
                    ContextMenuItem::new("Paste", Action::Paste).with_shortcut("Ctrl+V"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Clear Scrollback", Action::ScrollToTop),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("New Tab", Action::TabCreate).with_shortcut("Ctrl+T"),
                ]
            }
            
            ContextMenuTarget::LogViewer => {
                vec![
                    ContextMenuItem::new("Clear Logs", Action::LogViewerClear)
                        .with_shortcut("c"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Toggle Auto-scroll", Action::LogViewerToggleAutoScroll)
                        .with_shortcut("a"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Scroll to Top", Action::LogViewerScrollToTop)
                        .with_shortcut("g"),
                    ContextMenuItem::new("Scroll to Bottom", Action::LogViewerScrollToBottom)
                        .with_shortcut("G"),
                ]
            }
            
            ContextMenuTarget::Conversation => {
                vec![
                    ContextMenuItem::new("Clear Conversation", Action::LlmClearConversation),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Toggle Thinking", Action::ThinkingToggleCollapse)
                        .with_shortcut("T"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Expand All Tools", Action::ToolCallExpandAll),
                    ContextMenuItem::new("Collapse All Tools", Action::ToolCallCollapseAll),
                ]
            }

            ContextMenuTarget::ChatInput => {
                let has_selection = self.agent.chat_input.has_selection();
                vec![
                    if has_selection {
                        ContextMenuItem::new("Copy", Action::ChatInputCopy).with_shortcut("Ctrl+C")
                    } else {
                        ContextMenuItem::new("Copy", Action::ChatInputCopy).with_shortcut("Ctrl+C").disabled()
                    },
                    ContextMenuItem::new("Paste", Action::Paste).with_shortcut("Ctrl+V"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Clear Input", Action::ChatInputClear),
                ]
            }

            ContextMenuTarget::Generic => {
                vec![
                    ContextMenuItem::new("Command Palette", Action::OpenCommandPalette)
                        .with_shortcut(":"),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("New Tab", Action::TabCreate)
                        .with_shortcut("Ctrl+T"),
                    ContextMenuItem::new("Settings", Action::SettingsToggle),
                    ContextMenuItem::separator(),
                    ContextMenuItem::new("Quit", Action::Quit)
                        .with_shortcut("Ctrl+C"),
                ]
            }
        }
    }
    
    // ==================== Settings Editor helpers (TS-012) ====================
    
    /// Open the settings editor overlay
    fn open_settings_editor(&mut self) {
        // Initialize settings editor with current config
        let llm_config = self.config_manager.llm_config().clone();
        self.settings_editor.set_config(llm_config);
        
        // Load key statuses from keystore
        if let Some(ref keystore) = self.keystore {
            self.settings_editor.load_key_statuses_from_keystore(keystore);
        }
        
        // Set current provider/model info
        let provider = self.agent.agent_engine.current_provider();
        let provider = if provider.is_empty() { "anthropic" } else { provider };
        let models = self.agent.model_catalog.models_for_provider(provider);
        self.settings_editor.set_available_models(models.iter().map(|m| m.to_string()).collect());
        
        self.show_settings_editor = true;
        self.ui.focus.focus(FocusArea::SettingsEditor);
    }
    
    /// Close the settings editor overlay
    fn close_settings_editor(&mut self) {
        // Auto-save settings on close
        if let Err(e) = self.config_manager.save_llm_config() {
            tracing::warn!("Failed to auto-save LLM config on close: {}", e);
        } else {
            tracing::debug!("LLM settings auto-saved on close");
        }
        
        self.show_settings_editor = false;
        self.settings_editor.clear_key_test_status();
        self.ui.focus.focus(FocusArea::Menu);
    }
    
    /// Handle storing a new API key from settings
    fn handle_settings_key_entered(&mut self, provider: String, key: String) {
        tracing::info!("Storing API key for provider: {}", provider);
        if let Some(ref mut keystore) = self.keystore {
            let key_id = crate::config::KeyId::from_provider_str(&provider);
            tracing::debug!("Key ID: {:?}", key_id);
            match keystore.store(&key_id, &SecretString::new(key.clone())) {
                Ok(()) => {
                    // Update settings editor to show key is now configured
                    self.settings_editor.mark_key_configured(&provider);

                    // Register the key with AgentEngine's LLMManager
                    tracing::info!("Registering {} provider with LLMManager", provider);
                    match provider.as_str() {
                        "anthropic" => self.agent.agent_engine.llm_manager_mut().register_anthropic(key),
                        "openai" => self.agent.agent_engine.llm_manager_mut().register_openai(key),
                        "gemini" => self.agent.agent_engine.llm_manager_mut().register_gemini(key),
                        "grok" => self.agent.agent_engine.llm_manager_mut().register_grok(key),
                        "groq" => self.agent.agent_engine.llm_manager_mut().register_groq(key),
                        _ => {}
                    }

                    self.ui.notification_manager.success(format!("{} API key saved", provider));
                }
                Err(e) => {
                    self.ui.notification_manager.error_with_message(
                        "Failed to save key",
                        e.to_string(),
                    );
                }
            }
        } else {
            self.ui.notification_manager.error("Keystore not available");
        }
    }
    
    /// Handle test key request from settings
    fn handle_settings_test_key(&mut self) {
        if let Some(provider) = self.settings_editor.selected_provider() {
            let provider = provider.to_string();
            self.settings_editor.start_key_test(&provider);
            
            // Check if provider has a key configured
            let has_key = self.keystore.as_ref()
                .map(|ks| {
                    let key_id = crate::config::KeyId::from_provider_str(&provider);
                    ks.exists(&key_id).unwrap_or(false)
                })
                .unwrap_or(false);
            
            if !has_key {
                self.settings_editor.set_key_test_result(
                    &provider,
                    false,
                    Some("No API key configured".to_string()),
                );
                return;
            }
            
            // For now, just verify the key exists - actual API test would require async
            // Future: Spawn async task to actually test the API endpoint
            self.settings_editor.set_key_test_result(&provider, true, None);
            self.ui.notification_manager.success(format!("{} key verified", provider));
        }
    }
    
    /// Handle settings save request
    fn handle_settings_save(&mut self) {
        let config = self.settings_editor.config().clone();

        // Update AgentEngine with new settings
        self.agent.agent_engine.set_provider(&config.defaults.provider);
        self.agent.agent_engine.set_model(&config.defaults.model);

        // Update config manager with new settings
        *self.config_manager.llm_config_mut() = config;
        
        // Save to config file
        if let Err(e) = self.config_manager.save_llm_config() {
            self.ui.notification_manager.error_with_message(
                "Failed to save LLM config",
                e.to_string(),
            );
        } else {
            self.ui.notification_manager.success("LLM settings saved");
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, DisableBracketedPaste);
    }
}
