// Input event handling: keyboard, mouse, paste
// Domain: Event routing to appropriate handlers based on input mode and focus

use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
use ratatui::layout::Rect;

use crate::action::{Action, ContextMenuTarget, PaneBorder};
use crate::components::pane_layout::ResizableBorder;
use crate::components::Component;
use crate::input::focus::FocusArea;
use crate::input::mode::InputMode;
use crate::tabs::TabBar;

use super::super::App;

impl App {
    pub(in crate::app) fn handle_event(&mut self, event: CrosstermEvent) -> Option<Action> {
        match event {
            CrosstermEvent::Key(key) => self.handle_key(key),
            CrosstermEvent::Mouse(mouse) => self.handle_mouse(mouse),
            CrosstermEvent::Paste(text) => self.handle_paste(text),
            CrosstermEvent::Resize(cols, rows) => {
                let (term_cols, term_rows) = crate::app::pty_state::PtyState::calculate_terminal_size(Rect::new(0, 0, cols, rows));
                Some(Action::PtyResize {
                    cols: term_cols,
                    rows: term_rows,
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
        } else if self.ui.focus.is_focused(FocusArea::ChatInput) {
            // Paste to chat input when focused
            self.agent.chat_input.paste_text(&text);
            None
        } else {
            // Otherwise paste to active tab's PTY with bracketed paste mode
            // This prevents the shell from executing each line individually
            let mut data = Vec::with_capacity(text.len() + 12);
            data.extend_from_slice(b"\x1b[200~"); // Start bracketed paste
            data.extend_from_slice(text.as_bytes());
            data.extend_from_slice(b"\x1b[201~"); // End bracketed paste
            self.pty.tab_manager.write_to_active_pty(data);
            None
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Debug: Log key events to help diagnose input issues
        #[cfg(debug_assertions)]
        tracing::debug!("Key event: {:?}, mode: {:?}, focus: {:?}", key, self.ui.input_mode, self.ui.focus.current());

        // Modal dialogs take highest priority (they change input mode)
        // T2.4: Ask user dialog takes priority when visible
        if self.ui.ask_user_dialog.is_visible() {
            return self.ui.ask_user_dialog.handle_event(&CrosstermEvent::Key(key));
        }

        // Command palette and confirm dialog take priority over overlay panels
        match &self.ui.input_mode {
            InputMode::Confirm { .. } => {
                return self.ui.confirm_dialog.handle_event(&CrosstermEvent::Key(key));
            }
            InputMode::CommandPalette => {
                return self.ui.command_palette.handle_event(&CrosstermEvent::Key(key));
            }
            _ => {}
        }

        // SIRK Panel handles events when visible, but only if it recognizes the key
        // (fall through for unhandled keys like Ctrl+C)
        if self.ui.sirk_panel_visible {
            if let Some(ref mut sirk_panel) = self.sirk_panel {
                if let Some(action) = sirk_panel.handle_event(&CrosstermEvent::Key(key)) {
                    return Some(action);
                }
                // Fall through for unhandled keys
            }
        }

        // Activity Stream handles events when visible, but only if it recognizes the key
        // Skip in PtyRaw mode - character keys should pass through to the terminal
        // (fall through for unhandled keys like Ctrl+C)
        if self.ui.activity_stream_visible && !matches!(self.ui.input_mode, InputMode::PtyRaw) {
            if let Some(ref mut activity_stream) = self.activity_stream {
                if let Some(action) = activity_stream.handle_event(&CrosstermEvent::Key(key)) {
                    return Some(action);
                }
                // Fall through for unhandled keys
            }
        }

        match &self.ui.input_mode {
            InputMode::PtyRaw => {
                // First check configurable keybindings
                if let Some(action) = self.config_manager.keybindings().get_action(&self.ui.input_mode, &key) {
                    return Some(action);
                }

                // Double-tap ESC exits PTY mode (single ESC passes through to PTY)
                // This allows nvim/vim to receive single ESC for mode switching
                if key.code == KeyCode::Esc {
                    let now = std::time::Instant::now();
                    const DOUBLE_TAP_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(300);

                    if let Some(last) = self.ui.last_esc_press {
                        if now.duration_since(last) < DOUBLE_TAP_THRESHOLD {
                            // Double-tap detected: exit PTY mode
                            self.ui.last_esc_press = None;
                            return Some(Action::EnterNormalMode);
                        }
                    }
                    // First ESC or too slow: record time and pass ESC to PTY
                    self.ui.last_esc_press = Some(now);
                    return Some(Action::PtyInput(vec![0x1b])); // Send ESC byte to PTY
                }

                // Copy with selection (special handling) - use active tab's terminal (TRC-005)
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    let has_selection = self.pty.tab_manager
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
                let focus_action = match self.ui.focus.current() {
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
                        self.ui.menu.handle_event(&CrosstermEvent::Key(key))
                    }
                    FocusArea::StreamViewer => {
                        // Handle StreamViewer key events (also used for conversation history)
                        // 'i' focuses chat input when conversation is visible
                        if self.agent.show_conversation && (key.code == KeyCode::Char('i') || key.code == KeyCode::Tab) {
                            self.ui.focus.focus(FocusArea::ChatInput);
                            return None;
                        }

                        // When conversation is visible, route scroll keys to conversation viewer
                        if self.agent.show_conversation {
                            match key.code {
                                KeyCode::Char('j') | KeyCode::Down => Some(Action::ConversationScrollDown(1)),
                                KeyCode::Char('k') | KeyCode::Up => Some(Action::ConversationScrollUp(1)),
                                KeyCode::Char('g') => Some(Action::ConversationScrollToTop),
                                KeyCode::Char('G') => Some(Action::ConversationScrollToBottom),
                                KeyCode::PageUp => Some(Action::ConversationScrollUp(10)),
                                KeyCode::PageDown => Some(Action::ConversationScrollDown(10)),
                                KeyCode::Char('a') => {
                                    self.agent.conversation_viewer.toggle_auto_scroll();
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
                            self.ui.focus.focus(FocusArea::StreamViewer);
                            return None;
                        }
                        self.agent.chat_input.handle_event(&CrosstermEvent::Key(key))
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
                self.config_manager.keybindings().get_action(&self.ui.input_mode, &key)
            }
            InputMode::Insert { ref target } => {
                // TRC-029: Handle inline tab rename input
                if matches!(target, crate::input::mode::InsertTarget::TabRename) {
                    match key.code {
                        KeyCode::Esc => return Some(Action::TabCancelRename),
                        KeyCode::Enter => {
                            // Confirm rename and exit insert mode
                            self.pty.tab_manager.confirm_rename();
                            self.ui.input_mode = InputMode::Normal;
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
                            self.agent.thread_rename_buffer = None;
                            self.ui.input_mode = InputMode::Normal;
                            return Some(Action::ThreadCancelRename);
                        }
                        KeyCode::Enter => {
                            // Confirm rename
                            if let Some(new_name) = self.agent.thread_rename_buffer.take() {
                                self.ui.input_mode = InputMode::Normal;
                                return Some(Action::ThreadRename(new_name));
                            }
                            self.ui.input_mode = InputMode::Normal;
                            return None;
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut buffer) = self.agent.thread_rename_buffer {
                                buffer.pop();
                            }
                            return None;
                        }
                        KeyCode::Char(c) => {
                            if let Some(ref mut buffer) = self.agent.thread_rename_buffer {
                                buffer.push(c);
                            }
                            return None;
                        }
                        _ => {}
                    }
                }

                // Fall back to configurable keybindings for other insert targets
                if let Some(action) = self.config_manager.keybindings().get_action(&self.ui.input_mode, &key) {
                    return Some(action);
                }
                None
            }
            InputMode::ThreadPicker => {
                // P2-003: Route keyboard events to thread picker component
                if let Some(action) = self.agent.thread_picker.handle_event(&CrosstermEvent::Key(key)) {
                    return Some(action);
                }
                // If picker was hidden (Esc pressed), return to normal mode
                if !self.agent.thread_picker.is_visible() {
                    self.ui.input_mode = InputMode::Normal;
                }
                None
            }
            // These are handled earlier in the function, but must be listed for exhaustiveness
            InputMode::CommandPalette | InputMode::Confirm { .. } => {
                unreachable!("CommandPalette and Confirm modes are handled earlier")
            }
        }
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        // DEBUG: Log scroll events to trace the issue
        if matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
            tracing::debug!(
                "SCROLL EVENT: kind={:?}, row={}, col={}, input_mode={:?}, focus={:?}",
                mouse.kind, mouse.row, mouse.column, self.ui.input_mode, self.ui.focus.current()
            );
        }

        // TRC-020: If context menu is visible, route all mouse events to it first
        if self.ui.context_menu.is_visible() {
            if let Some(action) = self.ui.context_menu.handle_event(&CrosstermEvent::Mouse(mouse)) {
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
            if self.ui.tab_bar_area.height > 0
                && mouse.row >= self.ui.tab_bar_area.y
                && mouse.row < self.ui.tab_bar_area.y + self.ui.tab_bar_area.height
                && mouse.column >= self.ui.tab_bar_area.x
                && mouse.column < self.ui.tab_bar_area.x + self.ui.tab_bar_area.width
            {
                // Hit-test against tabs
                let tab_bar = TabBar::from_manager(&self.pty.tab_manager);
                let hit_areas = tab_bar.calculate_hit_areas(self.ui.tab_bar_area);

                for (start_x, end_x, tab_index) in hit_areas {
                    if mouse.column >= start_x && mouse.column < end_x {
                        return Some(Action::TabSelect(tab_index));
                    }
                }
                // Click was in tab bar but not on a tab - consume event
                return None;
            }

            // TRC-024: Check for clicks on pane borders for resize
            let show_conv = self.agent.show_conversation || !self.agent.llm_response_buffer.is_empty() || !self.agent.thinking_buffer.is_empty();
            if let Some(border) = self.ui.pane_layout.hit_test_border(mouse.column, mouse.row, self.ui.content_area, show_conv) {
                let pan_border = match border {
                    ResizableBorder::MainVertical => PaneBorder::MainVertical,
                    ResizableBorder::RightHorizontal => PaneBorder::RightHorizontal,
                    ResizableBorder::LeftHorizontal => PaneBorder::LeftHorizontal,
                };
                return Some(Action::PaneStartDrag(pan_border));
            }
        }

        // TRC-024: Handle drag events for pane resizing
        if self.ui.drag_state.is_dragging() {
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
        if self.agent.conversation_viewer.is_selecting() {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(action) = self.agent.conversation_viewer.handle_mouse(mouse) {
                        return Some(action);
                    }
                    return None;
                }
                _ => {}
            }
        }

        // Handle ongoing chat input text selection (drag/up events while selecting)
        if self.agent.chat_input.is_selecting() {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(action) = self.agent.chat_input.handle_mouse(mouse) {
                        return Some(action);
                    }
                    return None;
                }
                _ => {}
            }
        }

        // Mouse events over conversation area - route to conversation viewer for selection
        if self.ui.conversation_area.height > 0 {
            let in_conversation = mouse.row >= self.ui.conversation_area.y
                && mouse.row < self.ui.conversation_area.y + self.ui.conversation_area.height
                && mouse.column >= self.ui.conversation_area.x
                && mouse.column < self.ui.conversation_area.x + self.ui.conversation_area.width;

            // DEBUG
            if matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
                tracing::debug!(
                    "CONV CHECK: area=({},{} {}x{}), in_conv={}",
                    self.ui.conversation_area.x, self.ui.conversation_area.y,
                    self.ui.conversation_area.width, self.ui.conversation_area.height,
                    in_conversation
                );
            }

            if in_conversation {
                // Route mouse events to conversation viewer for text selection
                if let Some(action) = self.agent.conversation_viewer.handle_mouse(mouse) {
                    tracing::debug!("CONV HANDLER returned: {:?}", action);
                    return Some(action);
                }
                // Focus on click if no action returned
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    self.ui.focus.focus(FocusArea::StreamViewer);
                    return None;
                }
                // Don't return None for unhandled events - let them fall through
            }
        }

        // Mouse events over chat input area - route to chat_input for selection
        if self.ui.chat_input_area.height > 0 {
            let in_chat_input = mouse.row >= self.ui.chat_input_area.y
                && mouse.row < self.ui.chat_input_area.y + self.ui.chat_input_area.height
                && mouse.column >= self.ui.chat_input_area.x
                && mouse.column < self.ui.chat_input_area.x + self.ui.chat_input_area.width;

            // DEBUG
            if matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
                tracing::debug!(
                    "CHAT CHECK: area=({},{} {}x{}), in_chat={}",
                    self.ui.chat_input_area.x, self.ui.chat_input_area.y,
                    self.ui.chat_input_area.width, self.ui.chat_input_area.height,
                    in_chat_input
                );
            }

            if in_chat_input {
                // Route mouse events to chat input for text selection
                if let Some(action) = self.agent.chat_input.handle_mouse(mouse) {
                    tracing::debug!("CHAT HANDLER returned: {:?}", action);
                    return Some(action);
                }
                // Focus on click if no action returned
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    self.ui.focus.focus(FocusArea::ChatInput);
                    return None;
                }
                // Don't return None for unhandled events - let them fall through
                // to focus-based handling (enables PtyRaw scroll passthrough)
            }
        }

        // In PtyRaw mode, scroll always goes to PTY regardless of focus
        // (user is interacting with nested TUI, scroll should go there)
        if self.ui.input_mode == InputMode::PtyRaw {
            if matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
                let mouse_mode = self.pty.tab_manager.active_mouse_mode();
                
                // Calculate coordinates relative to terminal pane (1-based for SGR)
                let term_area = self.ui.terminal_area;
                let rel_x = mouse.column.saturating_sub(term_area.x).saturating_add(1);
                let rel_y = mouse.row.saturating_sub(term_area.y).saturating_add(1);
                
                tracing::debug!(
                    "PTYRAW SCROLL: mouse_mode={:?}, sgr={}, any_enabled={}",
                    mouse_mode, mouse_mode.sgr_ext, mouse_mode.any_enabled()
                );
                
                if mouse_mode.any_enabled() {
                    // Nested app has mouse tracking enabled - send SGR mouse wheel sequences
                    // SGR format: CSI < button ; x ; y M  (button: 64=wheel up, 65=wheel down)
                    let button = match mouse.kind {
                        MouseEventKind::ScrollUp => 64,
                        MouseEventKind::ScrollDown => 65,
                        _ => return None,
                    };
                    
                    // SGR encoding: \x1b[<{button};{x};{y}M
                    let seq = format!("\x1b[<{};{};{}M", button, rel_x, rel_y);
                    tracing::debug!("PTYRAW SGR: sending {:?}", seq);
                    return Some(Action::PtyInput(seq.into_bytes()));
                } else {
                    // Nested app doesn't have mouse tracking - check if in alternate screen
                    let in_alt_screen = self.pty.tab_manager.is_active_alternate_screen();
                    
                    if in_alt_screen {
                        // In alternate screen (TUI app like CC, vim, less) - send application cursor keys
                        // This is what Windows Terminal does with alternate scroll mode (DECSET 1007)
                        // Application cursor keys: ESC O A (up), ESC O B (down)
                        let arrow_seq = match mouse.kind {
                            MouseEventKind::ScrollUp => b"\x1bOA",   // Application mode Up
                            MouseEventKind::ScrollDown => b"\x1bOB", // Application mode Down
                            _ => return None,
                        };
                        tracing::debug!("PTYRAW ALT-SCREEN: sending application cursor key {:?}", arrow_seq);
                        return Some(Action::PtyInput(arrow_seq.to_vec()));
                    } else {
                        // Not in alternate screen - scroll our terminal buffer
                        tracing::debug!("PTYRAW NORMAL: scrolling terminal buffer");
                        return match mouse.kind {
                            MouseEventKind::ScrollUp => Some(Action::ScrollUp(3)),
                            MouseEventKind::ScrollDown => Some(Action::ScrollDown(3)),
                            _ => None,
                        };
                    }
                }
            }
        }

        // Focus-based mouse handling
        match self.ui.focus.current() {
            FocusArea::Terminal => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left) => {
                    // Use active tab's terminal widget (TRC-005)
                    if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                        session.terminal_mut()
                            .handle_event(&CrosstermEvent::Mouse(mouse))
                    } else {
                        None
                    }
                }
                // Scroll in Normal mode scrolls the terminal scrollback
                // (PtyRaw scroll is handled earlier, before focus-based routing)
                MouseEventKind::ScrollUp => Some(Action::ScrollUp(3)),
                MouseEventKind::ScrollDown => Some(Action::ScrollDown(3)),
                _ => None,
            },
            FocusArea::ProcessMonitor => {
                self.process_monitor
                    .handle_event(&CrosstermEvent::Mouse(mouse))
            }
            FocusArea::Menu => {
                self.ui.menu.handle_event(&CrosstermEvent::Mouse(mouse))
            }
            // Overlay areas
            FocusArea::StreamViewer => {
                // Handle mouse scroll for conversation/stream viewer
                if self.agent.show_conversation {
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
        let term_size = self.pty.terminal.size().ok()?;
        let screen = Rect::new(0, 0, term_size.width, term_size.height);

        // Check tab bar first
        if self.ui.tab_bar_area.height > 0 && self.ui.tab_bar_area.contains((x, y).into()) {
            let tab_bar = TabBar::from_manager(&self.pty.tab_manager);
            let hit_areas = tab_bar.calculate_hit_areas(self.ui.tab_bar_area);

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
        let show_tabs = self.pty.tab_manager.count() > 1 || self.agent.dangerous_mode;
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
         if self.ui.chat_input_area.height > 0 && self.ui.chat_input_area.contains((x, y).into()) {
            // Focus ChatInput when right-clicking on it
            self.ui.focus.focus(FocusArea::ChatInput);
            Some(Action::ContextMenuShow {
                x,
                y,
                target: ContextMenuTarget::ChatInput,
            })
        } else if self.ui.conversation_area.height > 0 && self.ui.conversation_area.contains((x, y).into()) {
            // Conversation viewer area
            self.ui.focus.focus(FocusArea::StreamViewer);
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
            let stream_idx = self.ui.menu.selected_index();
            Some(Action::ContextMenuShow {
                x,
                y,
                target: ContextMenuTarget::Stream(stream_idx),
            })
        }
    }
}

/// Convert a key event to bytes for PTY input
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
