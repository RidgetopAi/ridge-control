use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};
use tokio::sync::mpsc;

use crate::action::Action;
use crate::components::command_palette::CommandPalette;
use crate::components::config_panel::ConfigPanel;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::conversation_viewer::ConversationViewer;
use crate::components::log_viewer::LogViewer;
use crate::components::menu::Menu;
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
    LLMManager, LLMEvent, StreamChunk, StreamDelta, StopReason,
    ToolExecutor, ToolExecutionCheck, PendingToolUse, ToolUse,
};
use crate::streams::{StreamEvent, StreamManager, StreamsConfig, ConnectionState};
use crate::tabs::{TabId, TabManager, TabBar};

const TICK_INTERVAL_MS: u64 = 500;

pub struct App {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    should_quit: bool,
    input_mode: InputMode,
    focus: FocusManager,
    process_monitor: ProcessMonitor,
    menu: Menu,
    stream_manager: StreamManager,
    llm_manager: LLMManager,
    llm_response_buffer: String,
    clipboard: Option<Clipboard>,
    last_tick: Instant,
    // Tool execution
    tool_executor: ToolExecutor,
    confirm_dialog: ConfirmDialog,
    pending_tool: Option<PendingToolUse>,
    // Command palette
    command_palette: CommandPalette,
    // Tracking tool use during streaming
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
    current_tool_input: String,
    // Tool execution result receiver
    tool_result_rx: Option<mpsc::UnboundedReceiver<std::result::Result<crate::llm::ToolResult, crate::llm::ToolError>>>,
    // Configuration system
    config_manager: ConfigManager,
    config_watcher: Option<ConfigWatcherMode>,
    // Tab system with per-tab PTY sessions (TRC-005)
    tab_manager: TabManager,
    // PTY event receivers from all tabs, keyed by TabId
    pty_receivers: Vec<mpsc::UnboundedReceiver<(TabId, PtyEvent)>>,
    // LLM conversation display
    conversation_viewer: ConversationViewer,
    show_conversation: bool,
    // Stream viewer
    stream_viewer: StreamViewer,
    show_stream_viewer: bool,
    selected_stream_index: Option<usize>,
    // Layout areas for mouse hit-testing (TRC-010)
    tab_bar_area: Rect,
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
    // Spinner manager for animations (TRC-015)
    spinner_manager: SpinnerManager,
}

impl App {
    pub fn new() -> Result<Self> {
        enable_raw_mode().map_err(|e| RidgeError::Terminal(e.to_string()))?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
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

        let mut llm_manager = LLMManager::new();
        
        // Initialize secure key storage (TRC-011)
        let keystore = match KeyStore::new() {
            Ok(ks) => {
                // Try to register providers from keystore
                let registered = llm_manager.register_from_keystore(&ks);
                if !registered.is_empty() {
                    tracing::info!("Loaded API keys for providers: {:?}", registered);
                }
                Some(ks)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize keystore: {}", e);
                None
            }
        };
        
        // Get working directory for tool executor
        let working_dir = std::env::current_dir().unwrap_or_else(|_| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
        });
        let tool_executor = ToolExecutor::new(working_dir);
        
        // Initialize configuration system
        let config_manager = ConfigManager::new()?;
        
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

        // Initialize session manager (TRC-012)
        let session_manager = match SessionManager::new() {
            Ok(sm) => Some(sm),
            Err(e) => {
                tracing::warn!("Failed to initialize session manager: {}", e);
                None
            }
        };

        Ok(Self {
            terminal,
            should_quit: false,
            input_mode: InputMode::Normal,
            focus: FocusManager::new(),
            process_monitor: ProcessMonitor::new(),
            menu,
            stream_manager,
            llm_manager,
            llm_response_buffer: String::new(),
            clipboard,
            last_tick: Instant::now(),
            tool_executor,
            confirm_dialog: ConfirmDialog::new(),
            pending_tool: None,
            command_palette: CommandPalette::new(),
            current_tool_id: None,
            current_tool_name: None,
            current_tool_input: String::new(),
            tool_result_rx: None,
            config_manager,
            config_watcher,
            tab_manager,
            pty_receivers: Vec::new(),
            conversation_viewer: ConversationViewer::new(),
            show_conversation: false,
            stream_viewer: StreamViewer::new(),
            show_stream_viewer: false,
            selected_stream_index: initial_stream_index,
            tab_bar_area: Rect::default(),
            keystore,
            session_manager,
            log_viewer: LogViewer::new(),
            show_log_viewer: false,
            config_panel: ConfigPanel::new(),
            show_config_panel: false,
            spinner_manager: SpinnerManager::new(),
        })
    }

    pub fn configure_llm(&mut self, api_key: impl Into<String>) {
        self.llm_manager.register_anthropic(api_key);
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
                PtyEvent::Exited(_code) => {
                    self.tab_manager.mark_pty_dead(tab_id);
                    // If main tab (id 0) dies, quit the app
                    if tab_id == 0 {
                        self.should_quit = true;
                    }
                }
                PtyEvent::Error(_) => {
                    self.tab_manager.mark_pty_dead(tab_id);
                    if tab_id == 0 {
                        self.should_quit = true;
                    }
                }
            }
        }
    }

    pub fn run(&mut self) -> Result<()> {
        let mut stream_rx = self.stream_manager.take_event_rx();
        let mut llm_rx = self.llm_manager.take_event_rx();
        
        loop {
            self.draw()?;

            // Poll PTY events from all tabs (TRC-005)
            self.poll_pty_events();

            if let Some(ref mut rx) = stream_rx {
                while let Ok(stream_event) = rx.try_recv() {
                    self.handle_stream_event(stream_event);
                }
            }

            if let Some(ref mut rx) = llm_rx {
                while let Ok(llm_event) = rx.try_recv() {
                    self.handle_llm_event(llm_event);
                }
            }
            
            // Poll tool execution results - collect first, then dispatch to avoid borrow issues
            let tool_results: Vec<_> = if let Some(ref mut rx) = self.tool_result_rx {
                let mut results = Vec::new();
                while let Ok(result) = rx.try_recv() {
                    results.push(result);
                }
                results
            } else {
                Vec::new()
            };
            
            for result in tool_results {
                match result {
                    Ok(tool_result) => {
                        self.dispatch(Action::ToolResult(tool_result))?;
                    }
                    Err(e) => {
                        // Create an error result
                        if let Some(pending) = &self.pending_tool {
                            let error_result = crate::llm::ToolResult {
                                tool_use_id: pending.tool.id.clone(),
                                content: crate::llm::ToolResultContent::Text(e.to_string()),
                                is_error: true,
                            };
                            self.dispatch(Action::ToolResult(error_result))?;
                        }
                    }
                }
            }

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
                    self.dispatch(action)?;
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
                    StreamChunk::Delta(delta) => {
                        match delta {
                            StreamDelta::Text(text) => {
                                self.llm_response_buffer.push_str(&text);
                            }
                            StreamDelta::Thinking(text) => {
                                self.llm_response_buffer.push_str(&text);
                            }
                            StreamDelta::ToolInput { id, name, input_json } => {
                                // Track tool use being built up
                                if self.current_tool_id.is_none() || self.current_tool_id.as_ref() != Some(&id) {
                                    self.current_tool_id = Some(id);
                                    self.current_tool_name = name;
                                    self.current_tool_input.clear();
                                }
                                self.current_tool_input.push_str(&input_json);
                            }
                        }
                    }
                    StreamChunk::BlockStop { .. } => {
                        // When a tool use block stops, we have the complete tool use
                        if let (Some(id), Some(name)) = (self.current_tool_id.take(), self.current_tool_name.take()) {
                            let input: serde_json::Value = serde_json::from_str(&self.current_tool_input)
                                .unwrap_or(serde_json::Value::Null);
                            self.current_tool_input.clear();
                            
                            let tool_use = ToolUse { id, name, input };
                            self.handle_tool_use_request(tool_use);
                        }
                    }
                    StreamChunk::Stop { reason, .. } => {
                        // If stop reason is ToolUse, the tool was already handled in BlockStop
                        if reason != StopReason::ToolUse {
                            if !self.llm_response_buffer.is_empty() {
                                self.llm_manager.add_assistant_message(self.llm_response_buffer.clone());
                                self.llm_response_buffer.clear();
                            }
                        }
                    }
                    _ => {}
                }
            }
            LLMEvent::Complete => {
                if !self.llm_response_buffer.is_empty() {
                    self.llm_manager.add_assistant_message(self.llm_response_buffer.clone());
                    self.llm_response_buffer.clear();
                }
            }
            LLMEvent::Error(_err) => {
                self.llm_response_buffer.clear();
                self.current_tool_id = None;
                self.current_tool_name = None;
                self.current_tool_input.clear();
            }
            LLMEvent::ToolUseDetected(tool_use) => {
                self.handle_tool_use_request(tool_use);
            }
        }
    }
    
    fn handle_tool_use_request(&mut self, tool_use: ToolUse) {
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
                self.pending_tool = Some(pending.clone());
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
                self.pending_tool = Some(pending.clone());
                self.confirm_dialog.show(pending);
                self.input_mode = InputMode::Confirm {
                    title: "Tool Blocked".to_string(),
                    message: "Tool cannot execute".to_string(),
                };
            }
        }
    }
    
    fn execute_tool(&mut self, pending: PendingToolUse) {
        // Add the tool use to the conversation
        self.llm_manager.add_tool_use(pending.tool.clone());
        
        // Execute the tool asynchronously
        let tool = pending.tool.clone();
        let working_dir = std::env::current_dir().unwrap_or_else(|_| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
        });
        
        // We need to create a new executor for the async task
        let dangerous_mode = self.tool_executor.registry().is_dangerous_mode();
        
        // Spawn the tool execution
        let (result_tx, result_rx) = mpsc::unbounded_channel();
        self.tool_result_rx = Some(result_rx);
        
        tokio::spawn(async move {
            let mut executor = ToolExecutor::new(working_dir);
            executor.set_dangerous_mode(dangerous_mode);
            
            let result = executor.execute(&tool).await;
            let _ = result_tx.send(result);
        });
        
        // Store the pending tool for reference
        self.pending_tool = Some(pending);
    }

    fn handle_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Connected(id) => {
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(ConnectionState::Connected);
                }
            }
            StreamEvent::Disconnected(id, _reason) => {
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(ConnectionState::Disconnected);
                }
            }
            StreamEvent::Data(id, data) => {
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.push_data(data);
                }
            }
            StreamEvent::Error(id, _msg) => {
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(ConnectionState::Failed);
                }
            }
            StreamEvent::StateChanged(id, state) => {
                if let Some(client) = self.stream_manager.get_client_mut(&id) {
                    client.set_state(state);
                }
            }
        }
    }

    fn draw(&mut self) -> Result<()> {
        let focus = self.focus.clone();
        let streams: Vec<_> = self.stream_manager.clients().to_vec();
        let show_confirm = self.confirm_dialog.is_visible();
        let show_palette = self.command_palette.is_visible();
        let show_tabs = self.tab_manager.count() > 1; // Only show tab bar with multiple tabs
        let show_conversation = self.show_conversation || !self.llm_response_buffer.is_empty();
        let show_stream_viewer = self.show_stream_viewer;
        let show_log_viewer = self.show_log_viewer;
        let show_config_panel = self.show_config_panel;
        let selected_stream_idx = self.selected_stream_index;
        let theme = self.config_manager.theme().clone();
        let messages = self.llm_manager.conversation().to_vec();
        let streaming_buffer = self.llm_response_buffer.clone();
        
        // Get active tab's PTY session for rendering (TRC-005)
        let active_tab_id = self.tab_manager.active_tab().id();
        
        // Pre-calculate tab bar area for mouse hit-testing (TRC-010)
        let term_size = self.terminal.size().unwrap_or_default();
        let term_rect = Rect::new(0, 0, term_size.width, term_size.height);
        let computed_tab_bar_area = if show_tabs {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(term_rect);
            chunks[0]
        } else {
            Rect::default()
        };
        self.tab_bar_area = computed_tab_bar_area;

        self.terminal
            .draw(|frame| {
                let size = frame.area();

                // Split: optional tab bar at top, then main content
                let (tab_bar_area, content_area) = if show_tabs {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(0)])
                        .split(size);
                    (chunks[0], chunks[1])
                } else {
                    // No tab bar - use full area
                    (Rect::default(), size)
                };

                // Render tab bar if visible
                if show_tabs {
                    let tab_bar = TabBar::from_manager_themed(&self.tab_manager, &theme);
                    frame.render_widget(tab_bar, tab_bar_area);
                }

                // Main layout: left (terminal or terminal+conversation) and right (process monitor + menu)
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(67), Constraint::Percentage(33)])
                    .split(content_area);

                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(main_chunks[1]);

                // Left area: split between terminal and conversation if conversation is visible
                // Use active tab's terminal widget (TRC-005)
                if show_conversation {
                    let left_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                        .split(main_chunks[0]);

                    if let Some(session) = self.tab_manager.get_pty_session(active_tab_id) {
                        session.terminal().render(
                            frame,
                            left_chunks[0],
                            focus.is_focused(FocusArea::Terminal),
                            &theme,
                        );
                    }

                    self.conversation_viewer.render_conversation(
                        frame,
                        left_chunks[1],
                        focus.is_focused(FocusArea::StreamViewer), // Reuse StreamViewer focus for now
                        &messages,
                        &streaming_buffer,
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
                        block.inner(left_chunks[1])
                    };
                    self.conversation_viewer.set_inner_area(conv_inner);
                } else {
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
                
                // Stream viewer overlay - takes right half of screen when visible
                if show_stream_viewer {
                    let stream_area = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(size)[1];
                    
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
                
                // Log viewer overlay (TRC-013) - takes right half of screen when visible
                if show_log_viewer {
                    let log_area = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(size)[1];
                    
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
                
                // Config panel overlay (TRC-014) - takes right half of screen when visible
                if show_config_panel {
                    let config_area = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(size)[1];
                    
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
                
                if show_confirm {
                    self.confirm_dialog.render(frame, size, &theme);
                }
                
                if show_palette {
                    self.command_palette.render(frame, size, &theme);
                }
            })
            .map_err(|e| RidgeError::Terminal(e.to_string()))?;

        Ok(())
    }

    fn handle_event(&mut self, event: CrosstermEvent) -> Option<Action> {
        match event {
            CrosstermEvent::Key(key) => self.handle_key(key),
            CrosstermEvent::Mouse(mouse) => self.handle_mouse(mouse),
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

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Debug: Log key events to help diagnose input issues
        #[cfg(debug_assertions)]
        tracing::debug!("Key event: {:?}, mode: {:?}, focus: {:?}", key, self.input_mode, self.focus.current());
        
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
                // First check configurable keybindings for global actions
                let binding_action = self.config_manager.keybindings().get_action(&self.input_mode, &key);
                #[cfg(debug_assertions)]
                tracing::debug!("Normal mode keybinding lookup result: {:?}", binding_action);
                if let Some(action) = binding_action {
                    return Some(action);
                }
                
                // Alt+1 through Alt+9 for direct tab selection (hardcoded for convenience)
                if let KeyCode::Char(c @ '1'..='9') = key.code {
                    if key.modifiers.contains(KeyModifiers::ALT) {
                        let idx = (c as usize) - ('1' as usize);
                        return Some(Action::TabSelect(idx));
                    }
                }

                // Focus-specific key handling
                match self.focus.current() {
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
                        // Handle StreamViewer key events
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
                }
            }
            InputMode::Insert { .. } => {
                if let Some(action) = self.config_manager.keybindings().get_action(&self.input_mode, &key) {
                    return Some(action);
                }
                None
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
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
            FocusArea::StreamViewer => None,
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
                self.command_palette.show();
                self.input_mode = InputMode::CommandPalette;
            }
            Action::CloseCommandPalette => {
                self.command_palette.hide();
                self.input_mode = InputMode::Normal;
            }
            Action::FocusNext => {
                self.focus.next();
            }
            Action::FocusPrev => {
                self.focus.prev();
            }
            Action::FocusArea(area) => {
                self.focus.focus(area);
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
                // Paste to active tab's PTY (TRC-005)
                if let Some(ref mut clipboard) = self.clipboard {
                    if let Ok(text) = clipboard.get_text() {
                        self.tab_manager.write_to_active_pty(text.into_bytes());
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
                let config = StreamsConfig::load();
                self.stream_manager.load_streams(&config);
                let count = self.stream_manager.clients().len();
                self.menu.set_stream_count(count);
                // Reset selected_stream_index if out of bounds or no streams
                if count == 0 {
                    self.selected_stream_index = None;
                } else if self.selected_stream_index.map_or(true, |idx| idx >= count) {
                    self.selected_stream_index = Some(0);
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
            }
            Action::LlmSendMessage(msg) => {
                self.llm_manager.send_message(msg, None);
            }
            Action::LlmCancel => {
                self.llm_manager.cancel();
            }
            Action::LlmSelectModel(model) => {
                self.llm_manager.set_model(&model);
            }
            Action::LlmSelectProvider(provider) => {
                self.llm_manager.set_provider(&provider);
            }
            Action::LlmClearConversation => {
                self.llm_manager.clear_conversation();
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
                
                if let Some(pending) = self.pending_tool.take() {
                    // Update check to Allowed since user confirmed
                    let confirmed_pending = PendingToolUse::new(
                        pending.tool,
                        ToolExecutionCheck::Allowed
                    );
                    self.execute_tool(confirmed_pending);
                }
            }
            Action::ToolReject => {
                // User rejected tool execution
                self.confirm_dialog.dismiss();
                self.input_mode = InputMode::Normal;
                
                if let Some(pending) = self.pending_tool.take() {
                    // Send an error result back to the LLM
                    let error_result = crate::llm::ToolResult {
                        tool_use_id: pending.tool.id.clone(),
                        content: crate::llm::ToolResultContent::Text(
                            "User rejected tool execution".to_string()
                        ),
                        is_error: true,
                    };
                    self.llm_manager.add_tool_use(pending.tool);
                    self.llm_manager.add_tool_result(error_result);
                    // Continue conversation with the rejection
                    self.llm_manager.continue_after_tool(None);
                }
            }
            Action::ToolResult(result) => {
                // Tool execution completed, send result back to LLM
                self.llm_manager.add_tool_result(result);
                self.pending_tool = None;
                // Continue the conversation
                self.llm_manager.continue_after_tool(None);
            }
            Action::ToolToggleDangerousMode => {
                let current = self.tool_executor.registry().is_dangerous_mode();
                self.tool_executor.set_dangerous_mode(!current);
            }
            
            // Config actions
            Action::ConfigChanged(path) => {
                tracing::info!("Config file changed: {}", path.display());
                self.config_manager.reload_file(&path);
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

            // Tab actions (TRC-005: Per-tab PTY isolation)
            Action::TabCreate => {
                let new_tab_id = self.tab_manager.create_tab_default();
                // Spawn PTY for the new tab
                if let Err(e) = self.spawn_pty_for_tab(new_tab_id) {
                    tracing::error!("Failed to spawn PTY for new tab {}: {}", new_tab_id, e);
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
            
            // Key storage actions (TRC-011)
            Action::KeyStore(key_id, secret) => {
                if let Some(ref mut ks) = self.keystore {
                    let secret_str = SecretString::new(secret);
                    match ks.store(&key_id, &secret_str) {
                        Ok(()) => {
                            tracing::info!("Stored API key for {}", key_id);
                            // Re-register provider with new key
                            match ks.get(&key_id) {
                                Ok(Some(s)) => {
                                    match key_id {
                                        KeyId::Anthropic => self.llm_manager.register_anthropic(s.expose()),
                                        KeyId::OpenAI => self.llm_manager.register_openai(s.expose()),
                                        KeyId::Gemini => self.llm_manager.register_gemini(s.expose()),
                                        KeyId::Grok => self.llm_manager.register_grok(s.expose()),
                                        KeyId::Groq => self.llm_manager.register_groq(s.expose()),
                                        KeyId::Custom(_) => {}
                                    }
                                }
                                _ => {}
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
                            let registered = self.llm_manager.register_from_keystore(ks);
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
                let providers = self.llm_manager.registered_providers();
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
                    let providers = self.llm_manager.registered_providers();
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
            
            _ => {}
        }
        Ok(())
    }

    pub fn llm_manager(&self) -> &LLMManager {
        &self.llm_manager
    }

    pub fn llm_response_buffer(&self) -> &str {
        &self.llm_response_buffer
    }
    
    pub fn config(&self) -> &ConfigManager {
        &self.config_manager
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
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
