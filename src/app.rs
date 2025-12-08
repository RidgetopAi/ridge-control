use std::io::{self, Stdout};
use std::sync::mpsc as std_mpsc;
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
use crate::components::menu::Menu;
use crate::components::process_monitor::ProcessMonitor;
use crate::components::terminal::TerminalWidget;
use crate::components::Component;
use crate::error::{Result, RidgeError};
use crate::event::PtyEvent;
use crate::input::focus::{FocusArea, FocusManager};
use crate::input::mode::InputMode;
use crate::llm::{LLMManager, LLMEvent, StreamChunk, StreamDelta};
use crate::pty::PtyHandle;
use crate::streams::{StreamEvent, StreamManager, StreamsConfig, ConnectionState};

const TICK_INTERVAL_MS: u64 = 500;

pub struct App {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    should_quit: bool,
    input_mode: InputMode,
    focus: FocusManager,
    terminal_widget: TerminalWidget,
    process_monitor: ProcessMonitor,
    menu: Menu,
    stream_manager: StreamManager,
    llm_manager: LLMManager,
    llm_response_buffer: String,
    pty_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    pty_resize_tx: Option<std_mpsc::Sender<(u16, u16)>>,
    clipboard: Option<Clipboard>,
    last_tick: Instant,
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
        menu.set_stream_count(stream_manager.clients().len());

        let llm_manager = LLMManager::new();

        Ok(Self {
            terminal,
            should_quit: false,
            input_mode: InputMode::Normal,
            focus: FocusManager::new(),
            terminal_widget: TerminalWidget::new(term_cols, term_rows),
            process_monitor: ProcessMonitor::new(),
            menu,
            stream_manager,
            llm_manager,
            llm_response_buffer: String::new(),
            pty_tx: None,
            pty_resize_tx: None,
            clipboard,
            last_tick: Instant::now(),
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

    pub fn spawn_pty(&mut self) -> Result<mpsc::UnboundedReceiver<PtyEvent>> {
        let pty = PtyHandle::spawn()?;

        let size = self.terminal.size().map_err(|e| RidgeError::Terminal(e.to_string()))?;
        let area = Rect::new(0, 0, size.width, size.height);
        let (cols, rows) = Self::calculate_terminal_size(area);
        pty.resize(cols as u16, rows as u16)?;

        let (tx, rx) = mpsc::unbounded_channel();

        let (write_tx, write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        self.pty_tx = Some(write_tx);

        let (resize_tx, resize_rx) = std_mpsc::channel::<(u16, u16)>();
        self.pty_resize_tx = Some(resize_tx);

        std::thread::spawn(move || {
            let mut pty = pty;
            let mut buf = [0u8; 4096];
            let mut write_rx = write_rx;

            loop {
                if let Some(code) = pty.try_wait() {
                    let _ = tx.send(PtyEvent::Exited(code));
                    break;
                }

                while let Ok((cols, rows)) = resize_rx.try_recv() {
                    let _ = pty.resize(cols, rows);
                }

                while let Ok(data) = write_rx.try_recv() {
                    let _ = pty.write(&data);
                }

                match pty.try_read(&mut buf) {
                    Ok(0) => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Ok(n) => {
                        let _ = tx.send(PtyEvent::Output(buf[..n].to_vec()));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        let _ = tx.send(PtyEvent::Error(e));
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    pub fn run(&mut self, mut pty_rx: mpsc::UnboundedReceiver<PtyEvent>) -> Result<()> {
        let mut stream_rx = self.stream_manager.take_event_rx();
        let mut llm_rx = self.llm_manager.take_event_rx();
        
        loop {
            self.draw()?;

            while let Ok(pty_event) = pty_rx.try_recv() {
                match pty_event {
                    PtyEvent::Output(data) => {
                        self.terminal_widget.update(&Action::PtyOutput(data));
                    }
                    PtyEvent::Exited(_code) => {
                        self.should_quit = true;
                    }
                    PtyEvent::Error(_) => {
                        self.should_quit = true;
                    }
                }
            }

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

            if self.last_tick.elapsed() >= Duration::from_millis(TICK_INTERVAL_MS) {
                self.dispatch(Action::Tick)?;
                self.last_tick = Instant::now();
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
                            _ => {}
                        }
                    }
                    StreamChunk::Stop { .. } => {
                        if !self.llm_response_buffer.is_empty() {
                            self.llm_manager.add_assistant_message(self.llm_response_buffer.clone());
                            self.llm_response_buffer.clear();
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
            }
        }
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

        self.terminal
            .draw(|frame| {
                let size = frame.area();

                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(67), Constraint::Percentage(33)])
                    .split(size);

                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(main_chunks[1]);

                self.terminal_widget.render(
                    frame,
                    main_chunks[0],
                    focus.is_focused(FocusArea::Terminal),
                );
                self.process_monitor.render(
                    frame,
                    right_chunks[0],
                    focus.is_focused(FocusArea::ProcessMonitor),
                );
                self.menu.render_with_streams(
                    frame,
                    right_chunks[1],
                    focus.is_focused(FocusArea::Menu),
                    &streams,
                );

                let term_inner = {
                    let block = ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL);
                    block.inner(main_chunks[0])
                };
                self.terminal_widget.set_inner_area(term_inner);

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
        match self.input_mode {
            InputMode::PtyRaw => {
                if key.code == KeyCode::Esc && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Some(Action::EnterNormalMode);
                }

                if key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Some(Action::Paste);
                }

                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.terminal_widget.has_selection()
                {
                    return Some(Action::Copy);
                }

                let bytes = key_to_bytes(key);
                if !bytes.is_empty() {
                    return Some(Action::PtyInput(bytes));
                }
                None
            }
            InputMode::Normal => {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Some(Action::Quit);
                    }
                    KeyCode::Char('q') => return Some(Action::Quit),
                    KeyCode::Tab => return Some(Action::FocusNext),
                    KeyCode::BackTab => return Some(Action::FocusPrev),
                    _ => {}
                }

                match self.focus.current() {
                    FocusArea::Terminal => match key.code {
                        KeyCode::Enter => Some(Action::EnterPtyMode),
                        KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp(1)),
                        KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown(1)),
                        KeyCode::PageUp => Some(Action::ScrollPageUp),
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::ScrollPageUp)
                        }
                        KeyCode::PageDown => Some(Action::ScrollPageDown),
                        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::ScrollPageDown)
                        }
                        KeyCode::Char('g') => Some(Action::ScrollToTop),
                        KeyCode::Char('G') => Some(Action::ScrollToBottom),
                        KeyCode::Char('y') => Some(Action::Copy),
                        KeyCode::Char('p') => Some(Action::Paste),
                        _ => None,
                    },
                    FocusArea::ProcessMonitor => {
                        self.process_monitor.handle_event(&CrosstermEvent::Key(key))
                    }
                    FocusArea::Menu => {
                        self.menu.handle_event(&CrosstermEvent::Key(key))
                    }
                }
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        match self.focus.current() {
            FocusArea::Terminal => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left) => {
                    self.terminal_widget
                        .handle_event(&CrosstermEvent::Mouse(mouse))
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
        }
    }

    fn dispatch(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit | Action::ForceQuit => {
                self.should_quit = true;
            }
            Action::EnterPtyMode => {
                self.input_mode = InputMode::PtyRaw;
                self.focus.focus(FocusArea::Terminal);
                self.terminal_widget.scroll_to_bottom();
            }
            Action::EnterNormalMode => {
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
                if let Some(ref tx) = self.pty_tx {
                    let _ = tx.send(data);
                }
            }
            Action::PtyOutput(data) => {
                self.terminal_widget.update(&Action::PtyOutput(data));
            }
            Action::PtyResize { cols, rows } => {
                self.terminal_widget.update(&Action::PtyResize { cols, rows });
                if let Some(ref tx) = self.pty_resize_tx {
                    let _ = tx.send((cols, rows));
                }
            }
            Action::ScrollUp(n) => {
                self.terminal_widget.update(&Action::ScrollUp(n));
            }
            Action::ScrollDown(n) => {
                self.terminal_widget.update(&Action::ScrollDown(n));
            }
            Action::ScrollPageUp => {
                self.terminal_widget.update(&Action::ScrollPageUp);
            }
            Action::ScrollPageDown => {
                self.terminal_widget.update(&Action::ScrollPageDown);
            }
            Action::ScrollToTop => {
                self.terminal_widget.update(&Action::ScrollToTop);
            }
            Action::ScrollToBottom => {
                self.terminal_widget.update(&Action::ScrollToBottom);
            }
            Action::Copy => {
                if let Some(text) = self.terminal_widget.get_selected_text() {
                    if let Some(ref mut clipboard) = self.clipboard {
                        let _ = clipboard.set_text(text);
                    }
                }
                self.terminal_widget.clear_selection();
            }
            Action::Paste => {
                if let Some(ref mut clipboard) = self.clipboard {
                    if let Ok(text) = clipboard.get_text() {
                        if let Some(ref tx) = self.pty_tx {
                            let _ = tx.send(text.into_bytes());
                        }
                    }
                }
            }
            Action::MenuSelectNext => {
                self.menu.update(&Action::MenuSelectNext);
            }
            Action::MenuSelectPrev => {
                self.menu.update(&Action::MenuSelectPrev);
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
                self.menu.set_stream_count(self.stream_manager.clients().len());
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
