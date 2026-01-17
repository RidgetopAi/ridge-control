// UI chrome dispatch: notifications, context menu, spinners, ask_user dialog
// Domain: Notification toasts, context menus, spinner animations, ask_user dialogs

use crate::action::Action;
use crate::components::spinner_manager::SpinnerKey;
use crate::error::Result;

use super::super::App;

impl App {
    pub(super) fn dispatch_ui_chrome(&mut self, action: Action) -> Result<()> {
        match action {
            // Notification actions (TRC-023)
            Action::NotifyInfo(title) => {
                self.ui.notification_manager.info(title);
            }
            Action::NotifyInfoMessage(title, message) => {
                self.ui.notification_manager.info_with_message(title, message);
            }
            Action::NotifySuccess(title) => {
                self.ui.notification_manager.success(title);
            }
            Action::NotifySuccessMessage(title, message) => {
                self.ui.notification_manager.success_with_message(title, message);
            }
            Action::NotifyWarning(title) => {
                self.ui.notification_manager.warning(title);
            }
            Action::NotifyWarningMessage(title, message) => {
                self.ui.notification_manager.warning_with_message(title, message);
            }
            Action::NotifyError(title) => {
                self.ui.notification_manager.error(title);
            }
            Action::NotifyErrorMessage(title, message) => {
                self.ui.notification_manager.error_with_message(title, message);
            }
            Action::NotifyDismiss => {
                self.ui.notification_manager.dismiss_first();
            }
            Action::NotifyDismissAll => {
                self.ui.notification_manager.dismiss_all();
            }

            // Context menu actions (TRC-020)
            Action::ContextMenuShow { x, y, target } => {
                let items = self.build_context_menu_items(&target);
                self.ui.context_menu.show(x, y, target, items);
            }
            Action::ContextMenuClose => {
                self.ui.context_menu.hide();
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

            // Spinner actions (TRC-015)
            Action::SpinnerTick => {
                self.ui.spinner_manager.tick();
            }
            Action::SpinnerStart(name, label) => {
                self.ui.spinner_manager.start(SpinnerKey::custom(name), label);
            }
            Action::SpinnerStop(name) => {
                self.ui.spinner_manager.stop(&SpinnerKey::custom(name));
            }
            Action::SpinnerSetLabel(name, label) => {
                self.ui.spinner_manager.set_label(&SpinnerKey::custom(name), label);
            }

            // Ask User dialog actions (T2.4)
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
                self.ui.ask_user_dialog.show(request.tool_use_id.clone(), questions);
            }
            Action::AskUserCancel => {
                // User cancelled - create error result
                if self.ui.ask_user_dialog.is_visible() {
                    // Note: We don't have the tool_use_id here, so the dialog handles sending cancel
                    self.ui.ask_user_dialog.hide();
                }
            }
            Action::AskUserRespond(ref response) => {
                // User responded - create tool result with answers
                self.ui.ask_user_dialog.hide();
                let answers_json = serde_json::json!({
                    "answers": response.answers
                });
                let tool_result = crate::llm::ToolResult {
                    tool_use_id: response.tool_use_id.clone(),
                    content: crate::llm::ToolResultContent::Text(answers_json.to_string()),
                    is_error: false,
                };
                // Remove from pending and add to collected
                self.agent.pending_tools.remove(&response.tool_use_id);
                self.agent.collected_results.insert(response.tool_use_id.clone(), tool_result);

                // Check if we have all results for current batch
                if let Some((batch_id, expected)) = self.agent.expected_tool_batch {
                    if self.agent.collected_results.len() >= expected {
                        let results: Vec<crate::llm::ToolResult> = self.agent.collected_results.drain().map(|(_, r)| r).collect();
                        self.agent.agent_engine.continue_after_tools(results);
                        self.agent.expected_tool_batch = None;
                        self.agent.tool_batch_map.retain(|_, bid| *bid != batch_id);
                    }
                }
            }

            // These ask_user actions are handled by the dialog's handle_event
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

            _ => unreachable!("non-ui-chrome action passed to dispatch_ui_chrome: {:?}", action),
        }
        Ok(())
    }
}
