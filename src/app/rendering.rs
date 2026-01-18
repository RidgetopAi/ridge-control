// Rendering - draw() method and UI layout helpers
// Extracted as part of REFACTOR-P5.3

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use super::App;
use crate::agent::ContextStats;
use crate::components::Component;
use crate::config::Theme;
use crate::error::{Result, RidgeError};
use crate::input::focus::FocusArea;
use crate::llm::Message;
use crate::tabs::TabBar;

impl App {
    /// Main drawing method - renders entire UI
    pub(super) fn draw(&mut self) -> Result<()> {
        // Pre-compute focus booleans to avoid cloning FocusManager
        let focus_terminal = self.ui.focus.is_focused(FocusArea::Terminal);
        let focus_stream_viewer = self.ui.focus.is_focused(FocusArea::StreamViewer);
        let focus_chat_input = self.ui.focus.is_focused(FocusArea::ChatInput);
        let focus_process_monitor = self.ui.focus.is_focused(FocusArea::ProcessMonitor);
        let focus_menu = self.ui.focus.is_focused(FocusArea::Menu);
        let focus_log_viewer = self.ui.focus.is_focused(FocusArea::LogViewer);
        let focus_config_panel = self.ui.focus.is_focused(FocusArea::ConfigPanel);
        let focus_settings_editor = self.ui.focus.is_focused(FocusArea::SettingsEditor);
        
        let streams: Vec<_> = self.stream_manager.clients().to_vec();
        let show_confirm = self.ui.confirm_dialog.is_visible();
        let show_palette = self.ui.command_palette.is_visible();
        let show_thread_picker = self.agent.thread_picker.is_visible();
        let show_thread_rename = self.agent.thread_rename_buffer.is_some();
        let thread_rename_text = self.agent.thread_rename_buffer.clone().unwrap_or_default();
        let show_ask_user = self.ui.ask_user_dialog.is_visible();
        let show_context_menu = self.ui.context_menu.is_visible();
        let has_notifications = self.ui.notification_manager.has_notifications();
        let _show_tabs = self.pty.tab_manager.count() > 1; // Kept for potential future use
        let show_conversation = self.agent.show_conversation || !self.agent.llm_response_buffer.is_empty() || !self.agent.thinking_buffer.is_empty();
        let show_stream_viewer = self.show_stream_viewer;
        let show_log_viewer = self.show_log_viewer;
        let show_config_panel = self.show_config_panel;
        let show_settings_editor = self.show_settings_editor;
        let show_sirk_panel = self.ui.sirk_panel_visible;
        let show_activity_stream = self.ui.activity_stream_visible;
        let selected_stream_idx = self.selected_stream_index;
        // Clone theme once - it's small (just color values)
        let theme = self.config_manager.theme().clone();
        // TP2-002-14: Get messages from AgentThread segments if available
        let messages: Vec<Message> = if let Some(thread) = self.agent.agent_engine.current_thread() {
            // Extract all messages from thread segments
            thread.segments().iter()
                .flat_map(|segment| segment.messages.clone())
                .collect()
        } else {
            Vec::new()
        };
        let streaming_buffer = self.agent.llm_response_buffer.clone();
        // TRC-017: Clone thinking buffer for rendering
        let thinking_buffer = self.agent.thinking_buffer.clone();
        
        // Get active tab's PTY session for rendering (TRC-005)
        let active_tab_id = self.pty.tab_manager.active_tab().id();
        
        // Pre-calculate tab bar area for mouse hit-testing (TRC-010)
        let term_size = self.pty.terminal.size().unwrap_or_default();
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
        self.ui.tab_bar_area = computed_tab_bar_area;
        // TRC-024: Store content area for pane resize mouse hit-testing
        self.ui.content_area = computed_content_area;

        self.pty.terminal
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
                    let tab_bar = TabBar::from_manager_themed(&self.pty.tab_manager, &theme)
                        .dangerous_mode(self.agent.dangerous_mode)
                        .input_mode(self.ui.input_mode.clone());
                    frame.render_widget(tab_bar, tab_bar_area);
                }

                // TRC-024: Store content area for mouse hit-testing
                // Main layout: left (terminal or terminal+conversation) and right (process monitor + menu)
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.ui.pane_layout.main_constraints())
                    .split(content_area);

                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(self.ui.pane_layout.right_constraints())
                    .split(main_chunks[1]);

                // Left area: split between terminal and conversation if conversation is visible
                // Use active tab's terminal widget (TRC-005)
                if show_conversation {
                    let left_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(self.ui.pane_layout.left_constraints())
                        .split(main_chunks[0]);

                    // Save terminal area for mouse coordinate translation
                    self.ui.terminal_area = left_chunks[0];
                    
                    if let Some(session) = self.pty.tab_manager.get_pty_session(active_tab_id) {
                        session.terminal().render(
                            frame,
                            left_chunks[0],
                            focus_terminal,
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
                        let provider = self.agent.agent_engine.current_provider();
                        let model = self.agent.agent_engine.current_model();
                        if provider.is_empty() || model.is_empty() {
                            None
                        } else {
                            Some((provider, model))
                        }
                    };

                    // Phase 3: Compute context stats for header display (with caching)
                    let context_stats = {
                        let model = self.agent.agent_engine.current_model();
                        if model.is_empty() || messages.is_empty() {
                            None
                        } else {
                            let model_info = self.agent.model_catalog.info_for(model);
                            // Use cached token count if message count hasn't changed
                            let tokens_used = match self.agent.cached_token_count {
                                Some((count, tokens)) if count == messages.len() => tokens,
                                _ => {
                                    let tokens = self.agent.token_counter.count_messages(model, &messages);
                                    self.agent.cached_token_count = Some((messages.len(), tokens));
                                    tokens
                                }
                            };
                            // Budget = context window - default output tokens - 2% safety
                            let safety = model_info.max_context_tokens / 50; // 2%
                            let budget = model_info.max_context_tokens
                                .saturating_sub(model_info.default_max_output_tokens)
                                .saturating_sub(safety);
                            Some(ContextStats::new(tokens_used, budget, false, messages.len()))
                        }
                    };

                    self.agent.conversation_viewer.render_conversation(
                        frame,
                        conv_chunks[0],
                        focus_stream_viewer, // Conversation history focus
                        &messages,
                        &streaming_buffer,
                        &thinking_buffer,
                        &theme,
                        model_info,
                        context_stats.as_ref(),
                    );

                    // Render chat input at bottom of conversation area
                    self.agent.chat_input.render(
                        frame,
                        conv_chunks[1],
                        focus_chat_input,
                        &theme,
                    );

                    let term_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(left_chunks[0])
                    };
                    if let Some(session) = self.pty.tab_manager.get_pty_session_mut(active_tab_id) {
                        session.terminal_mut().set_inner_area(term_inner);
                    }

                    let conv_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(conv_chunks[0])
                    };
                    self.agent.conversation_viewer.set_inner_area(conv_inner);

                    // Set inner area for chat input mouse coordinate conversion
                    let chat_input_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(conv_chunks[1])
                    };
                    self.agent.chat_input.set_inner_area(chat_input_inner);

                    // Save conversation area for mouse hit-testing
                    self.ui.conversation_area = conv_chunks[0];
                    // Save chat input area for mouse hit-testing (paste routing and selection)
                    self.ui.chat_input_area = conv_chunks[1];
                } else {
                    // Clear conversation and chat input areas when not visible
                    self.ui.conversation_area = Rect::default();
                    self.ui.chat_input_area = Rect::default();
                    // Save terminal area for mouse coordinate translation
                    self.ui.terminal_area = main_chunks[0];
                    
                    if let Some(session) = self.pty.tab_manager.get_pty_session(active_tab_id) {
                        session.terminal().render(
                            frame,
                            main_chunks[0],
                            focus_terminal,
                            &theme,
                        );
                    }

                    let term_inner = {
                        let block = ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL);
                        block.inner(main_chunks[0])
                    };
                    if let Some(session) = self.pty.tab_manager.get_pty_session_mut(active_tab_id) {
                        session.terminal_mut().set_inner_area(term_inner);
                    }
                }

                self.process_monitor.render(
                    frame,
                    right_chunks[0],
                    focus_process_monitor,
                    &theme,
                );
                self.ui.menu.render_with_streams(
                    frame,
                    right_chunks[1],
                    focus_menu,
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
                self.ui.menu.set_inner_area(menu_inner);
                
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
                        focus_stream_viewer,
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
                        focus_log_viewer,
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
                        focus_config_panel,
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
                        focus_settings_editor,
                        &theme,
                    );
                }

                // SIRK Panel overlay (Forge control) - centered modal dialog
                if show_sirk_panel {
                    if let Some(ref sirk_panel) = self.sirk_panel {
                        // Calculate centered dialog size (50% width, fixed height for form)
                        let dialog_width = (size.width * 50 / 100).clamp(45, 70);
                        let dialog_height = 14u16; // Fixed height for form fields
                        let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
                        let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;
                        let sirk_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

                        // Clear the area behind for readability
                        frame.render_widget(ratatui::widgets::Clear, sirk_area);

                        sirk_panel.render(
                            frame,
                            sirk_area,
                            true, // Always focused when visible
                            &theme,
                        );
                    }
                }

                // Activity Stream overlay (Forge real-time activity) - right side panel
                if show_activity_stream {
                    if let Some(ref activity_stream) = self.activity_stream {
                        // Calculate right-side panel (40% width, full height minus status bar)
                        let panel_width = (size.width * 40 / 100).clamp(40, 80);
                        let panel_height = size.height.saturating_sub(2); // Leave room for status bar
                        let panel_x = size.width.saturating_sub(panel_width);
                        let panel_y = 1u16; // Below status bar
                        let activity_area = Rect::new(panel_x, panel_y, panel_width, panel_height);

                        // Clear the area behind for readability
                        frame.render_widget(ratatui::widgets::Clear, activity_area);

                        activity_stream.render(
                            frame,
                            activity_area,
                            true, // Always focused when visible
                            &theme,
                        );
                    }
                }

                if show_confirm {
                    self.ui.confirm_dialog.render(frame, size, &theme);
                }
                
                if show_palette {
                    self.ui.command_palette.render(frame, size, &theme);
                }

                // P2-003: Thread picker overlay
                if show_thread_picker {
                    self.agent.thread_picker.render(frame, size, &theme);
                }

                // P2-003: Thread rename dialog overlay
                if show_thread_rename {
                    Self::render_thread_rename_dialog(frame, size, &theme, &thread_rename_text);
                }

                // T2.4: Ask user dialog overlay
                if show_ask_user {
                    self.ui.ask_user_dialog.render(frame, size, &theme);
                }

                // TRC-020: Context menu overlay (highest z-index)
                if show_context_menu {
                    self.ui.context_menu.render(frame, size, &theme);
                }
                
                // TRC-023: Notifications overlay (top-right, highest z-index)
                if has_notifications {
                    self.ui.notification_manager.render(frame, size, &theme);
                }
            })
            .map_err(|e| RidgeError::Terminal(e.to_string()))?;

        Ok(())
    }

    /// Render the thread rename dialog overlay
    fn render_thread_rename_dialog(
        frame: &mut ratatui::Frame,
        size: Rect,
        theme: &Theme,
        thread_rename_text: &str,
    ) {
        use ratatui::widgets::{Block, Borders, Clear, Paragraph};
        use ratatui::text::{Line, Span};
        use ratatui::style::{Modifier, Style};
        use ratatui::layout::Alignment;

        // Calculate dialog size (centered, fixed width)
        let dialog_width = 50u16.min(size.width.saturating_sub(4));
        let dialog_height = 5u16;
        let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

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
            Span::styled(thread_rename_text, Style::default().fg(theme.command_palette.input_fg.to_color())),
            Span::styled("▎", Style::default().fg(theme.colors.primary.to_color())),
        ]);
        frame.render_widget(Paragraph::new(input_line), inner);

        // Help text below
        let help_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
        let help_text = Paragraph::new("Enter to confirm, Esc to cancel")
            .style(Style::default().fg(theme.command_palette.description_fg.to_color()))
            .alignment(Alignment::Center);
        frame.render_widget(help_text, help_area);
    }
}
