// Terminal, PTY, tabs, sessions, and pane layout dispatch
// Domain: PTY input/output, scrolling, copy/paste, tab management, session persistence, pane resizing

use crate::action::{Action, PaneBorder};
use crate::components::Component;
use crate::components::pane_layout::{ResizableBorder, ResizeDirection};
use crate::error::Result;
use crate::input::mode::InputMode;

use super::super::App;

impl App {
    pub(super) fn dispatch_terminal_tabs(&mut self, action: Action) -> Result<()> {
        match action {
            // PTY actions
            Action::PtyInput(data) => {
                // Write to active tab's PTY (TRC-005)
                self.pty.tab_manager.write_to_active_pty(data);
            }
            Action::PtyOutput(_data) => {
                // PTY output is now handled by poll_pty_events (TRC-005)
                // This action is kept for backward compatibility but not used directly
            }
            Action::PtyResize { cols, rows } => {
                // Resize all PTY sessions (TRC-005)
                self.pty.tab_manager.set_terminal_size(cols, rows);
            }

            // Scroll actions
            Action::ScrollUp(n) => {
                // Scroll active tab's terminal (TRC-005)
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollUp(n));
                }
            }
            Action::ScrollDown(n) => {
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollDown(n));
                }
            }
            Action::ScrollPageUp => {
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollPageUp);
                }
            }
            Action::ScrollPageDown => {
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollPageDown);
                }
            }
            Action::ScrollToTop => {
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollToTop);
                }
            }
            Action::ScrollToBottom => {
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().update(&Action::ScrollToBottom);
                }
            }

            // Copy/Paste actions
            Action::Copy => {
                // Copy from active tab's terminal (TRC-005)
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    if let Some(text) = session.terminal().get_selected_text() {
                        if let Some(ref mut clipboard) = self.ui.clipboard {
                            let _ = clipboard.set_text(text);
                        }
                    }
                    session.terminal_mut().clear_selection();
                }
            }
            Action::Paste => {
                // Route paste based on focus and editing state
                if let Some(ref mut clipboard) = self.ui.clipboard {
                    if let Ok(text) = clipboard.get_text() {
                        // If settings editor is visible and in editing mode, paste there
                        if self.show_settings_editor && self.settings_editor.is_editing() {
                            self.settings_editor.paste_text(&text);
                        } else if self.ui.focus.is_focused(crate::input::focus::FocusArea::ChatInput) {
                            // Paste to chat input when focused
                            self.agent.chat_input.paste_text(&text);
                        } else {
                            // Otherwise paste to active tab's PTY with bracketed paste mode (TRC-005)
                            // This prevents the shell from executing each line individually
                            let mut data = Vec::with_capacity(text.len() + 12);
                            data.extend_from_slice(b"\x1b[200~"); // Start bracketed paste
                            data.extend_from_slice(text.as_bytes());
                            data.extend_from_slice(b"\x1b[201~"); // End bracketed paste
                            self.pty.tab_manager.write_to_active_pty(data);
                        }
                    }
                }
            }

            // Tab actions (TRC-005: Per-tab PTY isolation)
            Action::TabCreate => {
                let new_tab_id = self.pty.tab_manager.create_tab_default();
                // Spawn PTY for the new tab
                if let Err(e) = self.spawn_pty_for_tab(new_tab_id) {
                    tracing::error!("Failed to spawn PTY for new tab {}: {}", new_tab_id, e);
                    // TRC-023: Notify on PTY spawn failure
                    self.ui.notification_manager.error_with_message("Tab Error", format!("Failed to spawn shell: {}", e));
                } else {
                    // TRC-023: Notify tab creation
                    self.ui.notification_manager.info(format!("Tab {} created", self.pty.tab_manager.count()));
                }
            }
            Action::TabClose => {
                self.pty.tab_manager.close_active_tab();
                // PTY cleanup is handled by TabManager::close_tab
            }
            Action::TabCloseIndex(idx) => {
                if let Some(tab) = self.pty.tab_manager.tabs().get(idx) {
                    let id = tab.id();
                    self.pty.tab_manager.close_tab(id);
                    // PTY cleanup is handled by TabManager::close_tab
                }
            }
            Action::TabNext => {
                self.pty.tab_manager.next_tab();
                // Clear activity indicator when switching to a tab
                self.pty.tab_manager.clear_active_activity();
            }
            Action::TabPrev => {
                self.pty.tab_manager.prev_tab();
                self.pty.tab_manager.clear_active_activity();
            }
            Action::TabSelect(idx) => {
                self.pty.tab_manager.select(idx);
                self.pty.tab_manager.clear_active_activity();
            }
            Action::TabRename(name) => {
                self.pty.tab_manager.rename_active_tab(name);
            }
            Action::TabMove { from, to } => {
                self.pty.tab_manager.move_tab(from, to);
            }

            // TRC-029: Inline tab rename actions
            Action::TabStartRename => {
                self.pty.tab_manager.start_rename();
                self.ui.input_mode = InputMode::Insert { target: crate::input::mode::InsertTarget::TabRename };
            }
            Action::TabCancelRename => {
                self.pty.tab_manager.cancel_rename();
                self.ui.input_mode = InputMode::Normal;
            }
            Action::TabRenameInput(c) => {
                self.pty.tab_manager.rename_input(c);
            }
            Action::TabRenameBackspace => {
                self.pty.tab_manager.rename_backspace();
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

            // Pane resize actions (TRC-024)
            Action::PaneResizeMainGrow => {
                self.ui.pane_layout.resize_main(ResizeDirection::Grow);
            }
            Action::PaneResizeMainShrink => {
                self.ui.pane_layout.resize_main(ResizeDirection::Shrink);
            }
            Action::PaneResizeRightGrow => {
                self.ui.pane_layout.resize_right(ResizeDirection::Grow);
            }
            Action::PaneResizeRightShrink => {
                self.ui.pane_layout.resize_right(ResizeDirection::Shrink);
            }
            Action::PaneResizeLeftGrow => {
                self.ui.pane_layout.resize_left(ResizeDirection::Grow);
            }
            Action::PaneResizeLeftShrink => {
                self.ui.pane_layout.resize_left(ResizeDirection::Shrink);
            }
            Action::PaneResetLayout => {
                self.ui.pane_layout.reset_to_defaults();
            }
            Action::PaneStartDrag(border) => {
                let rb = match border {
                    PaneBorder::MainVertical => ResizableBorder::MainVertical,
                    PaneBorder::RightHorizontal => ResizableBorder::RightHorizontal,
                    PaneBorder::LeftHorizontal => ResizableBorder::LeftHorizontal,
                };
                self.ui.drag_state.start(rb);
            }
            Action::PaneDrag { x, y } => {
                if let Some(border) = self.ui.drag_state.border() {
                    self.ui.pane_layout.handle_mouse_drag(x, y, self.ui.content_area, border, self.agent.show_conversation);
                }
            }
            Action::PaneEndDrag => {
                self.ui.drag_state.stop();
            }

            _ => unreachable!("non-terminal/tabs action passed to dispatch_terminal_tabs: {:?}", action),
        }
        Ok(())
    }
}

