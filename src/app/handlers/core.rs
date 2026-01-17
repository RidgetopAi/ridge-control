// Core app lifecycle, modes, and focus dispatch
// Domain: App lifecycle (quit), input modes (pty/normal/command palette), focus navigation

use crate::action::Action;
use crate::components::Component;
use crate::error::Result;
use crate::input::focus::FocusArea;
use crate::input::mode::InputMode;

use super::super::App;

impl App {
    pub(super) fn dispatch_core(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Noop => {
                // Do nothing - used to consume input without triggering behavior
            }
            Action::Quit | Action::ForceQuit => {
                // Auto-save session on quit (TRC-012)
                self.save_session();
                self.should_quit = true;
            }
            Action::Tick => {
                self.process_monitor.update(&Action::Tick);
                // Tick all active spinners (TRC-015)
                self.ui.spinner_manager.tick();
                // Tick menu spinners for stream connection animations
                self.ui.menu.tick_spinners();
                // Tick conversation viewer spinner for LLM streaming
                self.agent.conversation_viewer.tick_spinner();
                // Tick notifications to expire old ones (TRC-023)
                self.ui.notification_manager.tick();
            }
            Action::EnterPtyMode => {
                self.ui.input_mode = InputMode::PtyRaw;
                self.ui.focus.focus(FocusArea::Terminal);
                // Scroll active tab's terminal to bottom (TRC-005)
                if let Some(session) = self.pty.tab_manager.active_pty_session_mut() {
                    session.terminal_mut().scroll_to_bottom();
                }
            }
            Action::EnterNormalMode => {
                self.ui.input_mode = InputMode::Normal;
                // Also close command palette if open
                if self.ui.command_palette.is_visible() {
                    self.ui.command_palette.hide();
                }
            }
            Action::OpenCommandPalette => {
                // Populate dynamic provider/model commands before showing
                let providers = self.agent.model_catalog.providers();
                let current_provider = self.agent.agent_engine.current_provider();
                let current_provider = if current_provider.is_empty() { "anthropic" } else { current_provider };
                self.ui.command_palette.set_providers(&providers, current_provider);

                let models = self.agent.model_catalog.models_for_provider(current_provider);
                let current_model = self.agent.agent_engine.current_model();
                self.ui.command_palette.set_models(&models, current_model);

                // Populate subagent model commands (T2.1b)
                self.refresh_subagent_commands();

                self.ui.command_palette.show();
                self.ui.input_mode = InputMode::CommandPalette;
            }
            Action::CloseCommandPalette => {
                self.ui.command_palette.hide();
                self.ui.input_mode = InputMode::Normal;
            }
            Action::FocusNext => {
                let skip_chat = !self.agent.show_conversation;
                self.ui.focus.next_skip_chat(skip_chat);
                if self.ui.focus.current() == FocusArea::ProcessMonitor {
                    self.process_monitor.ensure_selection();
                }
            }
            Action::FocusPrev => {
                let skip_chat = !self.agent.show_conversation;
                self.ui.focus.prev_skip_chat(skip_chat);
                if self.ui.focus.current() == FocusArea::ProcessMonitor {
                    self.process_monitor.ensure_selection();
                }
            }
            Action::FocusArea(area) => {
                self.ui.focus.focus(area);
                if area == FocusArea::ProcessMonitor {
                    self.process_monitor.ensure_selection();
                }
            }
            _ => unreachable!("non-core action passed to dispatch_core: {:?}", action),
        }
        Ok(())
    }
}
