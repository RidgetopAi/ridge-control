// App module - some fields for future features

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use crossterm::{
    event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::action::{Action, ContextMenuTarget, PaneBorder};
use crate::cli::Cli;
use crate::components::chat_input::ChatInput;
use crate::components::command_palette::CommandPalette;
use crate::components::config_panel::ConfigPanel;
use crate::components::settings_editor::SettingsEditor;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{ContextMenu, ContextMenuItem};
use crate::components::thread_picker::ThreadPicker;
use crate::components::conversation_viewer::ConversationViewer;
use crate::components::log_viewer::LogViewer;
use crate::components::menu::Menu;
use crate::components::notification::NotificationManager;
use crate::components::pane_layout::{PaneLayout, DragState, ResizableBorder, ResizeDirection};
use crate::components::process_monitor::ProcessMonitor;
use crate::components::spinner_manager::{SpinnerManager, SpinnerKey};
use crate::components::stream_viewer::StreamViewer;
use crate::components::Component;
use crate::config::{ConfigManager, ConfigEvent, ConfigWatcherMode, KeyId, KeyStore, SecretString, SessionData, SessionManager};
use crate::error::{Result, RidgeError};
use crate::event::PtyEvent;
use crate::input::focus::{FocusArea, FocusManager};
use crate::input::mode::InputMode;
use crate::llm::{
    BlockType, LLMManager, LLMEvent, Message, StreamChunk, StreamDelta, StopReason,
    ToolExecutor, ToolExecutionCheck, PendingToolUse, ToolUse,
};
use crate::streams::{StreamEvent, StreamManager, StreamsConfig, ConnectionState};
use crate::tabs::{TabId, TabManager, TabBar};
use crate::agent::{
    AgentEngine, AgentEvent, ConfirmationRequiredExecutor, ContextManager, DiskThreadStore,
    ModelCatalog, DefaultTokenCounter, TokenCounter, ContextStats, SystemPromptBuilder,
    SubagentManager, ToolExecutor as AgentToolExecutor, ThreadStore,
    MandrelClient,
};
use crate::lsp::LspManager;

const TICK_INTERVAL_MS: u64 = 500;

pub struct App {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    should_quit: bool,
    input_mode: InputMode,
    focus: FocusManager,
    process_monitor: ProcessMonitor,
    menu: Menu,
    stream_manager: StreamManager,
    llm_response_buffer: String,
    /// Separate buffer for streaming thinking blocks (TRC-017)
    thinking_buffer: String,
    /// Current content block type being streamed (TRC-017)
    current_block_type: Option<BlockType>,
    /// Whether to show thinking blocks collapsed by default (TRC-017)
    #[allow(dead_code)]
    collapse_thinking: bool,
    clipboard: Option<Clipboard>,
    last_tick: Instant,
    // Tool execution
    tool_executor: ToolExecutor,
    confirm_dialog: ConfirmDialog,
    /// Multi-tool tracking: maps tool_use_id -> PendingToolUse
    pending_tools: HashMap<String, PendingToolUse>,
    /// How many tools we're expecting results for (set when ToolUseRequested events arrive)
    expected_tool_count: usize,
    /// Collected results waiting to be sent to engine
    collected_results: HashMap<String, crate::llm::ToolResult>,
    /// Tool ID currently being shown in confirmation dialog (if any)
    confirming_tool_id: Option<String>,
    // Command palette
    command_palette: CommandPalette,
    // Thread picker for resuming conversations (P2-003)
    thread_picker: ThreadPicker,
    // Thread rename state
    thread_rename_buffer: Option<String>,
    // Tracking tool use during streaming
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
    current_tool_input: String,
    // Tool execution result receivers: maps tool_use_id -> receiver
    tool_result_rxs: HashMap<String, mpsc::UnboundedReceiver<std::result::Result<crate::llm::ToolResult, crate::llm::ToolError>>>,
    // Configuration system
    config_manager: ConfigManager,
    config_watcher: Option<ConfigWatcherMode>,
    // Tab system with per-tab PTY sessions (TRC-005)
    tab_manager: TabManager,
    // PTY event receivers from all tabs, keyed by TabId
    pty_receivers: Vec<mpsc::UnboundedReceiver<(TabId, PtyEvent)>>,
    // LLM conversation display
    conversation_viewer: ConversationViewer,
    chat_input: ChatInput,
    show_conversation: bool,
    // Stream viewer
    stream_viewer: StreamViewer,
    show_stream_viewer: bool,
    selected_stream_index: Option<usize>,
    // Layout areas for mouse hit-testing (TRC-010)
    tab_bar_area: Rect,
    // Conversation area for mouse hit-testing (scroll routing)
    conversation_area: Rect,
    // Chat input area for mouse hit-testing (paste routing)
    chat_input_area: Rect,
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
    // Spinner manager for animations (TRC-015)
    spinner_manager: SpinnerManager,
    // TRC-018: Dangerous mode flag (from --dangerously-allow-all CLI flag)
    dangerous_mode: bool,
    // Context menu (TRC-020)
    context_menu: ContextMenu,
    // Notification system (TRC-023)
    notification_manager: NotificationManager,
    // Pane layout for resizable splits (TRC-024)
    pane_layout: PaneLayout,
    drag_state: DragState,
    content_area: Rect,
    // Phase 3: Context indicator - token counting infrastructure
    model_catalog: std::sync::Arc<ModelCatalog>,
    token_counter: std::sync::Arc<dyn TokenCounter>,
    // Phase 2: AgentEngine integration (P2-002)
    agent_engine: AgentEngine<DiskThreadStore>,
    agent_event_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    // TP2-002-FIX-01: AgentEngine's internal LLM event receiver
    agent_llm_event_rx: Option<mpsc::UnboundedReceiver<LLMEvent>>,
    current_thread_id: Option<String>,
    // T2.2: SubagentManager for spawning sub-agents
    subagent_manager: Option<SubagentManager>,
    // T2.3: MandrelClient for cross-session memory
    mandrel_client: Arc<RwLock<MandrelClient>>,
    // P3-T3.1: LspManager for semantic code navigation
    lsp_manager: Arc<RwLock<LspManager>>,
    // T2.4: Ask user dialog for structured questions
    ask_user_dialog: crate::components::ask_user_dialog::AskUserDialog,
}

impl App {
    pub fn new() -> Result<Self> {
        enable_raw_mode().map_err(|e| RidgeError::Terminal(e.to_string()))?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)
            .map_err(|e| RidgeError::Terminal(e.to_string()))?;

        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).map_err(|e| RidgeError::Terminal(e.to_string()))?;

        let size = terminal.size().map_err(|e| RidgeError::Terminal(e.to_string()))?;
        let area = Rect::new(0, 0, size.width, size.height);
        let (term_cols, term_rows) = Self::calculate_terminal_size(area);

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
        
        // Initialize TabManager with terminal size (TRC-005)
        let mut tab_manager = TabManager::new();
        tab_manager.set_terminal_size(term_cols as u16, term_rows as u16);

        // Phase 3: Initialize token counting infrastructure
        let model_catalog = std::sync::Arc::new(ModelCatalog::new());
        let token_counter: std::sync::Arc<dyn TokenCounter> = std::sync::Arc::new(DefaultTokenCounter::new(model_catalog.clone()));

        // Phase 2: Initialize AgentEngine (TP2-002-04)
        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let context_manager = std::sync::Arc::new(ContextManager::new(model_catalog.clone(), token_counter.clone()));
        let prompt_builder = SystemPromptBuilder::ridge_control();
        let agent_tool_executor: std::sync::Arc<dyn AgentToolExecutor> = std::sync::Arc::new(ConfirmationRequiredExecutor);
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

        Ok(Self {
            terminal,
            should_quit: false,
            input_mode: InputMode::Normal,
            focus: FocusManager::new(),
            process_monitor: ProcessMonitor::new(),
            menu,
            stream_manager,
            llm_response_buffer: String::new(),
            thinking_buffer: String::new(),
            current_block_type: None,
            collapse_thinking: false,
            clipboard,
            last_tick: Instant::now(),
            tool_executor,
            confirm_dialog: ConfirmDialog::new(),
            pending_tools: HashMap::new(),
            expected_tool_count: 0,
            collected_results: HashMap::new(),
            confirming_tool_id: None,
            command_palette: CommandPalette::new(),
            thread_picker: ThreadPicker::new(),
            thread_rename_buffer: None,
            current_tool_id: None,
            current_tool_name: None,
            current_tool_input: String::new(),
            tool_result_rxs: HashMap::new(),
            config_manager,
            config_watcher,
            tab_manager,
            pty_receivers: Vec::new(),
            conversation_viewer: ConversationViewer::new(),
            chat_input: ChatInput::new(),
            show_conversation: false,
            stream_viewer: StreamViewer::new(),
            show_stream_viewer: false,
            selected_stream_index: initial_stream_index,
            tab_bar_area: Rect::default(),
            conversation_area: Rect::default(),
            chat_input_area: Rect::default(),
            keystore,
            session_manager,
            log_viewer: LogViewer::new(),
            show_log_viewer: false,
            config_panel: ConfigPanel::new(),
            show_config_panel: false,
            settings_editor: SettingsEditor::new(),
            show_settings_editor: false,
            spinner_manager: SpinnerManager::new(),
            dangerous_mode: false,
            context_menu: ContextMenu::new(),
            notification_manager: NotificationManager::new(),
            pane_layout: PaneLayout::new(),
            drag_state: DragState::default(),
            content_area: Rect::default(),
            model_catalog,
            token_counter,
            // Phase 2: AgentEngine initialized (TP2-002-04)
            agent_engine,
            agent_event_rx: Some(agent_event_rx),
            // TP2-002-FIX-01: Wire AgentEngine's internal LLM event receiver
            agent_llm_event_rx,
            current_thread_id: None,
            // T2.2: SubagentManager for spawning sub-agents
            subagent_manager,
            // T2.3: MandrelClient for cross-session memory
            mandrel_client,
            // P3-T3.1: LspManager for semantic code navigation
            lsp_manager,
            // T2.4: Ask user dialog for structured questions
            ask_user_dialog: crate::components::ask_user_dialog::AskUserDialog::new(),
        })
    }

    /// Create App with CLI arguments (TRC-018)
    pub fn with_cli(cli: &Cli) -> Result<Self> {
        let mut app = Self::new()?;
        
        // TRC-018: Set dangerous mode from CLI flag
        if cli.dangerously_allow_all {
            app.set_dangerous_mode(true);
            tracing::warn!("DANGEROUS MODE ENABLED: All tool executions will be auto-approved");
        }
        
        // Set working directory if provided
        if let Some(ref working_dir) = cli.working_dir {
            app.tool_executor = ToolExecutor::new(working_dir.clone());
            if app.dangerous_mode {
                app.tool_executor.set_dangerous_mode(true);
            }
            // Preserve Mandrel client when recreating tool_executor
            if app.config_manager.mandrel_config().enabled {
                app.tool_executor.set_mandrel_client(app.mandrel_client.clone());
            }
            // Preserve LspManager when recreating tool_executor
            if app.config_manager.lsp_config().enabled {
                app.tool_executor.set_lsp_manager(app.lsp_manager.clone());
            }
        }
        
        // Register API keys from CLI (override keystore/config)
        if let Some(ref key) = cli.anthropic_api_key {
            app.agent_engine.llm_manager_mut().register_anthropic(key.clone());
        }
        if let Some(ref key) = cli.openai_api_key {
            app.agent_engine.llm_manager_mut().register_openai(key.clone());
        }
        if let Some(ref key) = cli.gemini_api_key {
            app.agent_engine.llm_manager_mut().register_gemini(key.clone());
        }
        if let Some(ref key) = cli.grok_api_key {
            app.agent_engine.llm_manager_mut().register_grok(key.clone());
        }
        if let Some(ref key) = cli.groq_api_key {
            app.agent_engine.llm_manager_mut().register_groq(key.clone());
        }
        
        Ok(app)
    }
    
    /// Set dangerous mode for tool execution (TRC-018)
    pub fn set_dangerous_mode(&mut self, enabled: bool) {
        self.dangerous_mode = enabled;
        self.tool_executor.set_dangerous_mode(enabled);
    }
    
    fn calculate_terminal_size(area: Rect) -> (usize, usize) {
        let terminal_width = (area.width * 2 / 3).saturating_sub(2);
        let terminal_height = area.height.saturating_sub(2);
        (terminal_width as usize, terminal_height as usize)
    }

    /// Spawn PTY for the main tab (TRC-005)
    /// This is called once at startup for backward compatibility
    pub fn spawn_pty(&mut self) -> Result<()> {
        // Spawn PTY for the main tab (tab 0)
        let main_tab_id = self.tab_manager.active_tab().id();
        if let Some(rx) = self.tab_manager.spawn_pty_for_tab(main_tab_id)? {
            self.pty_receivers.push(rx);
        }
        Ok(())
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
        let new_tab_ids = self.tab_manager.restore_from_session(tab_iter, session.active_tab_index);

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
            self.tab_manager.tabs_for_session(),
            self.tab_manager.active_index(),
        );

        if let Err(e) = session_manager.save(&session) {
            tracing::error!("Failed to save session: {}", e);
        }
    }

    /// Spawn PTY for a new tab (TRC-005)
    fn spawn_pty_for_tab(&mut self, tab_id: TabId) -> Result<()> {
        if let Some(rx) = self.tab_manager.spawn_pty_for_tab(tab_id)? {
            self.pty_receivers.push(rx);
        }
        Ok(())
    }

    /// Poll all PTY event receivers (TRC-005)
    fn poll_pty_events(&mut self) {
        // Collect events first to avoid borrow issues
        let mut events: Vec<(TabId, PtyEvent)> = Vec::new();
        
        for rx in &mut self.pty_receivers {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        
        // Process collected events
        for (tab_id, event) in events {
            match event {
                PtyEvent::Output(data) => {
                    self.tab_manager.process_pty_output(tab_id, &data);
                    // Mark tab as having activity if not active
                    if tab_id != self.tab_manager.active_tab().id() {
                        self.tab_manager.set_tab_activity(tab_id, true);
                    }
                }
                PtyEvent::Exited(code) => {
                    self.tab_manager.mark_pty_dead(tab_id);
                    // If main tab (id 0) dies, quit the app
                    if tab_id == 0 {
                        self.should_quit = true;
                    } else {
                        // TRC-023: Notify on background tab shell exit
                        self.notification_manager.info_with_message(
                            "Shell Exited",
                            format!("Tab {} exited with code {}", tab_id, code)
                        );
                    }
                }
                PtyEvent::Error(err) => {
                    self.tab_manager.mark_pty_dead(tab_id);
                    if tab_id == 0 {
                        self.should_quit = true;
                    } else {
                        // TRC-023: Notify on background tab shell error
                        self.notification_manager.error_with_message(
                            "Shell Error",
                            format!("Tab {}: {}", tab_id, err)
                        );
                    }
                }
            }
        }
    }

    pub fn run(&mut self) -> Result<()> {
        let mut stream_rx = self.stream_manager.take_event_rx();

        loop {
            self.draw()?;

            // Poll PTY events from all tabs (TRC-005)
            self.poll_pty_events();

            if let Some(ref mut rx) = stream_rx {
                while let Ok(stream_event) = rx.try_recv() {
                    self.handle_stream_event(stream_event);
                }
            }
            
            // TP2-002-FIX-01: Poll AgentEngine's internal LLM events and forward to engine
            // This is the critical wiring that was missing - LLMManager inside AgentEngine
            // sends events to its internal channel, which we now poll and forward
            let agent_llm_events: Vec<_> = if let Some(ref mut rx) = self.agent_llm_event_rx {
                let mut events = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                events
            } else {
                Vec::new()
            };
            
            for ev in agent_llm_events {
                self.agent_engine.handle_llm_event(ev);
            }
            
            // Poll AgentEngine events (TP2-002-05)
            // Collect first, then dispatch to avoid borrow issues
            let agent_events: Vec<_> = if let Some(ref mut rx) = self.agent_event_rx {
                let mut events = Vec::new();
                while let Ok(event) = rx.try_recv() {
                    events.push(event);
                }
                events
            } else {
                Vec::new()
            };
            
            for agent_event in agent_events {
                self.handle_agent_event(agent_event);
            }
            
            // Poll tool execution results from all receivers
            // Collect (tool_id, result) pairs first, then dispatch to avoid borrow issues
            let tool_results: Vec<(String, std::result::Result<crate::llm::ToolResult, crate::llm::ToolError>)> = {
                let mut results = Vec::new();
                for (tool_id, rx) in self.tool_result_rxs.iter_mut() {
                    while let Ok(result) = rx.try_recv() {
                        results.push((tool_id.clone(), result));
                    }
                }
                results
            };
            
            for (tool_id, result) in tool_results {
                match result {
                    Ok(tool_result) => {
                        self.dispatch(Action::ToolResult(tool_result))?;
                    }
                    Err(crate::llm::ToolError::WaitingForUserInput { tool_use_id, questions }) => {
                        // T2.4: Show ask_user dialog instead of sending error
                        self.ask_user_dialog.show(tool_use_id, questions);
                    }
                    Err(e) => {
                        // Create an error result using the tool_id we tracked
                        let error_result = crate::llm::ToolResult {
                            tool_use_id: tool_id,
                            content: crate::llm::ToolResultContent::Text(e.to_string()),
                            is_error: true,
                        };
                        self.dispatch(Action::ToolResult(error_result))?;
                    }
                }
            }
            
            // Clean up completed receivers (those with no pending tools)
            self.tool_result_rxs.retain(|id, _| self.pending_tools.contains_key(id));

            if self.last_tick.elapsed() >= Duration::from_millis(TICK_INTERVAL_MS) {
                self.dispatch(Action::Tick)?;
                self.last_tick = Instant::now();
            }
            
            // Poll config watcher for file changes
            let config_events: Vec<_> = if let Some(ref mut watcher) = self.config_watcher {
                watcher.poll_events()
            } else {
                Vec::new()
            };
            
            for event in config_events {
                match event {
                    ConfigEvent::Changed(path) => {
                        self.dispatch(Action::ConfigChanged(path))?;
                    }
                    ConfigEvent::Error(msg) => {
                        tracing::warn!("Config watcher error: {}", msg);
                    }
                }
            }

            if self.should_quit {
                break;
            }

            if event::poll(Duration::from_millis(16)).map_err(|e| RidgeError::Terminal(e.to_string()))? {
                let event = event::read().map_err(|e| RidgeError::Terminal(e.to_string()))?;

                if let Some(action) = self.handle_event(event) {
                    // Track if this action came from command palette
                    let was_in_palette = matches!(self.input_mode, InputMode::CommandPalette);

                    // Dispatch the action
                    self.dispatch(action)?;

                    // POST-DISPATCH HOOK: Reset mode after palette actions complete
                    // If we started in CommandPalette and are still there (action didn't
                    // explicitly set a different mode), reset to Normal
                    if was_in_palette && matches!(self.input_mode, InputMode::CommandPalette) {
                        self.input_mode = InputMode::Normal;
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_llm_event(&mut self, event: LLMEvent) {
        match event {
            LLMEvent::Chunk(chunk) => {
                match chunk {
                    StreamChunk::BlockStart { block_type, tool_id, tool_name, .. } => {
                        // TRC-017: Track what type of block we're receiving
                        self.current_block_type = Some(block_type);
                        
                        // If this is a tool use block, capture the tool id and name
                        if block_type == BlockType::ToolUse {
                            if let Some(id) = tool_id {
                                self.current_tool_id = Some(id);
                            }
                            self.current_tool_name = tool_name;
                            self.current_tool_input.clear();
                        }
                    }
                    StreamChunk::Delta(delta) => {
                        match delta {
                            StreamDelta::Text(text) => {
                                self.llm_response_buffer.push_str(&text);
                            }
                            StreamDelta::Thinking(text) => {
                                // TRC-017: Route thinking to separate buffer
                                self.thinking_buffer.push_str(&text);
                            }
                            StreamDelta::ToolInput { input_json, .. } => {
                                // Accumulate tool input JSON
                                self.current_tool_input.push_str(&input_json);
                            }
                        }
                    }
                    StreamChunk::BlockStop { .. } => {
                        // TRC-017: When a thinking block stops, finalize the thinking content
                        if self.current_block_type == Some(BlockType::Thinking) {
                            // Thinking block completed - it will be stored with the message
                            // when the full response completes
                        }
                        
                        // When a tool use block stops, we have the complete tool use
                        // AgentEngine owns tool orchestration - just clear local state
                        if let (Some(id), Some(_name)) = (self.current_tool_id.take(), self.current_tool_name.take()) {
                            self.current_tool_input.clear();
                            tracing::debug!(
                                "BlockStop: tool {} handled by AgentEngine, cleared local state",
                                id
                            );
                        }
                        
                        // Clear current block type
                        self.current_block_type = None;
                    }
                    StreamChunk::Stop { .. } => {
                        // Clear buffers on stop - AgentEngine tracks conversation via thread
                        self.llm_response_buffer.clear();
                        self.thinking_buffer.clear();
                        self.current_block_type = None;
                    }
                    _ => {}
                }
            }
            LLMEvent::Complete => {
                // Clear buffers on complete - AgentEngine tracks conversation via thread
                self.llm_response_buffer.clear();
                self.thinking_buffer.clear();
                self.current_block_type = None;
            }
            LLMEvent::Error(err) => {
                // TRC-023: Notify on LLM error
                self.notification_manager.error_with_message("LLM Error", err.to_string());
                self.llm_response_buffer.clear();
                self.thinking_buffer.clear();
                self.current_block_type = None;
                self.current_tool_id = None;
                self.current_tool_name = None;
                self.current_tool_input.clear();
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
                        self.spinner_manager.start(
                            SpinnerKey::LlmLoading,
                            Some("Thinking...".to_string()),
                        );
                    }
                    AgentState::ExecutingTools => {
                        // Update spinner label for tool execution
                        self.spinner_manager.set_label(
                            &SpinnerKey::LlmLoading,
                            Some("Executing tools...".to_string()),
                        );
                    }
                    AgentState::FinalizingTurn => {
                        // Update spinner for finalization
                        self.spinner_manager.set_label(
                            &SpinnerKey::LlmLoading,
                            Some("Finalizing...".to_string()),
                        );
                    }
                    AgentState::Idle | AgentState::AwaitingUserInput => {
                        // Stop spinner when idle or waiting for input
                        self.spinner_manager.stop(&SpinnerKey::LlmLoading);
                        // Re-enable auto-scroll for next response
                        self.conversation_viewer.set_auto_scroll(true);
                    }
                    AgentState::Error => {
                        // Stop spinner on error
                        self.spinner_manager.stop(&SpinnerKey::LlmLoading);
                    }
                }
            }
            AgentEvent::Chunk(chunk) => {
                // Forward to existing LLM event handler for streaming display
                self.handle_llm_event(LLMEvent::Chunk(chunk));
            }
            AgentEvent::ToolUseRequested(tool_use) => {
                // Track expected tool count - increments for each tool requested
                self.expected_tool_count += 1;
                tracing::info!(
                    "⚡ TOOL_REQUESTED: id={} name={}, expected_count now={}",
                    tool_use.id, tool_use.name, self.expected_tool_count
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
                
                // Stop any running spinners
                self.spinner_manager.stop(&SpinnerKey::LlmLoading);
                
                // Re-enable auto-scroll for next response
                self.conversation_viewer.set_auto_scroll(true);
                
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
                        self.notification_manager.warning("Response truncated (max tokens reached)");
                    }
                    StopReason::StopSequence => {
                        // Normal stop sequence - no notification needed
                    }
                    StopReason::ContentFilter => {
                        self.notification_manager.warning("Response filtered by content policy");
                    }
                }
            }
            AgentEvent::Error(err) => {
                // Stop spinners on error
                self.spinner_manager.stop(&SpinnerKey::LlmLoading);
                self.notification_manager.error_with_message("Agent Error", err);
                // Clear streaming buffers (mirrors LLMEvent::Error cleanup)
                self.llm_response_buffer.clear();
                self.thinking_buffer.clear();
                self.current_block_type = None;
                self.current_tool_id = None;
                self.current_tool_name = None;
                self.current_tool_input.clear();
            }
            AgentEvent::ContextTruncated { segments_dropped, tokens_used, budget } => {
                tracing::info!(
                    "Context truncated: dropped {} segments, using {}/{} tokens",
                    segments_dropped, tokens_used, budget
                );
                
                // Notify user that context was truncated
                self.notification_manager.warning(format!(
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
        for provider in self.model_catalog.providers() {
            let models: Vec<String> = self.model_catalog
                .models_for_provider(provider)
                .iter()
                .map(|s| s.to_string())
                .collect();
            available_models.insert(provider.to_string(), models);
        }

        let subagent_config = self.config_manager.subagent_config().clone();
        self.command_palette.set_subagent_models(&subagent_config, &available_models);
    }

    fn handle_tool_use_request(&mut self, tool_use: ToolUse) {
        // Register tool use in conversation viewer for UI tracking (TRC-016)
        self.conversation_viewer.register_tool_use(tool_use.clone());
        
        let tool_id = tool_use.id.clone();
        
        // Check if the tool can be executed
        let check = self.tool_executor.can_execute(&tool_use, false);
        
        match check {
            ToolExecutionCheck::Allowed => {
                // No confirmation needed, execute directly
                let pending = PendingToolUse::new(tool_use, check);
                self.execute_tool(pending);
            }
            ToolExecutionCheck::RequiresConfirmation => {
                // Show confirmation dialog
                let pending = PendingToolUse::new(tool_use, check);
                self.pending_tools.insert(tool_id.clone(), pending.clone());
                self.confirming_tool_id = Some(tool_id);
                self.confirm_dialog.show(pending);
                self.input_mode = InputMode::Confirm {
                    title: "Tool Execution".to_string(),
                    message: "Confirm tool use?".to_string(),
                };
            }
            ToolExecutionCheck::RequiresDangerousMode
            | ToolExecutionCheck::PathNotAllowed
            | ToolExecutionCheck::UnknownTool => {
                // Show dialog explaining why it can't run
                let pending = PendingToolUse::new(tool_use, check);
                self.pending_tools.insert(tool_id.clone(), pending.clone());
                self.confirming_tool_id = Some(tool_id);
                self.confirm_dialog.show(pending);
                self.input_mode = InputMode::Confirm {
                    title: "Tool Blocked".to_string(),
                    message: "Tool cannot execute".to_string(),
                };
            }
        }
    }
    
    fn execute_tool(&mut self, pending: PendingToolUse) {
        // Clear any streaming buffer content - AgentEngine tracks conversation via thread
        self.llm_response_buffer.clear();

        // Update tool state to Running in conversation viewer (TRC-016)
        self.conversation_viewer.start_tool_execution(&pending.tool.id);
        
        let tool_id = pending.tool.id.clone();
        
        // Execute the tool asynchronously
        let tool = pending.tool.clone();
        let working_dir = std::env::current_dir().unwrap_or_else(|_| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
        });
        
        // We need to create a new executor for the async task
        let dangerous_mode = self.tool_executor.registry().is_dangerous_mode();
        let mandrel_client = self.mandrel_client.clone();
        let mandrel_enabled = self.config_manager.mandrel_config().enabled;
        let lsp_manager = self.lsp_manager.clone();
        let lsp_enabled = self.config_manager.lsp_config().enabled;

        // Spawn the tool execution with its own result channel
        let (result_tx, result_rx) = mpsc::unbounded_channel();
        self.tool_result_rxs.insert(tool_id.clone(), result_rx);

        tracing::info!("⚡ EXECUTE_TOOL: id={} name={}, active_receivers={}",
            tool_id, pending.tool.name, self.tool_result_rxs.len());

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
        self.pending_tools.insert(tool_id, pending);
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
                self.notification_manager.success_with_message("Stream Connected", stream_name);
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
                    self.notification_manager.info_with_message("Stream Disconnected", format!("{}: {}", stream_name, msg));
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
                    self.notification_manager.error_with_message("Stream Error", format!("{}: {}", stream_name, msg));
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
                self.notification_manager.info_with_message(
                    "Reconnecting",
                    format!("{} (attempt {})", stream_name, attempt)
                );
            }
            StreamEvent::ReconnectGaveUp(id) => {
                // TRC-025: Notify when reconnection gives up
                let stream_name = self.stream_manager.get_client(&id)
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| id.clone());
                self.notification_manager.warning_with_message(
                    "Connection Failed",
                    format!("{}: Max retries reached. Use 'r' to retry manually.", stream_name)
                );
            }
        }
    }

    fn draw(&mut self) -> Result<()> {
        let focus = self.focus.clone();
        let streams: Vec<_> = self.stream_manager.clients().to_vec();
        let show_confirm = self.confirm_dialog.is_visible();
        let show_palette = self.command_palette.is_visible();
        let show_thread_picker = self.thread_picker.is_visible();
        let show_thread_rename = self.thread_rename_buffer.is_some();
        let thread_rename_text = self.thread_rename_buffer.clone().unwrap_or_default();
        let show_ask_user = self.ask_user_dialog.is_visible();
        let show_context_menu = self.context_menu.is_visible();
        let has_notifications = self.notification_manager.has_notifications();
        let _show_tabs = self.tab_manager.count() > 1; // Kept for potential future use
        let show_conversation = self.show_conversation || !self.llm_response_buffer.is_empty() || !self.thinking_buffer.is_empty();
        let show_stream_viewer = self.show_stream_viewer;
        let show_log_viewer = self.show_log_viewer;
        let show_config_panel = self.show_config_panel;
        let show_settings_editor = self.show_settings_editor;
        let selected_stream_idx = self.selected_stream_index;
        let theme = self.config_manager.theme().clone();
        // TP2-002-14: Get messages from AgentThread segments if available
        let messages: Vec<Message> = if let Some(thread) = self.agent_engine.current_thread() {
            // Extract all messages from thread segments
            thread.segments().iter()
                .flat_map(|segment| segment.messages.clone())
                .collect()
        } else {
            Vec::new()
        };
        let streaming_buffer = self.llm_response_buffer.clone();
        // TRC-017: Clone thinking buffer for rendering
        let thinking_buffer = self.thinking_buffer.clone();
        
        // Get active tab's PTY session for rendering (TRC-005)
        let active_tab_id = self.tab_manager.active_tab().id();
        
        // Pre-calculate tab bar area for mouse hit-testing (TRC-010)
        let term_size = self.terminal.size().unwrap_or_default();
        let term_rect = Rect::new(0, 0, term_size.width, term_size.height);
        // Always show status bar for mode indicator
        let show_status_bar_pre = true;
        let (computed_tab_bar_area, computed_content_area) = if show_status_bar_pre {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(term_rect);
            (chunks[0], chunks[1])
        } else {
            (Rect::default(), term_rect)
        };
        self.tab_bar_area = computed_tab_bar_area;
        // TRC-024: Store content area for pane resize mouse hit-testing
        self.content_area = computed_content_area;

        self.terminal
            .draw(|frame| {
                let size = frame.area();

                // Always show status bar for mode indicator
                let show_status_bar = true;
                
                // Split: optional tab/status bar at top, then main content
                let (tab_bar_area, content_area) = if show_status_bar {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(0)])
                        .split(size);
                    (chunks[0], chunks[1])
                } else {
                    // No tab bar - use full area
                    (Rect::default(), size)
                };

                // Render tab bar if multiple tabs OR dangerous mode warning bar
                // TRC-018: Pass dangerous_mode to show warning indicator
                if show_status_bar {
                    let tab_bar = TabBar::from_manager_themed(&self.tab_manager, &theme)
                        .dangerous_mode(self.dangerous_mode)
                        .input_mode(self.input_mode.clone());
                    frame.render_widget(tab_bar, tab_bar_area);
                }

                // TRC-024: Store content area for mouse hit-testing
                // Main layout: left (terminal or terminal+conversation) and right (process monitor + menu)
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.pane_layout.main_constraints())
                    .split(content_area);

                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(self.pane_layout.right_constraints())
                    .split(main_chunks[1]);

                // Left area: split between terminal and conversation if conversation is visible
                // Use active tab's terminal widget (TRC-005)
                if show_conversation {
                    let left_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(self.pane_layout.left_constraints())
                        .split(main_chunks[0]);

                    if let Some(session) = self.tab_manager.get_pty_session(active_tab_id) {
                        session.terminal().render(
                            frame,
                            left_chunks[0],
                            focus.is_focused(FocusArea::Terminal),
                            &theme,
                        );
                    }

                    // Split conversation area: messages on top, chat input at bottom
                    let conv_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(5), Constraint::Length(6)])
                        .split(left_chunks[1]);

                    // TRC-017: Pass thinking_buffer for extended thinking display
                    // Pass model info for header display
                    let model_info = {
                        let provider = self.agent_engine.current_provider();
                        let model = self.agent_engine.current_model();
                        if provider.is_empty() || model.is_empty() {
                            None
                        } else {
                            Some((provider, model))
                        }
                    };

                    // Phase 3: Compute context stats for header display
                    let context_stats = {
                        let model = self.agent_engine.current_model();
                        if model.is_empty() || messages.is_empty() {
                            None
                        } else {
                            let model_info = self.model_catalog.info_for(model);
                            let tokens_used = self.token_counter.count_messages(model, &messages);
                            // Budget = context window - default output tokens - 2% safety
                            let safety = model_info.max_context_tokens / 50; // 2%
                            let budget = model_info.max_context_tokens
                                .saturating_sub(model_info.default_max_output_tokens)
                                .saturating_sub(safety);
                            Some(ContextStats::new(tokens_used, budget, false, messages.len()))
                        }
                    };

                    self.conversation_viewer.render_conversation(
                        frame,
                        conv_chunks[0],
                        focus.is_focused(FocusArea::StreamViewer), // Conversation history focus
                        &messages,
                        &streaming_buffer,
                        &thinking_buffer,
                        &theme,
                        model_info,
                        context_stats.as_ref(),
                    );

                    // Render chat input at bottom of conversation area
                    self.chat_input.render(
                        frame,
                        conv_chunks[1],
                        focus.is_focused(FocusArea::ChatInput),
                        &theme,
                    );

                    let term_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(left_chunks[0])
                    };
                    if let Some(session) = self.tab_manager.get_pty_session_mut(active_tab_id) {
                        session.terminal_mut().set_inner_area(term_inner);
                    }

                    let conv_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(conv_chunks[0])
                    };
                    self.conversation_viewer.set_inner_area(conv_inner);

                    // Set inner area for chat input mouse coordinate conversion
                    let chat_input_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(conv_chunks[1])
                    };
                    self.chat_input.set_inner_area(chat_input_inner);

                    // Save conversation area for mouse hit-testing
                    self.conversation_area = conv_chunks[0];
                    // Save chat input area for mouse hit-testing (paste routing and selection)
                    self.chat_input_area = conv_chunks[1];
                } else {
                    // Clear conversation and chat input areas when not visible
                    self.conversation_area = Rect::default();
                    self.chat_input_area = Rect::default();
                    if let Some(session) = self.tab_manager.get_pty_session(active_tab_id) {
                        session.terminal().render(
                            frame,
                            main_chunks[0],
                            focus.is_focused(FocusArea::Terminal),
                            &theme,
                        );
                    }

                    let term_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(main_chunks[0])
                    };
                    if let Some(session) = self.tab_manager.get_pty_session_mut(active_tab_id) {
                        session.terminal_mut().set_inner_area(term_inner);
                    }
                }

                self.process_monitor.render(
                    frame,
                    right_chunks[0],
                    focus.is_focused(FocusArea::ProcessMonitor),
                    &theme,
                );
                self.menu.render_with_streams(
                    frame,
                    right_chunks[1],
                    focus.is_focused(FocusArea::Menu),
                    &streams,
                    &theme,
                );

                let proc_inner = {
                    let block = ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL);
                    block.inner(right_chunks[0])
                };
                self.process_monitor.set_inner_area(proc_inner);

                let menu_inner = {
                    let block = ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL);
                    block.inner(right_chunks[1])
                };
                self.menu.set_inner_area(menu_inner);
                
                // Render overlays (in order of z-index)
                
                // Stream viewer overlay - centered modal dialog
                if show_stream_viewer {
                    // Calculate centered dialog size (70% width, 70% height for stream content)
                    let dialog_width = (size.width * 70 / 100).clamp(60, 120);
                    let dialog_height = (size.height * 70 / 100).clamp(20, 40);
                    let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
                    let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;
                    let stream_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

                    // Clear the area behind for readability
                    frame.render_widget(ratatui::widgets::Clear, stream_area);

                    let selected_stream = selected_stream_idx
                        .and_then(|idx| streams.get(idx));

                    self.stream_viewer.render_stream_themed(
                        frame,
                        stream_area,
                        focus.is_focused(FocusArea::StreamViewer),
                        selected_stream,
                        &theme,
                    );
                }
                
                // Log viewer overlay (TRC-013) - centered modal dialog
                if show_log_viewer {
                    // Calculate centered dialog size (70% width, 70% height for log content)
                    let dialog_width = (size.width * 70 / 100).clamp(60, 120);
                    let dialog_height = (size.height * 70 / 100).clamp(20, 40);
                    let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
                    let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;
                    let log_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

                    // Clear the area behind for readability
                    frame.render_widget(ratatui::widgets::Clear, log_area);

                    self.log_viewer.render(
                        frame,
                        log_area,
                        focus.is_focused(FocusArea::LogViewer),
                        &theme,
                    );

                    let log_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(log_area)
                    };
                    self.log_viewer.set_inner_area(log_inner);
                }
                
                // Config panel overlay (TRC-014) - centered modal dialog
                if show_config_panel {
                    // Calculate centered dialog size (60% width, 60% height, clamped)
                    let dialog_width = (size.width * 60 / 100).clamp(50, 100);
                    let dialog_height = (size.height * 60 / 100).clamp(15, 35);
                    let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
                    let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;
                    let config_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

                    // Clear the area behind for readability
                    frame.render_widget(ratatui::widgets::Clear, config_area);

                    self.config_panel.render(
                        frame,
                        config_area,
                        focus.is_focused(FocusArea::ConfigPanel),
                        &theme,
                    );

                    let config_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(config_area)
                    };
                    self.config_panel.set_inner_area(config_inner);
                }
                
                // Settings Editor overlay (TS-012) - centered modal dialog
                if show_settings_editor {
                    // Calculate centered dialog size (70% width, 70% height, clamped)
                    let dialog_width = (size.width * 70 / 100).clamp(60, 120);
                    let dialog_height = (size.height * 70 / 100).clamp(20, 40);
                    let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
                    let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;
                    let settings_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

                    // Clear the area behind for readability
                    frame.render_widget(ratatui::widgets::Clear, settings_area);

                    self.settings_editor.render(
                        frame,
                        settings_area,
                        focus.is_focused(FocusArea::SettingsEditor),
                        &theme,
                    );
                }
                
                if show_confirm {
                    self.confirm_dialog.render(frame, size, &theme);
                }
                
                if show_palette {
                    self.command_palette.render(frame, size, &theme);
                }

                // P2-003: Thread picker overlay
                if show_thread_picker {
                    self.thread_picker.render(frame, size, &theme);
                }

                // P2-003: Thread rename dialog overlay
                if show_thread_rename {
                    use ratatui::widgets::{Block, Borders, Clear, Paragraph};
                    use ratatui::text::{Line, Span};
                    use ratatui::style::{Modifier, Style};
                    use ratatui::layout::Alignment;

                    // Calculate dialog size (centered, fixed width)
                    let dialog_width = 50u16.min(size.width.saturating_sub(4));
                    let dialog_height = 5u16;
                    let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
                    let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;
                    let dialog_area = ratatui::layout::Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

                    // Clear background
                    frame.render_widget(Clear, dialog_area);

                    // Dialog block
                    let block = Block::default()
                        .title(" Rename Thread ")
                        .title_style(Style::default().fg(theme.command_palette.border.to_color()).add_modifier(Modifier::BOLD))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.command_palette.border.to_color()));

                    let inner = block.inner(dialog_area);
                    frame.render_widget(block, dialog_area);

                    // Input line with cursor
                    let input_line = Line::from(vec![
                        Span::styled(&thread_rename_text, Style::default().fg(theme.command_palette.input_fg.to_color())),
                        Span::styled("▎", Style::default().fg(theme.colors.primary.to_color())),
                    ]);
                    frame.render_widget(Paragraph::new(input_line), inner);

                    // Help text below
                    let help_area = ratatui::layout::Rect::new(inner.x, inner.y + 1, inner.width, 1);
                    let help_text = Paragraph::new("Enter to confirm, Esc to cancel")
                        .style(Style::default().fg(theme.command_palette.description_fg.to_color()))
                        .alignment(Alignment::Center);
                    frame.render_widget(help_text, help_area);
                }

                // T2.4: Ask user dialog overlay
                if show_ask_user {
                    self.ask_user_dialog.render(frame, size, &theme);
                }

                // TRC-020: Context menu overlay (highest z-index)
                if show_context_menu {
                    self.context_menu.render(frame, size, &theme);
                }
                
                // TRC-023: Notifications overlay (top-right, highest z-index)
                if has_notifications {
                    self.notification_manager.render(frame, size, &theme);
                }
            })
            .map_err(|e| RidgeError::Terminal(e.to_string()))?;

        Ok(())
    }

    fn handle_event(&mut self, event: CrosstermEvent) -> Option<Action> {
        match event {
            CrosstermEvent::Key(key) => self.handle_key(key),
            CrosstermEvent::Mouse(mouse) => self.handle_mouse(mouse),
            CrosstermEvent::Paste(text) => self.handle_paste(text),
            CrosstermEvent::Resize(cols, rows) => {
                let (term_cols, term_rows) = Self::calculate_terminal_size(Rect::new(0, 0, cols, rows));
                Some(Action::PtyResize {
                    cols: term_cols as u16,
                    rows: term_rows as u16,
                })
            }
            _ => None,
        }
    }

    /// Handle bracketed paste events - route to appropriate component based on focus
    fn handle_paste(&mut self, text: String) -> Option<Action> {
        // Route paste based on focus and editing state
        if self.show_settings_editor && self.settings_editor.is_editing() {
            // Paste to settings editor when editing
            self.settings_editor.paste_text(&text);
            None
        } else if self.focus.is_focused(FocusArea::ChatInput) {
            // Paste to chat input when focused
            self.chat_input.paste_text(&text);
            None
        } else {
            // Otherwise paste to active tab's PTY
            self.tab_manager.write_to_active_pty(text.into_bytes());
            None
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Debug: Log key events to help diagnose input issues
        #[cfg(debug_assertions)]
        tracing::debug!("Key event: {:?}, mode: {:?}, focus: {:?}", key, self.input_mode, self.focus.current());

        // T2.4: Ask user dialog takes priority when visible
        if self.ask_user_dialog.is_visible() {
            return self.ask_user_dialog.handle_event(&CrosstermEvent::Key(key));
        }

        match &self.input_mode {
            InputMode::Confirm { .. } => {
                self.confirm_dialog.handle_event(&CrosstermEvent::Key(key))
            }
            InputMode::CommandPalette => {
                self.command_palette.handle_event(&CrosstermEvent::Key(key))
            }
            InputMode::PtyRaw => {
                // First check configurable keybindings
                if let Some(action) = self.config_manager.keybindings().get_action(&self.input_mode, &key) {
                    return Some(action);
                }

                // Esc exits PTY mode (like vim's Esc to exit insert mode)
                if key.code == KeyCode::Esc {
                    return Some(Action::EnterNormalMode);
                }

                // Copy with selection (special handling) - use active tab's terminal (TRC-005)
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    let has_selection = self.tab_manager
                        .active_pty_session()
                        .map(|s| s.terminal().has_selection())
                        .unwrap_or(false);
                    if has_selection {
                        return Some(Action::Copy);
                    }
                }

                // Pass through to PTY
                let bytes = key_to_bytes(key);
                if !bytes.is_empty() {
                    return Some(Action::PtyInput(bytes));
                }
                None
            }
            InputMode::Normal => {
                // Focus-specific key handling FIRST (so j/k work per-pane)
                let focus_action = match self.focus.current() {
                    FocusArea::Terminal => {
                        // Enter key should enter PTY mode when terminal is focused
                        // This is a hardcoded fallback in case keybinding lookup fails
                        if key.code == KeyCode::Enter && key.modifiers.is_empty() {
                            return Some(Action::EnterPtyMode);
                        }
                        None
                    }
                    FocusArea::ProcessMonitor => {
                        self.process_monitor.handle_event(&CrosstermEvent::Key(key))
                    }
                    FocusArea::Menu => {
                        self.menu.handle_event(&CrosstermEvent::Key(key))
                    }
                    FocusArea::StreamViewer => {
                        // Handle StreamViewer key events (also used for conversation history)
                        // 'i' focuses chat input when conversation is visible
                        if self.show_conversation && (key.code == KeyCode::Char('i') || key.code == KeyCode::Tab) {
                            self.focus.focus(FocusArea::ChatInput);
                            return None;
                        }
                        
                        // When conversation is visible, route scroll keys to conversation viewer
                        if self.show_conversation {
                            match key.code {
                                KeyCode::Char('j') | KeyCode::Down => Some(Action::ConversationScrollDown(1)),
                                KeyCode::Char('k') | KeyCode::Up => Some(Action::ConversationScrollUp(1)),
                                KeyCode::Char('g') => Some(Action::ConversationScrollToTop),
                                KeyCode::Char('G') => Some(Action::ConversationScrollToBottom),
                                KeyCode::PageUp => Some(Action::ConversationScrollUp(10)),
                                KeyCode::PageDown => Some(Action::ConversationScrollDown(10)),
                                KeyCode::Char('a') => {
                                    self.conversation_viewer.toggle_auto_scroll();
                                    None
                                }
                                KeyCode::Esc | KeyCode::Char('q') => Some(Action::StreamViewerHide),
                                _ => None,
                            }
                        } else {
                            match key.code {
                                KeyCode::Char('j') | KeyCode::Down => Some(Action::StreamViewerScrollDown(1)),
                                KeyCode::Char('k') | KeyCode::Up => Some(Action::StreamViewerScrollUp(1)),
                                KeyCode::Char('g') => Some(Action::StreamViewerScrollToTop),
                                KeyCode::Char('G') => Some(Action::StreamViewerScrollToBottom),
                                KeyCode::PageUp => Some(Action::StreamViewerScrollUp(10)),
                                KeyCode::PageDown => Some(Action::StreamViewerScrollDown(10)),
                                KeyCode::Esc | KeyCode::Char('q') => Some(Action::StreamViewerHide),
                                _ => None,
                            }
                        }
                    }
                    FocusArea::ConfigPanel => {
                        // Handle ConfigPanel key events (TRC-014)
                        self.config_panel.handle_event(&CrosstermEvent::Key(key))
                    }
                    FocusArea::LogViewer => {
                        // Handle LogViewer key events (TRC-013)
                        match key.code {
                            KeyCode::Char('j') | KeyCode::Down => Some(Action::LogViewerScrollDown(1)),
                            KeyCode::Char('k') | KeyCode::Up => Some(Action::LogViewerScrollUp(1)),
                            KeyCode::Char('g') => Some(Action::LogViewerScrollToTop),
                            KeyCode::Char('G') => Some(Action::LogViewerScrollToBottom),
                            KeyCode::PageUp => Some(Action::LogViewerScrollPageUp),
                            KeyCode::PageDown => Some(Action::LogViewerScrollPageDown),
                            KeyCode::Char('a') => Some(Action::LogViewerToggleAutoScroll),
                            KeyCode::Char('c') => Some(Action::LogViewerClear),
                            KeyCode::Esc | KeyCode::Char('q') => Some(Action::LogViewerHide),
                            _ => None,
                        }
                    }
                    FocusArea::ChatInput => {
                        // Handle ChatInput key events - delegate to component
                        // Escape returns focus to conversation viewer
                        if key.code == KeyCode::Esc {
                            self.focus.focus(FocusArea::StreamViewer);
                            return None;
                        }
                        self.chat_input.handle_event(&CrosstermEvent::Key(key))
                    }
                    FocusArea::SettingsEditor => {
                        // Handle SettingsEditor key events (TS-012)
                        self.settings_editor.handle_event(&CrosstermEvent::Key(key))
                    }
                };

                // If focus-specific handler returned an action, use it
                if focus_action.is_some() {
                    return focus_action;
                }

                // F1 through F9 for direct tab selection (hardcoded for convenience)
                if let KeyCode::F(n @ 1..=9) = key.code {
                    if key.modifiers.is_empty() {
                        return Some(Action::TabSelect((n - 1) as usize));
                    }
                }

                // Fall back to global keybindings (scroll, quit, etc.)
                self.config_manager.keybindings().get_action(&self.input_mode, &key)
            }
            InputMode::Insert { ref target } => {
                // TRC-029: Handle inline tab rename input
                if matches!(target, crate::input::mode::InsertTarget::TabRename) {
                    match key.code {
                        KeyCode::Esc => return Some(Action::TabCancelRename),
                        KeyCode::Enter => {
                            // Confirm rename and exit insert mode
                            self.tab_manager.confirm_rename();
                            self.input_mode = InputMode::Normal;
                            return None;
                        }
                        KeyCode::Backspace => return Some(Action::TabRenameBackspace),
                        KeyCode::Char(c) => return Some(Action::TabRenameInput(c)),
                        _ => {}
                    }
                }

                // P2-003: Handle inline thread rename input
                if matches!(target, crate::input::mode::InsertTarget::ThreadRename) {
                    match key.code {
                        KeyCode::Esc => {
                            self.thread_rename_buffer = None;
                            self.input_mode = InputMode::Normal;
                            return Some(Action::ThreadCancelRename);
                        }
                        KeyCode::Enter => {
                            // Confirm rename
                            if let Some(new_name) = self.thread_rename_buffer.take() {
                                self.input_mode = InputMode::Normal;
                                return Some(Action::ThreadRename(new_name));
                            }
                            self.input_mode = InputMode::Normal;
                            return None;
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut buffer) = self.thread_rename_buffer {
                                buffer.pop();
                            }
                            return None;
                        }
                        KeyCode::Char(c) => {
                            if let Some(ref mut buffer) = self.thread_rename_buffer {
                                buffer.push(c);
                            }
                            return None;
                        }
                        _ => {}
                    }
                }

                // Fall back to configurable keybindings for other insert targets
                if let Some(action) = self.config_manager.keybindings().get_action(&self.input_mode, &key) {
                    return Some(action);
                }
                None
            }
            InputMode::ThreadPicker => {
                // P2-003: Route keyboard events to thread picker component
                if let Some(action) = self.thread_picker.handle_event(&CrosstermEvent::Key(key)) {
                    return Some(action);
                }
                // If picker was hidden (Esc pressed), return to normal mode
                if !self.thread_picker.is_visible() {
                    self.input_mode = InputMode::Normal;
                }
                None
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        // TRC-020: If context menu is visible, route all mouse events to it first
        if self.context_menu.is_visible() {
            if let Some(action) = self.context_menu.handle_event(&CrosstermEvent::Mouse(mouse)) {
                return Some(action);
            }
            // If click was outside menu, the handler returns ContextMenuClose
            // For any other event not handled by context menu, consume it
            return None;
        }
        
        // TRC-020: Handle right-click to show context menus
        if let MouseEventKind::Down(MouseButton::Right) = mouse.kind {
            return self.handle_right_click(mouse.column, mouse.row);
        }
        
        // TRC-010: Check for clicks on the tab bar first (before focus-based routing)
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            // Check if click is within tab bar area
            if self.tab_bar_area.height > 0
                && mouse.row >= self.tab_bar_area.y
                && mouse.row < self.tab_bar_area.y + self.tab_bar_area.height
                && mouse.column >= self.tab_bar_area.x
                && mouse.column < self.tab_bar_area.x + self.tab_bar_area.width
            {
                // Hit-test against tabs
                let tab_bar = TabBar::from_manager(&self.tab_manager);
                let hit_areas = tab_bar.calculate_hit_areas(self.tab_bar_area);
                
                for (start_x, end_x, tab_index) in hit_areas {
                    if mouse.column >= start_x && mouse.column < end_x {
                        return Some(Action::TabSelect(tab_index));
                    }
                }
                // Click was in tab bar but not on a tab - consume event
                return None;
            }
            
            // TRC-024: Check for clicks on pane borders for resize
            let show_conv = self.show_conversation || !self.llm_response_buffer.is_empty() || !self.thinking_buffer.is_empty();
            if let Some(border) = self.pane_layout.hit_test_border(mouse.column, mouse.row, self.content_area, show_conv) {
                let pan_border = match border {
                    ResizableBorder::MainVertical => PaneBorder::MainVertical,
                    ResizableBorder::RightHorizontal => PaneBorder::RightHorizontal,
                    ResizableBorder::LeftHorizontal => PaneBorder::LeftHorizontal,
                };
                return Some(Action::PaneStartDrag(pan_border));
            }
        }
        
        // TRC-024: Handle drag events for pane resizing
        if self.drag_state.is_dragging() {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    return Some(Action::PaneDrag { x: mouse.column, y: mouse.row });
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    return Some(Action::PaneEndDrag);
                }
                _ => {}
            }
        }
        
        // Handle ongoing conversation text selection (drag/up events while selecting)
        if self.conversation_viewer.is_selecting() {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(action) = self.conversation_viewer.handle_mouse(mouse) {
                        return Some(action);
                    }
                    return None;
                }
                _ => {}
            }
        }

        // Handle ongoing chat input text selection (drag/up events while selecting)
        if self.chat_input.is_selecting() {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(action) = self.chat_input.handle_mouse(mouse) {
                        return Some(action);
                    }
                    return None;
                }
                _ => {}
            }
        }

        // Mouse events over conversation area - route to conversation viewer for selection
        if self.conversation_area.height > 0 {
            let in_conversation = mouse.row >= self.conversation_area.y
                && mouse.row < self.conversation_area.y + self.conversation_area.height
                && mouse.column >= self.conversation_area.x
                && mouse.column < self.conversation_area.x + self.conversation_area.width;

            if in_conversation {
                // Route mouse events to conversation viewer for text selection
                if let Some(action) = self.conversation_viewer.handle_mouse(mouse) {
                    return Some(action);
                }
                // Focus on click if no action returned
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    self.focus.focus(FocusArea::StreamViewer);
                }
                return None;
            }
        }

        // Mouse events over chat input area - route to chat_input for selection
        if self.chat_input_area.height > 0 {
            let in_chat_input = mouse.row >= self.chat_input_area.y
                && mouse.row < self.chat_input_area.y + self.chat_input_area.height
                && mouse.column >= self.chat_input_area.x
                && mouse.column < self.chat_input_area.x + self.chat_input_area.width;

            if in_chat_input {
                // Route mouse events to chat input for text selection
                if let Some(action) = self.chat_input.handle_mouse(mouse) {
                    return Some(action);
                }
                // Focus on click if no action returned
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    self.focus.focus(FocusArea::ChatInput);
                }
                return None;
            }
        }

        // Focus-based mouse handling
        match self.focus.current() {
            FocusArea::Terminal => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left) => {
                    // Use active tab's terminal widget (TRC-005)
                    if let Some(session) = self.tab_manager.active_pty_session_mut() {
                        session.terminal_mut()
                            .handle_event(&CrosstermEvent::Mouse(mouse))
                    } else {
                        None
                    }
                }
                MouseEventKind::ScrollUp => Some(Action::ScrollUp(3)),
                MouseEventKind::ScrollDown => Some(Action::ScrollDown(3)),
                _ => None,
            },
            FocusArea::ProcessMonitor => {
                self.process_monitor
                    .handle_event(&CrosstermEvent::Mouse(mouse))
            }
            FocusArea::Menu => {
                self.menu.handle_event(&CrosstermEvent::Mouse(mouse))
            }
            // Overlay areas
            FocusArea::StreamViewer => {
                // Handle mouse scroll for conversation/stream viewer
                if self.show_conversation {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => Some(Action::ConversationScrollUp(3)),
                        MouseEventKind::ScrollDown => Some(Action::ConversationScrollDown(3)),
                        _ => None,
                    }
                } else {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => Some(Action::StreamViewerScrollUp(3)),
                        MouseEventKind::ScrollDown => Some(Action::StreamViewerScrollDown(3)),
                        _ => None,
                    }
                }
            }
            FocusArea::ConfigPanel => {
                // Handle ConfigPanel mouse events (TRC-014)
                self.config_panel.handle_event(&CrosstermEvent::Mouse(mouse))
            }
            FocusArea::LogViewer => {
                // Handle LogViewer mouse events (TRC-013)
                match mouse.kind {
                    MouseEventKind::ScrollUp => Some(Action::LogViewerScrollUp(3)),
                    MouseEventKind::ScrollDown => Some(Action::LogViewerScrollDown(3)),
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Click on title bar area toggles auto-scroll
                        Some(Action::LogViewerToggleAutoScroll)
                    }
                    _ => None,
                }
            }
            FocusArea::ChatInput => {
                // ChatInput doesn't handle mouse events currently
                None
            }
            FocusArea::SettingsEditor => {
                // Handle SettingsEditor mouse events (TS-012)
                self.settings_editor.handle_event(&CrosstermEvent::Mouse(mouse))
            }
        }
    }
    
    /// TRC-020: Handle right-click to determine context menu target and items
    fn handle_right_click(&mut self, x: u16, y: u16) -> Option<Action> {
        let term_size = self.terminal.size().ok()?;
        let screen = Rect::new(0, 0, term_size.width, term_size.height);
        
        // Check tab bar first
        if self.tab_bar_area.height > 0 && self.tab_bar_area.contains((x, y).into()) {
            let tab_bar = TabBar::from_manager(&self.tab_manager);
            let hit_areas = tab_bar.calculate_hit_areas(self.tab_bar_area);
            
            for (start_x, end_x, tab_index) in hit_areas {
                if x >= start_x && x < end_x {
                    return Some(Action::ContextMenuShow { 
                        x, 
                        y, 
                        target: ContextMenuTarget::Tab(tab_index) 
                    });
                }
            }
            return None;
        }
        
        // Determine target based on position
        // Calculate layout areas (same as in draw())
        let show_tabs = self.tab_manager.count() > 1 || self.dangerous_mode;
        let content_y = if show_tabs { 1 } else { 0 };
        let content_height = screen.height.saturating_sub(content_y);
        let content_area = Rect::new(0, content_y, screen.width, content_height);
        
        // Main layout: 67% left, 33% right
        let left_width = (content_area.width * 67) / 100;
        
        // Right side: 50% top (process monitor), 50% bottom (menu)
        let right_top_height = content_height / 2;
        let right_bottom_y = content_y + right_top_height;
        
        // Determine which area was clicked
        // Check chat input area first (it's on the left side, need to check before Terminal)
        if self.chat_input_area.height > 0 && self.chat_input_area.contains((x, y).into()) {
            // Focus ChatInput when right-clicking on it
            self.focus.focus(FocusArea::ChatInput);
            Some(Action::ContextMenuShow {
                x,
                y,
                target: ContextMenuTarget::ChatInput,
            })
        } else if self.conversation_area.height > 0 && self.conversation_area.contains((x, y).into()) {
            // Conversation viewer area
            self.focus.focus(FocusArea::StreamViewer);
            Some(Action::ContextMenuShow {
                x,
                y,
                target: ContextMenuTarget::Conversation,
            })
        } else if x < left_width {
            // Terminal area
            Some(Action::ContextMenuShow {
                x,
                y,
                target: ContextMenuTarget::Terminal,
            })
        } else if y < right_bottom_y {
            // Process monitor area
            // Try to find which process was clicked - pass raw screen Y
            let selected_pid = self.process_monitor.get_pid_at_screen_y(y);
            
            if let Some(pid) = selected_pid {
                Some(Action::ContextMenuShow {
                    x,
                    y,
                    target: ContextMenuTarget::Process(pid),
                })
            } else {
                Some(Action::ContextMenuShow {
                    x,
                    y,
                    target: ContextMenuTarget::Generic,
                })
            }
        } else {
            // Menu area - check for stream
            let stream_idx = self.menu.selected_index();
            Some(Action::ContextMenuShow {
                x,
                y,
                target: ContextMenuTarget::Stream(stream_idx),
            })
        }
    }

    fn dispatch(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit | Action::ForceQuit => {
                // Auto-save session on quit (TRC-012)
                self.save_session();
                self.should_quit = true;
            }
            Action::EnterPtyMode => {
                self.input_mode = InputMode::PtyRaw;
                self.focus.focus(FocusArea::Terminal);
                // Scroll active tab's terminal to bottom (TRC-005)
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().scroll_to_bottom();
                }
            }
            Action::EnterNormalMode => {
                self.input_mode = InputMode::Normal;
                // Also close command palette if open
                if self.command_palette.is_visible() {
                    self.command_palette.hide();
                }
            }
            Action::OpenCommandPalette => {
                // Populate dynamic provider/model commands before showing
                let providers = self.model_catalog.providers();
                let current_provider = self.agent_engine.current_provider();
                let current_provider = if current_provider.is_empty() { "anthropic" } else { current_provider };
                self.command_palette.set_providers(&providers, current_provider);

                let models = self.model_catalog.models_for_provider(current_provider);
                let current_model = self.agent_engine.current_model();
                self.command_palette.set_models(&models, current_model);

                // Populate subagent model commands (T2.1b)
                self.refresh_subagent_commands();

                self.command_palette.show();
                self.input_mode = InputMode::CommandPalette;
            }
            Action::CloseCommandPalette => {
                self.command_palette.hide();
                self.input_mode = InputMode::Normal;
            }
            Action::FocusNext => {
                let skip_chat = !self.show_conversation;
                self.focus.next_skip_chat(skip_chat);
                if self.focus.current() == FocusArea::ProcessMonitor {
                    self.process_monitor.ensure_selection();
                }
            }
            Action::FocusPrev => {
                let skip_chat = !self.show_conversation;
                self.focus.prev_skip_chat(skip_chat);
                if self.focus.current() == FocusArea::ProcessMonitor {
                    self.process_monitor.ensure_selection();
                }
            }
            Action::FocusArea(area) => {
                self.focus.focus(area);
                if area == FocusArea::ProcessMonitor {
                    self.process_monitor.ensure_selection();
                }
            }
            Action::PtyInput(data) => {
                // Write to active tab's PTY (TRC-005)
                self.tab_manager.write_to_active_pty(data);
            }
            Action::PtyOutput(_data) => {
                // PTY output is now handled by poll_pty_events (TRC-005)
                // This action is kept for backward compatibility but not used directly
            }
            Action::PtyResize { cols, rows } => {
                // Resize all PTY sessions (TRC-005)
                self.tab_manager.set_terminal_size(cols, rows);
            }
            Action::ScrollUp(n) => {
                // Scroll active tab's terminal (TRC-005)
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollUp(n));
                }
            }
            Action::ScrollDown(n) => {
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollDown(n));
                }
            }
            Action::ScrollPageUp => {
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollPageUp);
                }
            }
            Action::ScrollPageDown => {
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollPageDown);
                }
            }
            Action::ScrollToTop => {
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollToTop);
                }
            }
            Action::ScrollToBottom => {
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollToBottom);
                }
            }
            Action::Copy => {
                // Copy from active tab's terminal (TRC-005)
                if let Some(session) = self.tab_manager.active_pty_session_mut() {
                    if let Some(text) = session.terminal().get_selected_text() {
                        if let Some(ref mut clipboard) = self.clipboard {
                            let _ = clipboard.set_text(text);
                        }
                    }
                    session.terminal_mut().clear_selection();
                }
            }
            Action::Paste => {
                // Route paste based on focus and editing state
                if let Some(ref mut clipboard) = self.clipboard {
                    if let Ok(text) = clipboard.get_text() {
                        // If settings editor is visible and in editing mode, paste there
                        if self.show_settings_editor && self.settings_editor.is_editing() {
                            self.settings_editor.paste_text(&text);
                        } else if self.focus.is_focused(FocusArea::ChatInput) {
                            // Paste to chat input when focused
                            self.chat_input.paste_text(&text);
                        } else {
                            // Otherwise paste to active tab's PTY (TRC-005)
                            self.tab_manager.write_to_active_pty(text.into_bytes());
                        }
                    }
                }
            }
            Action::MenuSelectNext => {
                self.menu.update(&Action::MenuSelectNext);
                // Sync selected_stream_index with menu selection
                let idx = self.menu.selected_index();
                self.selected_stream_index = Some(idx);
            }
            Action::MenuSelectPrev => {
                self.menu.update(&Action::MenuSelectPrev);
                // Sync selected_stream_index with menu selection
                let idx = self.menu.selected_index();
                self.selected_stream_index = Some(idx);
            }
            Action::MenuSelected(idx) => {
                // Direct selection update (e.g., from mouse click)
                self.selected_stream_index = Some(idx);
            }
            Action::StreamConnect(idx) => {
                if let Some(client) = self.stream_manager.clients().get(idx) {
                    let id = client.id().to_string();
                    self.stream_manager.connect(&id);
                }
            }
            Action::StreamDisconnect(idx) => {
                if let Some(client) = self.stream_manager.clients().get(idx) {
                    let id = client.id().to_string();
                    self.stream_manager.disconnect(&id);
                }
            }
            Action::StreamToggle(idx) => {
                let id_and_state = self.stream_manager.clients().get(idx).map(|c| {
                    (c.id().to_string(), c.state())
                });
                if let Some((id, state)) = id_and_state {
                    match state {
                        ConnectionState::Connected => self.stream_manager.disconnect(&id),
                        _ => self.stream_manager.connect(&id),
                    }
                }
            }
            Action::StreamRefresh => {
                // TRC-028: Use centralized reload method for consistency
                self.reload_streams_from_config();
            }
            Action::StreamRetry(idx) => {
                // TRC-025: Retry connection for failed stream, resetting health
                let info = self.stream_manager.clients().get(idx)
                    .map(|c| (c.id().to_string(), c.name().to_string()));
                if let Some((id, name)) = info {
                    self.stream_manager.retry(&id);
                    self.notification_manager.info(format!("Retrying {}...", name));
                }
            }
            Action::StreamCancelReconnect(idx) => {
                // TRC-025: Cancel ongoing reconnection
                if let Some(client) = self.stream_manager.clients().get(idx) {
                    let id = client.id().to_string();
                    self.stream_manager.cancel_reconnect(&id);
                }
            }
            Action::StreamViewerShow(idx) => {
                self.selected_stream_index = Some(idx);
                self.show_stream_viewer = true;
                self.focus.focus(FocusArea::StreamViewer);
            }
            Action::StreamViewerHide => {
                self.show_stream_viewer = false;
                self.focus.focus(FocusArea::Menu);
            }
            Action::StreamViewerToggle => {
                if self.show_stream_viewer {
                    self.show_stream_viewer = false;
                    self.focus.focus(FocusArea::Menu);
                } else if self.selected_stream_index.is_some() {
                    self.show_stream_viewer = true;
                    self.focus.focus(FocusArea::StreamViewer);
                }
            }
            Action::StreamViewerScrollUp(n) => {
                self.stream_viewer.scroll_up(n);
            }
            Action::StreamViewerScrollDown(n) => {
                self.stream_viewer.scroll_down(n);
            }
            Action::StreamViewerScrollToTop => {
                self.stream_viewer.scroll_to_top();
            }
            Action::StreamViewerScrollToBottom => {
                self.stream_viewer.scroll_to_bottom();
            }
            Action::ProcessRefresh
            | Action::ProcessSelectNext
            | Action::ProcessSelectPrev
            | Action::ProcessKillRequest(_)
            | Action::ProcessKillConfirm(_)
            | Action::ProcessKillCancel
            | Action::ProcessSetFilter(_)
            | Action::ProcessClearFilter
            | Action::ProcessSetSort(_)
            | Action::ProcessToggleSortOrder => {
                self.process_monitor.update(&action);
            }
            Action::Tick => {
                self.process_monitor.update(&Action::Tick);
                // Tick all active spinners (TRC-015)
                self.spinner_manager.tick();
                // Tick menu spinners for stream connection animations
                self.menu.tick_spinners();
                // Tick conversation viewer spinner for LLM streaming
                self.conversation_viewer.tick_spinner();
                // Tick notifications to expire old ones (TRC-023)
                self.notification_manager.tick();
            }
            Action::LlmSendMessage(msg) => {
                tracing::info!("Sending LLM message: {} chars", msg.len());
                // Ensure conversation is visible when sending a message
                if !self.show_conversation {
                    self.show_conversation = true;
                }
                
                // Route through AgentEngine (always available)
                // Ensure we have an active thread
                if self.agent_engine.current_thread().is_none() {
                    let model = self.agent_engine.current_model().to_string();
                    self.agent_engine.new_thread(model);
                    // TP2-002-15: Update current_thread_id when auto-creating thread
                    self.current_thread_id = self.agent_engine.current_thread().map(|t| t.id.clone());
                    tracing::info!("Created new AgentEngine thread: {:?}", self.current_thread_id);
                }

                // Send message through AgentEngine
                self.agent_engine.send_message(msg);
                tracing::info!("Message sent through AgentEngine");
            }
            Action::LlmCancel => {
                // Cancel AgentEngine's internal LLM
                self.agent_engine.cancel();
                // Immediately stop spinner and clear buffers for responsive UI
                // (don't wait for async AgentEvent::Error to propagate)
                self.spinner_manager.stop(&SpinnerKey::LlmLoading);
                self.llm_response_buffer.clear();
                self.thinking_buffer.clear();
                self.current_block_type = None;
                self.current_tool_id = None;
                self.current_tool_name = None;
                self.current_tool_input.clear();
                self.notification_manager.info_with_message("Request Cancelled", "LLM request interrupted by user");
            }
            Action::LlmSelectModel(model) => {
                // Update AgentEngine's LLMManager
                self.agent_engine.set_model(&model);
                // Also persist to config so model is remembered on restart
                self.config_manager.llm_config_mut().defaults.model = model.clone();
                if let Err(e) = self.config_manager.save_llm_config() {
                    tracing::warn!("Failed to save model selection: {}", e);
                }
            }
            Action::LlmSelectProvider(provider) => {
                // Update AgentEngine's LLMManager
                self.agent_engine.set_provider(&provider);
            }
            Action::LlmClearConversation => {
                // Start a new thread to clear conversation (AgentEngine tracks via thread)
                let model = self.agent_engine.current_model().to_string();
                self.agent_engine.new_thread(model);
                self.current_thread_id = self.agent_engine.current_thread().map(|t| t.id.clone());
                // Also clear tool calls in conversation viewer (TRC-016)
                self.conversation_viewer.clear_tool_calls();
            }
            // Chat input actions
            Action::ChatInputClear => {
                self.chat_input.clear();
            }
            Action::ChatInputPaste(text) => {
                self.chat_input.paste_text(&text);
            }
            Action::ChatInputCopy => {
                // Copy selected text from chat input to clipboard
                if let Some(text) = self.chat_input.get_selected_text() {
                    if let Some(ref mut clipboard) = self.clipboard {
                        let _ = clipboard.set_text(&text);
                        self.notification_manager.info("Copied to clipboard");
                    }
                }
                self.chat_input.clear_selection();
            }
            Action::ChatInputScrollUp(n) => {
                self.chat_input.scroll_up(n);
            }
            Action::ChatInputScrollDown(n) => {
                self.chat_input.scroll_down(n);
            }
            // Subagent configuration actions (T2.1b)
            Action::SubagentSelectModel { agent_type, model } => {
                self.config_manager.subagent_config_mut().get_mut(&agent_type).model = model;
                if let Err(e) = self.config_manager.save_subagent_config() {
                    tracing::warn!("Failed to save subagent config: {}", e);
                }
                // Refresh command palette to show updated checkmarks
                self.refresh_subagent_commands();
            }
            Action::SubagentSelectProvider { agent_type, provider } => {
                self.config_manager.subagent_config_mut().get_mut(&agent_type).provider = provider;
                if let Err(e) = self.config_manager.save_subagent_config() {
                    tracing::warn!("Failed to save subagent config: {}", e);
                }
                // Refresh command palette to show models for new provider
                self.refresh_subagent_commands();
            }
            Action::LlmStreamChunk(_) | Action::LlmStreamComplete | Action::LlmStreamError(_) => {
                // These are handled by handle_llm_event, not dispatched directly
            }
            
            // Tool execution actions
            Action::ToolUseReceived(pending) => {
                self.handle_tool_use_request(pending.tool.clone());
            }
            Action::ToolConfirm => {
                // User confirmed tool execution
                self.confirm_dialog.dismiss();
                self.input_mode = InputMode::Normal;
                
                // Get the tool from pending_tools using confirming_tool_id
                if let Some(tool_id) = self.confirming_tool_id.take() {
                    if let Some(pending) = self.pending_tools.remove(&tool_id) {
                        // Update check to Allowed since user confirmed
                        let confirmed_pending = PendingToolUse::new(
                            pending.tool,
                            ToolExecutionCheck::Allowed
                        );
                        self.execute_tool(confirmed_pending);
                    }
                }
            }
            Action::ToolReject => {
                // User rejected tool execution
                self.confirm_dialog.dismiss();
                self.input_mode = InputMode::Normal;
                
                // Get the tool from pending_tools using confirming_tool_id
                if let Some(tool_id) = self.confirming_tool_id.take() {
                    if let Some(pending) = self.pending_tools.remove(&tool_id) {
                        // Update tool state in conversation viewer (TRC-016)
                        self.conversation_viewer.reject_tool(&pending.tool.id);
                        
                        // Create rejection result
                        let error_result = crate::llm::ToolResult {
                            tool_use_id: pending.tool.id.clone(),
                            content: crate::llm::ToolResultContent::Text(
                                "User rejected tool execution".to_string()
                            ),
                            is_error: true,
                        };
                        
                        // TP2-002-12: Bridge tool rejection to AgentEngine if active
                        // Collect rejection as a result
                        self.collected_results.insert(pending.tool.id.clone(), error_result);

                        // Check if we have all results now
                        if self.collected_results.len() >= self.expected_tool_count && self.expected_tool_count > 0 {
                            let all_results: Vec<crate::llm::ToolResult> = self.collected_results.drain().map(|(_, r)| r).collect();
                            self.agent_engine.continue_after_tools(all_results);
                            self.expected_tool_count = 0;
                        }
                    }
                }
            }
            Action::ToolResult(result) => {
                let tool_use_id = result.tool_use_id.clone();
                
                // Update tool state in conversation viewer (TRC-016)
                let tool_name = self.pending_tools.get(&tool_use_id)
                    .map(|p| p.tool_name().to_string())
                    .unwrap_or_else(|| "Tool".to_string());
                self.conversation_viewer.complete_tool(&tool_use_id, result.clone());
                
                // TRC-023: Notify on tool completion
                if result.is_error {
                    self.notification_manager.warning_with_message(
                        format!("{} failed", tool_name),
                        "See conversation for details".to_string()
                    );
                }
                
                // Remove from pending_tools since we got the result
                self.pending_tools.remove(&tool_use_id);
                
                // TP2-002-12: Bridge tool result to AgentEngine
                // Collect this result
                self.collected_results.insert(tool_use_id.clone(), result);

                tracing::info!(
                    "📥 TOOL_RESULT collected: id={}, collected={}/{} expected",
                    tool_use_id, self.collected_results.len(), self.expected_tool_count
                );

                // Only continue when we have ALL expected results
                if self.collected_results.len() >= self.expected_tool_count && self.expected_tool_count > 0 {
                    // Collect all results and send them together
                    let all_results: Vec<crate::llm::ToolResult> = self.collected_results.drain().map(|(_, r)| r).collect();

                    tracing::info!(
                        "✅ ALL_TOOLS_COMPLETE: sending {} results to engine",
                        all_results.len()
                    );

                    self.agent_engine.continue_after_tools(all_results);

                    // Reset tracking state for next tool batch
                    self.expected_tool_count = 0;
                }
            }
            Action::ToolToggleDangerousMode => {
                let current = self.dangerous_mode;
                self.set_dangerous_mode(!current);
            }
            Action::ToolSetDangerousMode(enabled) => {
                self.set_dangerous_mode(enabled);
            }
            
            // Thread management actions (Phase 2)
            Action::ThreadNew => {
                let model = self.agent_engine.current_model().to_string();
                self.agent_engine.new_thread(model);
                self.current_thread_id = self.agent_engine.current_thread().map(|t| t.id.clone());
                self.conversation_viewer.clear();
                self.notification_manager.info("New conversation thread started");
                tracing::info!("Created new thread: {:?}", self.current_thread_id);
            }
            Action::ThreadLoad(id) => {
                // TP2-002-09: Load existing thread by ID
                match self.agent_engine.load_thread(&id) {
                    Ok(()) => {
                        // Update current thread ID
                        self.current_thread_id = self.agent_engine.current_thread().map(|t| t.id.clone());

                        // Clear conversation viewer state
                        self.conversation_viewer.clear();

                        // Phase 2: Re-populate tool calls from loaded thread segments
                        // Register all tool uses first, then complete with results
                        if let Some(thread) = self.agent_engine.current_thread() {
                            for segment in thread.segments() {
                                for message in &segment.messages {
                                    for content_block in &message.content {
                                        match content_block {
                                            crate::llm::ContentBlock::ToolUse(tool_use) => {
                                                self.conversation_viewer.register_tool_use(tool_use.clone());
                                            }
                                            crate::llm::ContentBlock::ToolResult(result) => {
                                                // Complete the tool with its result
                                                self.conversation_viewer.complete_tool(&result.tool_use_id, result.clone());
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }

                            let title = thread.title.clone();
                            self.notification_manager.info(format!("Loaded thread: {}", title));
                            tracing::info!("Loaded thread: {} ({})", title, id);
                        }
                    }
                    Err(e) => {
                        self.notification_manager.error_with_message("Failed to load thread", e.clone());
                        tracing::error!("Failed to load thread {}: {}", id, e);
                    }
                }
            }
            Action::ThreadList => {
                // Future: Show thread list UI
            }
            Action::ThreadSave => {
                // TP2-002-10: Manually save current thread to DiskThreadStore
                match self.agent_engine.save_thread() {
                    Ok(()) => {
                        if let Some(thread) = self.agent_engine.current_thread() {
                            let title = thread.title.clone();
                            self.notification_manager.success(format!("Thread saved: {}", title));
                            tracing::info!("Manually saved thread: {} ({})", title, thread.id);
                        } else {
                            self.notification_manager.success("Thread saved");
                        }
                    }
                    Err(e) => {
                        self.notification_manager.error_with_message("Failed to save thread", e.clone());
                        tracing::error!("Failed to save thread: {}", e);
                    }
                }
            }
            Action::ThreadClear => {
                // TP2-002-11: Clear current thread (start fresh without deleting)
                if let Some(thread) = self.agent_engine.current_thread_mut() {
                    // Clear the thread segments
                    thread.clear();
                    // Clear the UI
                    self.conversation_viewer.clear();
                    // Notify user
                    self.notification_manager.info("Conversation cleared");
                    tracing::debug!("ThreadClear: cleared current thread and conversation viewer");
                } else {
                    self.notification_manager.warning("No active conversation to clear");
                    tracing::warn!("ThreadClear: no current thread to clear");
                }
            }

            // P2-003: Thread picker actions
            Action::ThreadPickerShow => {
                // Get thread summaries from DiskThreadStore
                let summaries = self.agent_engine.thread_store().list_summary();
                if summaries.is_empty() {
                    self.notification_manager.warning("No saved threads to continue");
                } else {
                    self.thread_picker.show(summaries);
                    self.input_mode = InputMode::ThreadPicker;
                    tracing::debug!("ThreadPickerShow: showing thread picker");
                }
            }
            Action::ThreadPickerHide => {
                self.thread_picker.hide();
                self.input_mode = InputMode::Normal;
                tracing::debug!("ThreadPickerHide: hiding thread picker");
            }

            // P2-003: Thread rename actions
            Action::ThreadStartRename => {
                if let Some(thread) = self.agent_engine.current_thread() {
                    // Initialize rename buffer with current title
                    self.thread_rename_buffer = Some(thread.title.clone());
                    self.input_mode = InputMode::Insert { target: crate::input::mode::InsertTarget::ThreadRename };
                    self.notification_manager.info("Editing thread name (Enter to confirm, Esc to cancel)");
                    tracing::debug!("ThreadStartRename: started rename mode with title '{}'", thread.title);
                } else {
                    self.notification_manager.warning("No active thread to rename");
                }
            }
            Action::ThreadCancelRename => {
                self.thread_rename_buffer = None;
                self.input_mode = InputMode::Normal;
                self.notification_manager.info("Rename cancelled");
                tracing::debug!("ThreadCancelRename: cancelled rename");
            }
            Action::ThreadRenameInput(c) => {
                if let Some(ref mut buffer) = self.thread_rename_buffer {
                    buffer.push(c);
                }
            }
            Action::ThreadRenameBackspace => {
                if let Some(ref mut buffer) = self.thread_rename_buffer {
                    buffer.pop();
                }
            }
            Action::ThreadRename(new_name) => {
                match self.agent_engine.rename_thread(&new_name) {
                    Ok(()) => {
                        self.notification_manager.success(format!("Thread renamed to '{}'", new_name));
                        tracing::info!("ThreadRename: renamed thread to '{}'", new_name);
                    }
                    Err(e) => {
                        self.notification_manager.error_with_message("Failed to rename thread", e.clone());
                        tracing::error!("ThreadRename: failed to rename - {}", e);
                    }
                }
                self.thread_rename_buffer = None;
                self.input_mode = InputMode::Normal;
            }

            // Config actions
            Action::ConfigChanged(path) => {
                tracing::info!("Config file changed: {}", path.display());
                self.config_manager.reload_file(&path);
                
                // TRC-028: Handle streams.toml changes - dynamically regenerate menu from config
                if path.file_name().and_then(|n| n.to_str()) == Some("streams.toml") {
                    self.reload_streams_from_config();
                }
                
                // Re-apply LLM settings when llm.toml changes (fixes model not updating after hot-reload)
                if path.file_name().and_then(|n| n.to_str()) == Some("llm.toml") {
                    let llm_config = self.config_manager.llm_config();
                    self.agent_engine.set_provider(&llm_config.defaults.provider);
                    self.agent_engine.set_model(&llm_config.defaults.model);
                    tracing::info!(
                        "Re-applied LLM settings after hot-reload: provider={}, model={}",
                        llm_config.defaults.provider,
                        llm_config.defaults.model
                    );
                }
            }
            Action::ConfigReload => {
                tracing::info!("Reloading all configuration files");
                self.config_manager.reload_all();
            }
            Action::ConfigApplyTheme => {
                tracing::debug!("Theme changes applied");
            }
            
            // Conversation viewer actions
            Action::ConversationToggle => {
                self.show_conversation = !self.show_conversation;
                // When opening conversation, focus the chat input for typing
                if self.show_conversation {
                    self.focus.focus(FocusArea::ChatInput);
                } else {
                    // When closing, return focus to terminal
                    self.focus.focus(FocusArea::Terminal);
                }
            }
            Action::ConversationScrollUp(n) => {
                self.conversation_viewer.scroll_up(n);
            }
            Action::ConversationScrollDown(n) => {
                self.conversation_viewer.scroll_down(n);
            }
            Action::ConversationScrollToTop => {
                self.conversation_viewer.scroll_to_top();
            }
            Action::ConversationScrollToBottom => {
                self.conversation_viewer.scroll_to_bottom();
            }
            Action::ConversationCopy => {
                // Copy selected text from conversation viewer to clipboard
                if let Some(text) = self.conversation_viewer.get_selected_text() {
                    if let Some(ref mut clipboard) = self.clipboard {
                        let _ = clipboard.set_text(&text);
                        self.notification_manager.info("Copied to clipboard");
                    }
                }
                self.conversation_viewer.clear_selection();
            }

            // Tab actions (TRC-005: Per-tab PTY isolation)
            Action::TabCreate => {
                let new_tab_id = self.tab_manager.create_tab_default();
                // Spawn PTY for the new tab
                if let Err(e) = self.spawn_pty_for_tab(new_tab_id) {
                    tracing::error!("Failed to spawn PTY for new tab {}: {}", new_tab_id, e);
                    // TRC-023: Notify on PTY spawn failure
                    self.notification_manager.error_with_message("Tab Error", format!("Failed to spawn shell: {}", e));
                } else {
                    // TRC-023: Notify tab creation
                    self.notification_manager.info(format!("Tab {} created", self.tab_manager.count()));
                }
            }
            Action::TabClose => {
                self.tab_manager.close_active_tab();
                // PTY cleanup is handled by TabManager::close_tab
            }
            Action::TabCloseIndex(idx) => {
                if let Some(tab) = self.tab_manager.tabs().get(idx) {
                    let id = tab.id();
                    self.tab_manager.close_tab(id);
                    // PTY cleanup is handled by TabManager::close_tab
                }
            }
            Action::TabNext => {
                self.tab_manager.next_tab();
                // Clear activity indicator when switching to a tab
                self.tab_manager.clear_active_activity();
            }
            Action::TabPrev => {
                self.tab_manager.prev_tab();
                self.tab_manager.clear_active_activity();
            }
            Action::TabSelect(idx) => {
                self.tab_manager.select(idx);
                self.tab_manager.clear_active_activity();
            }
            Action::TabRename(name) => {
                self.tab_manager.rename_active_tab(name);
            }
            Action::TabMove { from, to } => {
                self.tab_manager.move_tab(from, to);
            }
            
            // TRC-029: Inline tab rename actions
            Action::TabStartRename => {
                self.tab_manager.start_rename();
                self.input_mode = InputMode::Insert { target: crate::input::mode::InsertTarget::TabRename };
            }
            Action::TabCancelRename => {
                self.tab_manager.cancel_rename();
                self.input_mode = InputMode::Normal;
            }
            Action::TabRenameInput(c) => {
                self.tab_manager.rename_input(c);
            }
            Action::TabRenameBackspace => {
                self.tab_manager.rename_backspace();
            }
            
            // Key storage actions (TRC-011)
            Action::KeyStore(key_id, secret) => {
                if let Some(ref mut ks) = self.keystore {
                    let secret_str = SecretString::new(secret);
                    match ks.store(&key_id, &secret_str) {
                        Ok(()) => {
                            tracing::info!("Stored API key for {}", key_id);
                            // Re-register provider with new key
                            if let Ok(Some(s)) = ks.get(&key_id) {
                                match key_id {
                                    KeyId::Anthropic => self.agent_engine.llm_manager_mut().register_anthropic(s.expose()),
                                    KeyId::OpenAI => self.agent_engine.llm_manager_mut().register_openai(s.expose()),
                                    KeyId::Gemini => self.agent_engine.llm_manager_mut().register_gemini(s.expose()),
                                    KeyId::Grok => self.agent_engine.llm_manager_mut().register_grok(s.expose()),
                                    KeyId::Groq => self.agent_engine.llm_manager_mut().register_groq(s.expose()),
                                    KeyId::Custom(_) => {}
                                }
                            }
                        }
                        Err(e) => tracing::error!("Failed to store API key: {}", e),
                    }
                } else {
                    tracing::warn!("Keystore not initialized");
                }
            }
            Action::KeyGet(_key_id) => {
                // Key retrieval is handled internally by register_from_keystore
                // This action exists for programmatic access if needed
            }
            Action::KeyDelete(key_id) => {
                if let Some(ref mut ks) = self.keystore {
                    match ks.delete(&key_id) {
                        Ok(()) => tracing::info!("Deleted API key for {}", key_id),
                        Err(e) => tracing::error!("Failed to delete API key: {}", e),
                    }
                }
            }
            Action::KeyList => {
                if let Some(ref ks) = self.keystore {
                    match ks.list() {
                        Ok(keys) => {
                            let names: Vec<_> = keys.iter().map(|k| k.as_str()).collect();
                            tracing::info!("Stored API keys: {:?}", names);
                        }
                        Err(e) => tracing::error!("Failed to list API keys: {}", e),
                    }
                }
            }
            Action::KeyUnlock(password) => {
                if let Some(ref mut ks) = self.keystore {
                    match ks.unlock(&password) {
                        Ok(()) => {
                            tracing::info!("Keystore unlocked");
                            // Re-register providers after unlock
                            let registered = self.agent_engine.llm_manager_mut().register_from_keystore(ks);
                            if !registered.is_empty() {
                                tracing::info!("Loaded API keys for providers: {:?}", registered);
                            }
                        }
                        Err(e) => tracing::error!("Failed to unlock keystore: {}", e),
                    }
                }
            }
            Action::KeyInit(password) => {
                if let Some(ref mut ks) = self.keystore {
                    match ks.init_encrypted(&password) {
                        Ok(()) => tracing::info!("Keystore initialized with encryption"),
                        Err(e) => tracing::error!("Failed to initialize keystore: {}", e),
                    }
                }
            }
            
            // Session persistence actions (TRC-012)
            Action::SessionSave => {
                self.save_session();
            }
            Action::SessionLoad => {
                if let Err(e) = self.restore_session() {
                    tracing::error!("Failed to restore session: {}", e);
                }
            }
            Action::SessionClear => {
                if let Some(ref session_manager) = self.session_manager {
                    if let Err(e) = session_manager.clear() {
                        tracing::error!("Failed to clear session: {}", e);
                    }
                }
            }
            
            // Log viewer actions (TRC-013)
            Action::LogViewerShow => {
                self.show_log_viewer = true;
                self.focus.focus(FocusArea::LogViewer);
            }
            Action::LogViewerHide => {
                self.show_log_viewer = false;
                self.focus.focus(FocusArea::Menu);
            }
            Action::LogViewerToggle => {
                if self.show_log_viewer {
                    self.show_log_viewer = false;
                    self.focus.focus(FocusArea::Menu);
                } else {
                    self.show_log_viewer = true;
                    self.focus.focus(FocusArea::LogViewer);
                }
            }
            Action::LogViewerScrollUp(n) => {
                self.log_viewer.scroll_up(n);
            }
            Action::LogViewerScrollDown(n) => {
                self.log_viewer.scroll_down(n);
            }
            Action::LogViewerScrollToTop => {
                self.log_viewer.scroll_to_top();
            }
            Action::LogViewerScrollToBottom => {
                self.log_viewer.scroll_to_bottom();
            }
            Action::LogViewerScrollPageUp => {
                self.log_viewer.scroll_page_up();
            }
            Action::LogViewerScrollPageDown => {
                self.log_viewer.scroll_page_down();
            }
            Action::LogViewerToggleAutoScroll => {
                self.log_viewer.toggle_auto_scroll();
            }
            Action::LogViewerClear => {
                self.log_viewer.clear();
            }
            Action::LogViewerPush(target, message) => {
                self.log_viewer.push_info(target, message);
            }
            
            // Config panel actions (TRC-014)
            Action::ConfigPanelShow => {
                // Refresh config panel with current settings before showing
                let providers = self.agent_engine.registered_providers();
                self.config_panel.refresh(
                    self.config_manager.app_config(),
                    self.config_manager.keybindings(),
                    self.config_manager.theme(),
                    &providers,
                );
                self.show_config_panel = true;
                self.focus.focus(FocusArea::ConfigPanel);
            }
            Action::ConfigPanelHide => {
                self.show_config_panel = false;
                self.focus.focus(FocusArea::Menu);
            }
            Action::ConfigPanelToggle => {
                if self.show_config_panel {
                    self.show_config_panel = false;
                    self.focus.focus(FocusArea::Menu);
                } else {
                    let providers = self.agent_engine.registered_providers();
                    self.config_panel.refresh(
                        self.config_manager.app_config(),
                        self.config_manager.keybindings(),
                        self.config_manager.theme(),
                        &providers,
                    );
                    self.show_config_panel = true;
                    self.focus.focus(FocusArea::ConfigPanel);
                }
            }
            Action::ConfigPanelScrollUp(n) => {
                self.config_panel.scroll_up(n);
            }
            Action::ConfigPanelScrollDown(n) => {
                self.config_panel.scroll_down(n);
            }
            Action::ConfigPanelScrollToTop => {
                self.config_panel.scroll_to_top();
            }
            Action::ConfigPanelScrollToBottom => {
                self.config_panel.scroll_to_bottom();
            }
            Action::ConfigPanelScrollPageUp => {
                self.config_panel.scroll_page_up();
            }
            Action::ConfigPanelScrollPageDown => {
                self.config_panel.scroll_page_down();
            }
            Action::ConfigPanelNextSection => {
                self.config_panel.next_section();
            }
            Action::ConfigPanelPrevSection => {
                self.config_panel.prev_section();
            }
            Action::ConfigPanelToggleSection => {
                self.config_panel.toggle_section();
            }

            // Spinner actions (TRC-015)
            Action::SpinnerTick => {
                self.spinner_manager.tick();
            }
            Action::SpinnerStart(name, label) => {
                self.spinner_manager.start(SpinnerKey::custom(name), label);
            }
            Action::SpinnerStop(name) => {
                self.spinner_manager.stop(&SpinnerKey::custom(name));
            }
            Action::SpinnerSetLabel(name, label) => {
                self.spinner_manager.set_label(&SpinnerKey::custom(name), label);
            }
            
            // Tool Call UI actions (TRC-016)
            Action::ToolCallNextTool => {
                self.conversation_viewer.select_next_tool();
            }
            Action::ToolCallPrevTool => {
                self.conversation_viewer.select_prev_tool();
            }
            Action::ToolCallToggleExpand => {
                self.conversation_viewer.toggle_selected_tool();
            }
            Action::ToolCallExpandAll => {
                self.conversation_viewer.expand_all_tools();
            }
            Action::ToolCallCollapseAll => {
                self.conversation_viewer.collapse_all_tools();
            }
            Action::ToolCallStartExecution(tool_id) => {
                self.conversation_viewer.start_tool_execution(&tool_id);
            }
            Action::ToolCallRegister(tool_use) => {
                self.conversation_viewer.register_tool_use(tool_use);
            }
            
            // TRC-017: Thinking block toggle
            Action::ThinkingToggleCollapse => {
                self.conversation_viewer.toggle_thinking_collapse();
            }

            // Tool result collapse toggle
            Action::ToolResultToggleCollapse => {
                self.conversation_viewer.toggle_tool_results_collapse();
            }

            // Phase 4: Tool verbosity cycle
            Action::ToolVerbosityCycle => {
                self.conversation_viewer.cycle_tool_verbosity();
            }

            // TRC-020: Context menu actions
            Action::ContextMenuShow { x, y, target } => {
                let items = self.build_context_menu_items(&target);
                self.context_menu.show(x, y, target, items);
            }
            Action::ContextMenuClose => {
                self.context_menu.hide();
            }
            Action::ContextMenuNext => {
                // Navigation is handled internally by context_menu.handle_event()
            }
            Action::ContextMenuPrev => {
                // Navigation is handled internally by context_menu.handle_event()
            }
            Action::ContextMenuSelect => {
                // Selection is handled internally by context_menu.handle_event()
            }
            
            // TRC-023: Notification actions
            Action::NotifyInfo(title) => {
                self.notification_manager.info(title);
            }
            Action::NotifyInfoMessage(title, message) => {
                self.notification_manager.info_with_message(title, message);
            }
            Action::NotifySuccess(title) => {
                self.notification_manager.success(title);
            }
            Action::NotifySuccessMessage(title, message) => {
                self.notification_manager.success_with_message(title, message);
            }
            Action::NotifyWarning(title) => {
                self.notification_manager.warning(title);
            }
            Action::NotifyWarningMessage(title, message) => {
                self.notification_manager.warning_with_message(title, message);
            }
            Action::NotifyError(title) => {
                self.notification_manager.error(title);
            }
            Action::NotifyErrorMessage(title, message) => {
                self.notification_manager.error_with_message(title, message);
            }
            Action::NotifyDismiss => {
                self.notification_manager.dismiss_first();
            }
            Action::NotifyDismissAll => {
                self.notification_manager.dismiss_all();
            }
            
            // TRC-024: Pane resize actions
            Action::PaneResizeMainGrow => {
                self.pane_layout.resize_main(ResizeDirection::Grow);
            }
            Action::PaneResizeMainShrink => {
                self.pane_layout.resize_main(ResizeDirection::Shrink);
            }
            Action::PaneResizeRightGrow => {
                self.pane_layout.resize_right(ResizeDirection::Grow);
            }
            Action::PaneResizeRightShrink => {
                self.pane_layout.resize_right(ResizeDirection::Shrink);
            }
            Action::PaneResizeLeftGrow => {
                self.pane_layout.resize_left(ResizeDirection::Grow);
            }
            Action::PaneResizeLeftShrink => {
                self.pane_layout.resize_left(ResizeDirection::Shrink);
            }
            Action::PaneResetLayout => {
                self.pane_layout.reset_to_defaults();
            }
            Action::PaneStartDrag(border) => {
                let rb = match border {
                    PaneBorder::MainVertical => ResizableBorder::MainVertical,
                    PaneBorder::RightHorizontal => ResizableBorder::RightHorizontal,
                    PaneBorder::LeftHorizontal => ResizableBorder::LeftHorizontal,
                };
                self.drag_state.start(rb);
            }
            Action::PaneDrag { x, y } => {
                if let Some(border) = self.drag_state.border() {
                    self.pane_layout.handle_mouse_drag(x, y, self.content_area, border, self.show_conversation);
                }
            }
            Action::PaneEndDrag => {
                self.drag_state.stop();
            }
            
            // Settings Editor actions (TS-012)
            Action::SettingsShow => {
                self.open_settings_editor();
            }
            Action::SettingsClose => {
                self.close_settings_editor();
            }
            Action::SettingsToggle => {
                if self.show_settings_editor {
                    self.close_settings_editor();
                } else {
                    self.open_settings_editor();
                }
            }
            Action::SettingsNextSection => {
                self.settings_editor.update(&action);
            }
            Action::SettingsPrevSection => {
                self.settings_editor.update(&action);
            }
            Action::SettingsNextItem => {
                self.settings_editor.update(&action);
            }
            Action::SettingsPrevItem => {
                self.settings_editor.update(&action);
            }
            Action::SettingsScrollUp(_) => {
                self.settings_editor.update(&action);
            }
            Action::SettingsScrollDown(_) => {
                self.settings_editor.update(&action);
            }
            Action::SettingsStartEdit => {
                // Handled by settings_editor internally via handle_event
            }
            Action::SettingsCancelEdit => {
                // Handled by settings_editor internally via handle_event
            }
            Action::SettingsKeyEntered { ref provider, ref key } => {
                // Store the key in keystore and update SettingsEditor
                self.handle_settings_key_entered(provider.clone(), key.clone());
            }
            Action::SettingsProviderChanged(ref provider) => {
                // Update AgentEngine with new provider
                self.agent_engine.set_provider(provider);
                // Update config_manager so it persists on save
                self.config_manager.llm_config_mut().defaults.provider = provider.clone();
                // Refresh models list for the new provider
                let models = self.model_catalog.models_for_provider(provider);
                self.settings_editor.set_available_models(models.iter().map(|m| m.to_string()).collect());
            }
            Action::SettingsModelChanged(ref model) => {
                // Update AgentEngine with new model
                self.agent_engine.set_model(model);
                // Update config_manager so it persists on save
                self.config_manager.llm_config_mut().defaults.model = model.clone();
            }
            Action::SettingsTestKey => {
                self.handle_settings_test_key();
            }
            Action::SettingsTestKeyResult { ref provider, success, ref error } => {
                self.settings_editor.set_key_test_result(provider, success, error.clone());
            }
            Action::SettingsTemperatureChanged(temp) => {
                // Update config with new temperature
                self.config_manager.llm_config_mut().parameters.temperature = temp;
            }
            Action::SettingsMaxTokensChanged(tokens) => {
                // Update config with new max tokens  
                self.config_manager.llm_config_mut().parameters.max_tokens = tokens;
            }
            Action::SettingsSave => {
                self.handle_settings_save();
            }

            // T2.4: Ask user dialog actions
            Action::AskUserShow(ref request) => {
                // Convert AskUserRequest to ParsedQuestions
                let questions: Vec<crate::llm::ParsedQuestion> = request.questions.iter().map(|q| {
                    crate::llm::ParsedQuestion {
                        header: q.header.clone(),
                        question: q.question.clone(),
                        options: q.options.iter().map(|o| crate::llm::ParsedOption {
                            label: o.label.clone(),
                            description: o.description.clone(),
                        }).collect(),
                        multi_select: q.multi_select,
                    }
                }).collect();
                self.ask_user_dialog.show(request.tool_use_id.clone(), questions);
            }
            Action::AskUserCancel => {
                // User cancelled - create error result
                if self.ask_user_dialog.is_visible() {
                    // Note: We don't have the tool_use_id here, so the dialog handles sending cancel
                    self.ask_user_dialog.hide();
                }
            }
            Action::AskUserRespond(ref response) => {
                // User responded - create tool result with answers
                self.ask_user_dialog.hide();
                let answers_json = serde_json::json!({
                    "answers": response.answers
                });
                let tool_result = crate::llm::ToolResult {
                    tool_use_id: response.tool_use_id.clone(),
                    content: crate::llm::ToolResultContent::Text(answers_json.to_string()),
                    is_error: false,
                };
                // Remove from pending and add to collected
                self.pending_tools.remove(&response.tool_use_id);
                self.collected_results.insert(response.tool_use_id.clone(), tool_result.clone());
                // Check if we have all results
                if self.collected_results.len() >= self.expected_tool_count && self.expected_tool_count > 0 {
                    let results: Vec<crate::llm::ToolResult> = self.collected_results.drain().map(|(_, r)| r).collect();
                    self.agent_engine.continue_after_tools(results);
                    self.expected_tool_count = 0;
                }
            }
            // Other ask_user actions are handled by the dialog's handle_event
            Action::AskUserNextOption
            | Action::AskUserPrevOption
            | Action::AskUserNextQuestion
            | Action::AskUserPrevQuestion
            | Action::AskUserToggleOption
            | Action::AskUserSelectOption
            | Action::AskUserStartCustom
            | Action::AskUserCancelCustom
            | Action::AskUserCustomInput(_)
            | Action::AskUserCustomBackspace
            | Action::AskUserSubmitCustom
            | Action::AskUserSubmit => {
                // These are handled by the dialog's handle_event
            }

            _ => {}
        }
        Ok(())
    }

    pub fn llm_response_buffer(&self) -> &str {
        &self.llm_response_buffer
    }
    
    /// Get the current streaming thinking buffer (TRC-017)
    pub fn thinking_buffer(&self) -> &str {
        &self.thinking_buffer
    }
    
    /// Toggle thinking block collapse state (TRC-017)
    pub fn toggle_thinking_collapse(&mut self) {
        self.collapse_thinking = !self.collapse_thinking;
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
        self.menu.set_stream_count(new_count);
        
        // Reset selected_stream_index if out of bounds or no streams
        if new_count == 0 {
            self.selected_stream_index = None;
        } else if self.selected_stream_index.map_or(true, |idx| idx >= new_count) {
            self.selected_stream_index = Some(0);
        }
        
        // Notify user of the reload
        if old_count != new_count {
            self.notification_manager.info(format!(
                "Streams reloaded: {} → {} configured",
                old_count, new_count
            ));
        } else {
            self.notification_manager.info(format!(
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
                let tab_count = self.tab_manager.count();
                
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
                let has_selection = self.tab_manager
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
                let has_selection = self.chat_input.has_selection();
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
        let provider = self.agent_engine.current_provider();
        let provider = if provider.is_empty() { "anthropic" } else { provider };
        let models = self.model_catalog.models_for_provider(provider);
        self.settings_editor.set_available_models(models.iter().map(|m| m.to_string()).collect());
        
        self.show_settings_editor = true;
        self.focus.focus(FocusArea::SettingsEditor);
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
        self.focus.focus(FocusArea::Menu);
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
                        "anthropic" => self.agent_engine.llm_manager_mut().register_anthropic(key),
                        "openai" => self.agent_engine.llm_manager_mut().register_openai(key),
                        "gemini" => self.agent_engine.llm_manager_mut().register_gemini(key),
                        "grok" => self.agent_engine.llm_manager_mut().register_grok(key),
                        "groq" => self.agent_engine.llm_manager_mut().register_groq(key),
                        _ => {}
                    }

                    self.notification_manager.success(format!("{} API key saved", provider));
                }
                Err(e) => {
                    self.notification_manager.error_with_message(
                        "Failed to save key",
                        e.to_string(),
                    );
                }
            }
        } else {
            self.notification_manager.error("Keystore not available");
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
            self.notification_manager.success(format!("{} key verified", provider));
        }
    }
    
    /// Handle settings save request
    fn handle_settings_save(&mut self) {
        let config = self.settings_editor.config().clone();

        // Update AgentEngine with new settings
        self.agent_engine.set_provider(&config.defaults.provider);
        self.agent_engine.set_model(&config.defaults.model);

        // Update config manager with new settings
        *self.config_manager.llm_config_mut() = config;
        
        // Save to config file
        if let Err(e) = self.config_manager.save_llm_config() {
            self.notification_manager.error_with_message(
                "Failed to save LLM config",
                e.to_string(),
            );
        } else {
            self.notification_manager.success("LLM settings saved");
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, DisableBracketedPaste);
    }
}

fn key_to_bytes(key: KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char(c) => {
            if ctrl && c.is_ascii_alphabetic() {
                vec![(c.to_ascii_lowercase() as u8) - b'a' + 1]
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        KeyCode::F(n) => match n {
            1 => vec![0x1b, b'O', b'P'],
            2 => vec![0x1b, b'O', b'Q'],
            3 => vec![0x1b, b'O', b'R'],
            4 => vec![0x1b, b'O', b'S'],
            5 => vec![0x1b, b'[', b'1', b'5', b'~'],
            6 => vec![0x1b, b'[', b'1', b'7', b'~'],
            7 => vec![0x1b, b'[', b'1', b'8', b'~'],
            8 => vec![0x1b, b'[', b'1', b'9', b'~'],
            9 => vec![0x1b, b'[', b'2', b'0', b'~'],
            10 => vec![0x1b, b'[', b'2', b'1', b'~'],
            11 => vec![0x1b, b'[', b'2', b'3', b'~'],
            12 => vec![0x1b, b'[', b'2', b'4', b'~'],
            _ => vec![],
        },
        _ => vec![],
    }
}
