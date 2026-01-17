// Chat, LLM, threads, tools, and conversation dispatch
// Domain: LLM messaging, chat input, conversation viewer, tool execution, thread management

use crate::action::Action;
use crate::agent::ThreadStore;
use crate::components::spinner_manager::SpinnerKey;
use crate::error::Result;
use crate::input::focus::FocusArea;
use crate::input::mode::InputMode;
use crate::llm::{PendingToolUse, ToolExecutionCheck};

use super::super::App;

impl App {
    pub(super) fn dispatch_chat_llm(&mut self, action: Action) -> Result<()> {
        match action {
            // LLM messaging actions
            Action::LlmSendMessage(msg) => {
                tracing::info!("Sending LLM message: {} chars", msg.len());
                // Ensure conversation is visible when sending a message
                if !self.agent.show_conversation {
                    self.agent.show_conversation = true;
                }

                // Route through AgentEngine (always available)
                // Ensure we have an active thread
                if self.agent.agent_engine.current_thread().is_none() {
                    let model = self.agent.agent_engine.current_model().to_string();
                    self.agent.agent_engine.new_thread(model);
                    // TP2-002-15: Update current_thread_id when auto-creating thread
                    self.agent.current_thread_id = self.agent.agent_engine.current_thread().map(|t| t.id.clone());
                    tracing::info!("Created new AgentEngine thread: {:?}", self.agent.current_thread_id);
                }

                // Send message through AgentEngine
                self.agent.agent_engine.send_message(msg);
                tracing::info!("Message sent through AgentEngine");
            }
            Action::LlmCancel => {
                // Cancel AgentEngine's internal LLM
                self.agent.agent_engine.cancel();
                // Immediately stop spinner and clear buffers for responsive UI
                // (don't wait for async AgentEvent::Error to propagate)
                self.ui.spinner_manager.stop(&SpinnerKey::LlmLoading);
                self.agent.llm_response_buffer.clear();
                self.agent.thinking_buffer.clear();
                self.agent.current_block_type = None;
                self.agent.current_tool_id = None;
                self.agent.current_tool_name = None;
                self.agent.current_tool_input.clear();
                self.ui.notification_manager.info_with_message("Request Cancelled", "LLM request interrupted by user");
            }
            Action::LlmSelectModel(model) => {
                // Update AgentEngine's LLMManager
                self.agent.agent_engine.set_model(&model);
                // Also persist to config so model is remembered on restart
                self.config_manager.llm_config_mut().defaults.model = model.clone();
                if let Err(e) = self.config_manager.save_llm_config() {
                    tracing::warn!("Failed to save model selection: {}", e);
                }
            }
            Action::LlmSelectProvider(provider) => {
                // Update AgentEngine's LLMManager
                self.agent.agent_engine.set_provider(&provider);
            }
            Action::LlmClearConversation => {
                // Start a new thread to clear conversation (AgentEngine tracks via thread)
                let model = self.agent.agent_engine.current_model().to_string();
                self.agent.agent_engine.new_thread(model);
                self.agent.current_thread_id = self.agent.agent_engine.current_thread().map(|t| t.id.clone());
                // Also clear tool calls in conversation viewer (TRC-016)
                self.agent.conversation_viewer.clear_tool_calls();
            }
            Action::LlmStreamChunk(_) | Action::LlmStreamComplete | Action::LlmStreamError(_) => {
                // These are handled by handle_llm_event, not dispatched directly
            }

            // Chat input actions
            Action::ChatInputClear => {
                self.agent.chat_input.clear();
            }
            Action::ChatInputPaste(text) => {
                self.agent.chat_input.paste_text(&text);
            }
            Action::ChatInputCopy => {
                // Copy selected text from chat input to clipboard
                if let Some(text) = self.agent.chat_input.get_selected_text() {
                    if let Some(ref mut clipboard) = self.ui.clipboard {
                        let _ = clipboard.set_text(&text);
                        self.ui.notification_manager.info("Copied to clipboard");
                    }
                }
                self.agent.chat_input.clear_selection();
            }
            Action::ChatInputScrollUp(n) => {
                self.agent.chat_input.scroll_up(n);
            }
            Action::ChatInputScrollDown(n) => {
                self.agent.chat_input.scroll_down(n);
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

            // Conversation viewer actions
            Action::ConversationToggle => {
                self.agent.show_conversation = !self.agent.show_conversation;
                // When opening conversation, focus the chat input for typing
                if self.agent.show_conversation {
                    self.ui.focus.focus(FocusArea::ChatInput);
                } else {
                    // When closing, return focus to terminal
                    self.ui.focus.focus(FocusArea::Terminal);
                }
            }
            Action::ConversationScrollUp(n) => {
                self.agent.conversation_viewer.scroll_up(n);
            }
            Action::ConversationScrollDown(n) => {
                self.agent.conversation_viewer.scroll_down(n);
            }
            Action::ConversationScrollToTop => {
                self.agent.conversation_viewer.scroll_to_top();
            }
            Action::ConversationScrollToBottom => {
                self.agent.conversation_viewer.scroll_to_bottom();
            }
            Action::ConversationCopy => {
                // Copy selected text from conversation viewer to clipboard
                if let Some(text) = self.agent.conversation_viewer.get_selected_text() {
                    if let Some(ref mut clipboard) = self.ui.clipboard {
                        let _ = clipboard.set_text(&text);
                        self.ui.notification_manager.info("Copied to clipboard");
                    }
                }
                self.agent.conversation_viewer.clear_selection();
            }

            // Conversation search actions - placeholder, methods not yet fully implemented
            Action::ConversationSearchStart
            | Action::ConversationSearchClose
            | Action::ConversationSearchNext
            | Action::ConversationSearchPrev
            | Action::ConversationSearchQuery(_)
            | Action::ConversationSearchToggleCase => {
                // TODO: Implement search in ConversationViewer
            }

            // Tool execution actions
            Action::ToolUseReceived(pending) => {
                self.handle_tool_use_request(pending.tool.clone());
            }
            Action::ToolConfirm => {
                // User confirmed tool execution
                self.ui.confirm_dialog.dismiss();
                self.ui.input_mode = InputMode::Normal;

                // Get the tool from pending_tools using confirming_tool_id
                if let Some(tool_id) = self.agent.confirming_tool_id.take() {
                    if let Some(pending) = self.agent.pending_tools.remove(&tool_id) {
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
                self.ui.confirm_dialog.dismiss();
                self.ui.input_mode = InputMode::Normal;

                // Get the tool from pending_tools using confirming_tool_id
                if let Some(tool_id) = self.agent.confirming_tool_id.take() {
                    if let Some(pending) = self.agent.pending_tools.remove(&tool_id) {
                        // Update tool state in conversation viewer (TRC-016)
                        self.agent.conversation_viewer.reject_tool(&pending.tool.id);

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
                        self.agent.collected_results.insert(pending.tool.id.clone(), error_result);

                        // Check if we have all results for current batch
                        if let Some((batch_id, expected)) = self.agent.expected_tool_batch {
                            if self.agent.collected_results.len() >= expected {
                                let all_results: Vec<crate::llm::ToolResult> = self.agent.collected_results.drain().map(|(_, r)| r).collect();
                                self.agent.agent_engine.continue_after_tools(all_results);
                                self.agent.expected_tool_batch = None;
                                // Clean up batch mappings for completed batch
                                self.agent.tool_batch_map.retain(|_, bid| *bid != batch_id);
                            }
                        }
                    }
                }
            }
            Action::ToolResult(result) => {
                let tool_use_id = result.tool_use_id.clone();

                // Update tool state in conversation viewer (TRC-016)
                let tool_name = self.agent.pending_tools.get(&tool_use_id)
                    .map(|p| p.tool_name().to_string())
                    .unwrap_or_else(|| "Tool".to_string());
                self.agent.conversation_viewer.complete_tool(&tool_use_id, result.clone());

                // TRC-023: Notify on tool completion
                if result.is_error {
                    self.ui.notification_manager.warning_with_message(
                        format!("{} failed", tool_name),
                        "See conversation for details".to_string()
                    );
                }

                // Remove from pending_tools since we got the result
                self.agent.pending_tools.remove(&tool_use_id);

                // Get the batch this tool belongs to
                let tool_batch = self.agent.tool_batch_map.get(&tool_use_id).copied();

                // TP2-002-12: Bridge tool result to AgentEngine
                // Only collect if this tool belongs to current batch
                if let Some((current_batch_id, expected)) = self.agent.expected_tool_batch {
                    if tool_batch == Some(current_batch_id) {
                        self.agent.collected_results.insert(tool_use_id.clone(), result);

                        tracing::info!(
                            "📥 TOOL_RESULT collected: id={}, batch={}, collected={}/{} expected",
                            tool_use_id, current_batch_id, self.agent.collected_results.len(), expected
                        );

                        // Only continue when we have ALL expected results for this batch
                        if self.agent.collected_results.len() >= expected {
                            let all_results: Vec<crate::llm::ToolResult> = self.agent.collected_results.drain().map(|(_, r)| r).collect();

                            tracing::info!(
                                "✅ ALL_TOOLS_COMPLETE: batch={}, sending {} results to engine",
                                current_batch_id, all_results.len()
                            );

                            self.agent.agent_engine.continue_after_tools(all_results);

                            // Reset tracking state for next tool batch
                            self.agent.expected_tool_batch = None;
                            // Clean up batch mappings for completed batch
                            self.agent.tool_batch_map.retain(|_, bid| *bid != current_batch_id);
                        }
                    } else {
                        // Tool from a different/stale batch - log and ignore
                        tracing::warn!(
                            "⚠️ TOOL_RESULT from stale batch: id={}, tool_batch={:?}, current_batch={}",
                            tool_use_id, tool_batch, current_batch_id
                        );
                    }
                } else {
                    // No active batch - this is a stale result
                    tracing::warn!(
                        "⚠️ TOOL_RESULT with no active batch: id={}, tool_batch={:?}",
                        tool_use_id, tool_batch
                    );
                }
            }
            Action::ToolToggleDangerousMode => {
                let current = self.agent.dangerous_mode;
                self.agent.set_dangerous_mode(!current);
            }
            Action::ToolSetDangerousMode(enabled) => {
                self.agent.set_dangerous_mode(enabled);
            }

            // Tool Call UI actions (TRC-016)
            Action::ToolCallNextTool => {
                self.agent.conversation_viewer.select_next_tool();
            }
            Action::ToolCallPrevTool => {
                self.agent.conversation_viewer.select_prev_tool();
            }
            Action::ToolCallToggleExpand => {
                self.agent.conversation_viewer.toggle_selected_tool();
            }
            Action::ToolCallExpandAll => {
                self.agent.conversation_viewer.expand_all_tools();
            }
            Action::ToolCallCollapseAll => {
                self.agent.conversation_viewer.collapse_all_tools();
            }
            Action::ToolCallStartExecution(tool_id) => {
                self.agent.conversation_viewer.start_tool_execution(&tool_id);
            }
            Action::ToolCallRegister(tool_use) => {
                self.agent.conversation_viewer.register_tool_use(tool_use);
            }

            // TRC-017: Thinking block toggle
            Action::ThinkingToggleCollapse => {
                self.agent.conversation_viewer.toggle_thinking_collapse();
            }

            // Tool result collapse toggle
            Action::ToolResultToggleCollapse => {
                self.agent.conversation_viewer.toggle_tool_results_collapse();
            }

            // Phase 4: Tool verbosity cycle
            Action::ToolVerbosityCycle => {
                self.agent.conversation_viewer.cycle_tool_verbosity();
            }

            // Thread management actions (Phase 2)
            Action::ThreadNew => {
                let model = self.agent.agent_engine.current_model().to_string();
                self.agent.agent_engine.new_thread(model);
                self.agent.current_thread_id = self.agent.agent_engine.current_thread().map(|t| t.id.clone());
                self.agent.conversation_viewer.clear();
                self.ui.notification_manager.info("New conversation thread started");
                tracing::info!("Created new thread: {:?}", self.agent.current_thread_id);
            }
            Action::ThreadLoad(id) => {
                // TP2-002-09: Load existing thread by ID
                match self.agent.agent_engine.load_thread(&id) {
                    Ok(()) => {
                        self.agent.current_thread_id = Some(id.clone());
                        // Clear and repopulate conversation viewer
                        self.agent.conversation_viewer.clear();

                        // Phase 2: Re-populate tool calls from loaded thread segments
                        // Register all tool uses first, then complete with results
                        if let Some(thread) = self.agent.agent_engine.current_thread() {
                            for segment in thread.segments() {
                                for message in &segment.messages {
                                    for content_block in &message.content {
                                        match content_block {
                                            crate::llm::ContentBlock::ToolUse(tool_use) => {
                                                self.agent.conversation_viewer.register_tool_use(tool_use.clone());
                                            }
                                            crate::llm::ContentBlock::ToolResult(result) => {
                                                // Complete the tool with its result
                                                self.agent.conversation_viewer.complete_tool(&result.tool_use_id, result.clone());
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }

                            let title = thread.title.clone();
                            self.ui.notification_manager.info(format!("Loaded thread: {}", title));
                            tracing::info!("Loaded thread: {} ({})", title, id);
                        }
                    }
                    Err(e) => {
                        self.ui.notification_manager.error_with_message("Failed to load thread", e.clone());
                        tracing::error!("Failed to load thread {}: {}", id, e);
                    }
                }
            }
            Action::ThreadList => {
                // Future: Show thread list UI
            }
            Action::ThreadSave => {
                // TP2-002-10: Manually save current thread to DiskThreadStore
                match self.agent.agent_engine.save_thread() {
                    Ok(()) => {
                        if let Some(thread) = self.agent.agent_engine.current_thread() {
                            let title = thread.title.clone();
                            self.ui.notification_manager.success(format!("Thread saved: {}", title));
                            tracing::info!("Manually saved thread: {} ({})", title, thread.id);
                        } else {
                            self.ui.notification_manager.success("Thread saved");
                        }
                    }
                    Err(e) => {
                        self.ui.notification_manager.error_with_message("Failed to save thread", e.clone());
                        tracing::error!("Failed to save thread: {}", e);
                    }
                }
            }
            Action::ThreadClear => {
                // TP2-002-11: Clear current thread (start fresh without deleting)
                if let Some(thread) = self.agent.agent_engine.current_thread_mut() {
                    // Clear the thread segments
                    thread.clear();
                    // Clear the UI
                    self.agent.conversation_viewer.clear();
                    // Notify user
                    self.ui.notification_manager.info("Conversation cleared");
                    tracing::debug!("ThreadClear: cleared current thread and conversation viewer");
                } else {
                    self.ui.notification_manager.warning("No active conversation to clear");
                    tracing::warn!("ThreadClear: no current thread to clear");
                }
            }

            // P2-003: Thread picker actions
            Action::ThreadPickerShow => {
                // Get thread summaries from DiskThreadStore
                let summaries = self.agent.agent_engine.thread_store().list_summary();
                if summaries.is_empty() {
                    self.ui.notification_manager.warning("No saved threads to continue");
                } else {
                    self.agent.thread_picker.show(summaries);
                    self.ui.input_mode = InputMode::ThreadPicker;
                    tracing::debug!("ThreadPickerShow: showing thread picker");
                }
            }
            Action::ThreadPickerHide => {
                self.agent.thread_picker.hide();
                self.ui.input_mode = InputMode::Normal;
                tracing::debug!("ThreadPickerHide: hiding thread picker");
            }

            // P2-003: Thread rename actions
            Action::ThreadStartRename => {
                if let Some(thread) = self.agent.agent_engine.current_thread() {
                    // Initialize rename buffer with current title
                    self.agent.thread_rename_buffer = Some(thread.title.clone());
                    self.ui.input_mode = InputMode::Insert { target: crate::input::mode::InsertTarget::ThreadRename };
                    self.ui.notification_manager.info("Editing thread name (Enter to confirm, Esc to cancel)");
                    tracing::debug!("ThreadStartRename: started rename mode with title '{}'", thread.title);
                } else {
                    self.ui.notification_manager.warning("No active thread to rename");
                }
            }
            Action::ThreadCancelRename => {
                self.agent.thread_rename_buffer = None;
                self.ui.input_mode = InputMode::Normal;
                self.ui.notification_manager.info("Rename cancelled");
                tracing::debug!("ThreadCancelRename: cancelled rename");
            }
            Action::ThreadRenameInput(c) => {
                if let Some(ref mut buffer) = self.agent.thread_rename_buffer {
                    buffer.push(c);
                }
            }
            Action::ThreadRenameBackspace => {
                if let Some(ref mut buffer) = self.agent.thread_rename_buffer {
                    buffer.pop();
                }
            }
            Action::ThreadRename(new_name) => {
                match self.agent.agent_engine.rename_thread(&new_name) {
                    Ok(()) => {
                        self.ui.notification_manager.success(format!("Thread renamed to '{}'", new_name));
                        tracing::info!("ThreadRename: renamed thread to '{}'", new_name);
                    }
                    Err(e) => {
                        self.ui.notification_manager.error_with_message("Failed to rename thread", e.clone());
                        tracing::error!("ThreadRename: failed to rename - {}", e);
                    }
                }
                self.agent.thread_rename_buffer = None;
                self.ui.input_mode = InputMode::Normal;
            }

            _ => unreachable!("non-chat/llm action passed to dispatch_chat_llm: {:?}", action),
        }
        Ok(())
    }
}
