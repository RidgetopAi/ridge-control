// Event handlers and action dispatch
// Extracted from mod.rs as part of REFACTOR-P5.4

use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
use ratatui::layout::Rect;

use crate::action::{Action, ContextMenuTarget, PaneBorder};
use crate::components::pane_layout::{ResizableBorder, ResizeDirection};
use crate::components::Component;
use crate::config::SecretString;
use crate::error::Result;
use crate::input::focus::FocusArea;
use crate::input::mode::InputMode;
use crate::llm::{PendingToolUse, ToolExecutionCheck};
use crate::agent::ThreadStore;
use crate::streams::ConnectionState;
use crate::tabs::TabBar;
use crate::components::spinner_manager::SpinnerKey;

use super::App;

impl App {
    pub(super) fn handle_event(&mut self, event: CrosstermEvent) -> Option<Action> {
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
    pub(super) fn handle_paste(&mut self, text: String) -> Option<Action> {
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
            // Otherwise paste to active tab's PTY with bracketed paste mode
            // This prevents the shell from executing each line individually
            let mut data = Vec::with_capacity(text.len() + 12);
            data.extend_from_slice(b"\x1b[200~"); // Start bracketed paste
            data.extend_from_slice(text.as_bytes());
            data.extend_from_slice(b"\x1b[201~"); // End bracketed paste
            self.tab_manager.write_to_active_pty(data);
            None
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
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

                // Double-tap ESC exits PTY mode (single ESC passes through to PTY)
                // This allows nvim/vim to receive single ESC for mode switching
                if key.code == KeyCode::Esc {
                    let now = std::time::Instant::now();
                    const DOUBLE_TAP_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(300);
                    
                    if let Some(last) = self.last_esc_press {
                        if now.duration_since(last) < DOUBLE_TAP_THRESHOLD {
                            // Double-tap detected: exit PTY mode
                            self.last_esc_press = None;
                            return Some(Action::EnterNormalMode);
                        }
                    }
                    // First ESC or too slow: record time and pass ESC to PTY
                    self.last_esc_press = Some(now);
                    return Some(Action::PtyInput(vec![0x1b])); // Send ESC byte to PTY
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

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
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
    pub(super) fn handle_right_click(&mut self, x: u16, y: u16) -> Option<Action> {
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

    pub(super) fn dispatch(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Noop => {
                // Do nothing - used to consume input without triggering behavior
            }
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
                            // Otherwise paste to active tab's PTY with bracketed paste mode (TRC-005)
                            // This prevents the shell from executing each line individually
                            let mut data = Vec::with_capacity(text.len() + 12);
                            data.extend_from_slice(b"\x1b[200~"); // Start bracketed paste
                            data.extend_from_slice(text.as_bytes());
                            data.extend_from_slice(b"\x1b[201~"); // End bracketed paste
                            self.tab_manager.write_to_active_pty(data);
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
            // Continue with remaining dispatch arms...
            _ => {
                self.dispatch_continued(action)?;
            }
        }
        Ok(())
    }

    /// Continuation of dispatch for remaining action types (split for readability)
    fn dispatch_continued(&mut self, action: Action) -> Result<()> {
        match action {
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
            // Remaining actions handled in dispatch_continued2
            _ => {
                self.dispatch_continued2(action)?;
            }
        }
        Ok(())
    }

    /// Third part of dispatch for thread, config, tab, and UI actions
    fn dispatch_continued2(&mut self, action: Action) -> Result<()> {
        match action {
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
            // Continue in dispatch_continued3
            _ => {
                self.dispatch_continued3(action)?;
            }
        }
        Ok(())
    }

    /// Fourth part of dispatch for tab, key storage, session, log, config panel, and UI actions
    fn dispatch_continued3(&mut self, action: Action) -> Result<()> {
        match action {
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
                                use crate::config::KeyId;
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
            // Continue in dispatch_continued4
            _ => {
                self.dispatch_continued4(action)?;
            }
        }
        Ok(())
    }

    /// Fifth part of dispatch for spinner, tool call UI, context menu, notification, pane, settings
    fn dispatch_continued4(&mut self, action: Action) -> Result<()> {
        match action {
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
}

pub(super) fn key_to_bytes(key: KeyEvent) -> Vec<u8> {
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
