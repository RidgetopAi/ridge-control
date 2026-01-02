//! Ask User dialog component for structured user input
//!
//! Displays questions from the ask_user tool and captures user responses.
//! Supports single-select, multi-select, and custom text input.

use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::action::{Action, AskUserResponse};
use crate::config::Theme;
use crate::llm::ParsedQuestion;

/// Input mode for the dialog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    /// Selecting from options
    Selecting,
    /// Entering custom text for "Other"
    CustomText,
}

/// Ask User dialog component
pub struct AskUserDialog {
    /// Whether the dialog is currently visible
    visible: bool,
    /// Tool use ID to respond to
    tool_use_id: String,
    /// Questions to display
    questions: Vec<ParsedQuestion>,
    /// Current question index
    current_question: usize,
    /// Answers for each question (option labels or custom text)
    answers: Vec<Vec<String>>,
    /// Current selection index within options (includes "Other" at end)
    selected_option: usize,
    /// List state for option selection
    list_state: ListState,
    /// Current input mode
    input_mode: InputMode,
    /// Custom text input buffer
    custom_text: String,
}

impl AskUserDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            tool_use_id: String::new(),
            questions: Vec::new(),
            current_question: 0,
            answers: Vec::new(),
            selected_option: 0,
            list_state: ListState::default(),
            input_mode: InputMode::Selecting,
            custom_text: String::new(),
        }
    }

    /// Check if the dialog is currently visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show the dialog with questions
    pub fn show(&mut self, tool_use_id: String, questions: Vec<ParsedQuestion>) {
        self.visible = true;
        self.tool_use_id = tool_use_id;
        self.answers = vec![Vec::new(); questions.len()];
        self.questions = questions;
        self.current_question = 0;
        self.selected_option = 0;
        self.list_state.select(Some(0));
        self.input_mode = InputMode::Selecting;
        self.custom_text.clear();
    }

    /// Hide the dialog and clear state
    pub fn hide(&mut self) {
        self.visible = false;
        self.tool_use_id.clear();
        self.questions.clear();
        self.answers.clear();
        self.current_question = 0;
        self.selected_option = 0;
        self.list_state.select(None);
        self.input_mode = InputMode::Selecting;
        self.custom_text.clear();
    }

    /// Get current question if any
    fn current_question_data(&self) -> Option<&ParsedQuestion> {
        self.questions.get(self.current_question)
    }

    /// Total options including "Other"
    fn total_options(&self) -> usize {
        self.current_question_data()
            .map(|q| q.options.len() + 1) // +1 for "Other"
            .unwrap_or(0)
    }

    /// Check if current selection is on "Other"
    fn is_other_selected(&self) -> bool {
        self.current_question_data()
            .map(|q| self.selected_option >= q.options.len())
            .unwrap_or(false)
    }

    /// Handle input events
    pub fn handle_event(&mut self, event: &Event) -> Option<Action> {
        if !self.visible {
            return None;
        }

        if let Event::Key(key) = event {
            match self.input_mode {
                InputMode::Selecting => self.handle_selecting_input(key),
                InputMode::CustomText => self.handle_custom_text_input(key),
            }
        } else {
            None
        }
    }

    fn handle_selecting_input(&mut self, key: &crossterm::event::KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.hide();
                Some(Action::AskUserCancel)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_option > 0 {
                    self.selected_option -= 1;
                    self.list_state.select(Some(self.selected_option));
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_option < self.total_options().saturating_sub(1) {
                    self.selected_option += 1;
                    self.list_state.select(Some(self.selected_option));
                }
                None
            }
            KeyCode::Tab => {
                // Move to next question
                if self.current_question < self.questions.len() - 1 {
                    self.current_question += 1;
                    self.selected_option = 0;
                    self.list_state.select(Some(0));
                }
                None
            }
            KeyCode::BackTab => {
                // Move to previous question
                if self.current_question > 0 {
                    self.current_question -= 1;
                    self.selected_option = 0;
                    self.list_state.select(Some(0));
                }
                None
            }
            KeyCode::Char(' ') => {
                // Toggle selection for multi-select, or select for single
                if let Some(q) = self.current_question_data() {
                    if self.is_other_selected() {
                        // Start custom text input
                        self.input_mode = InputMode::CustomText;
                        self.custom_text.clear();
                    } else if q.multi_select {
                        // Toggle this option
                        let option_label = q.options[self.selected_option].label.clone();
                        let answers = &mut self.answers[self.current_question];
                        if let Some(pos) = answers.iter().position(|a| a == &option_label) {
                            answers.remove(pos);
                        } else {
                            answers.push(option_label);
                        }
                    } else {
                        // Single select - replace answer
                        let option_label = q.options[self.selected_option].label.clone();
                        self.answers[self.current_question] = vec![option_label];
                    }
                }
                None
            }
            KeyCode::Enter => {
                if self.is_other_selected() {
                    // Start custom text input
                    self.input_mode = InputMode::CustomText;
                    self.custom_text.clear();
                    None
                } else {
                    // Select option and possibly submit
                    if let Some(q) = self.current_question_data() {
                        if !q.multi_select {
                            // Single select - select this option
                            let option_label = q.options[self.selected_option].label.clone();
                            self.answers[self.current_question] = vec![option_label];
                        }
                    }

                    // Check if we can submit (all questions have answers)
                    if self.all_questions_answered() {
                        Some(self.build_response())
                    } else if self.current_question < self.questions.len() - 1 {
                        // Move to next unanswered question
                        self.current_question += 1;
                        self.selected_option = 0;
                        self.list_state.select(Some(0));
                        None
                    } else {
                        None
                    }
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+S to submit if all answered
                if self.all_questions_answered() {
                    Some(self.build_response())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn handle_custom_text_input(&mut self, key: &crossterm::event::KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                // Cancel custom input, go back to selecting
                self.input_mode = InputMode::Selecting;
                self.custom_text.clear();
                None
            }
            KeyCode::Enter => {
                // Submit custom text
                if !self.custom_text.is_empty() {
                    let text = format!("Other: {}", self.custom_text);
                    self.answers[self.current_question] = vec![text];
                    self.input_mode = InputMode::Selecting;
                    self.custom_text.clear();

                    // Check if we can submit
                    if self.all_questions_answered() {
                        Some(self.build_response())
                    } else if self.current_question < self.questions.len() - 1 {
                        self.current_question += 1;
                        self.selected_option = 0;
                        self.list_state.select(Some(0));
                        None
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            KeyCode::Backspace => {
                self.custom_text.pop();
                None
            }
            KeyCode::Char(c) => {
                self.custom_text.push(c);
                None
            }
            _ => None,
        }
    }

    fn all_questions_answered(&self) -> bool {
        self.answers.iter().all(|a| !a.is_empty())
    }

    fn build_response(&self) -> Action {
        // Join multi-select answers with ", "
        let answers: Vec<String> = self.answers
            .iter()
            .map(|a| a.join(", "))
            .collect();

        let response = AskUserResponse {
            tool_use_id: self.tool_use_id.clone(),
            answers,
        };

        self.hide_without_clear();
        Action::AskUserRespond(response)
    }

    /// Hide without clearing (for response building)
    fn hide_without_clear(&self) {
        // This is a bit awkward but we need to return the action
        // The actual hide will happen when the app processes AskUserRespond
    }

    /// Render the dialog
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        // Calculate dialog size (centered, 60% width, 70% height)
        let dialog_width = (area.width as f32 * 0.6) as u16;
        let dialog_height = (area.height as f32 * 0.7) as u16;
        let dialog_x = (area.width - dialog_width) / 2;
        let dialog_y = (area.height - dialog_height) / 2;
        let dialog_area = Rect::new(
            area.x + dialog_x,
            area.y + dialog_y,
            dialog_width,
            dialog_height,
        );

        // Clear the area
        frame.render_widget(Clear, dialog_area);

        // Main block
        let block = Block::default()
            .title(" Ask User ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.colors.primary.to_color()));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout: question tabs, question text, options, input area, hints
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Question tabs
                Constraint::Length(3), // Question text
                Constraint::Min(5),    // Options
                Constraint::Length(3), // Input area (for custom text)
                Constraint::Length(2), // Hints
            ])
            .split(inner);

        // Render question tabs
        self.render_question_tabs(frame, chunks[0], theme);

        // Render current question (clone to avoid borrow conflict)
        if let Some(q) = self.current_question_data().cloned() {
            // Question header and text
            let question_text = Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("[{}] ", q.header),
                    Style::default().fg(theme.colors.primary.to_color()).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::raw(&q.question)),
            ])
            .wrap(Wrap { trim: true });
            frame.render_widget(question_text, chunks[1]);

            // Options list
            self.render_options(frame, chunks[2], &q, theme);
        }

        // Custom text input area
        if self.input_mode == InputMode::CustomText {
            let input_block = Block::default()
                .title(" Enter custom response ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.colors.primary.to_color()));
            let input_text = Paragraph::new(self.custom_text.as_str())
                .block(input_block);
            frame.render_widget(input_text, chunks[3]);
        }

        // Hints
        let hints = if self.input_mode == InputMode::CustomText {
            "Enter: submit | Esc: cancel"
        } else if self.current_question_data().map(|q| q.multi_select).unwrap_or(false) {
            "Space: toggle | Enter: next/submit | Tab: next Q | Esc: cancel"
        } else {
            "Enter: select | Tab: next Q | Esc: cancel"
        };
        let hints_text = Paragraph::new(hints)
            .style(Style::default().fg(theme.colors.muted.to_color()))
            .alignment(Alignment::Center);
        frame.render_widget(hints_text, chunks[4]);
    }

    fn render_question_tabs(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut spans = Vec::new();
        for (i, q) in self.questions.iter().enumerate() {
            let answered = !self.answers[i].is_empty();
            let is_current = i == self.current_question;

            let style = if is_current {
                Style::default().fg(theme.colors.primary.to_color()).add_modifier(Modifier::BOLD)
            } else if answered {
                Style::default().fg(theme.colors.success.to_color())
            } else {
                Style::default().fg(theme.colors.muted.to_color())
            };

            let marker = if answered { "[x]" } else { "[ ]" };
            spans.push(Span::styled(format!(" {} {} ", marker, q.header), style));

            if i < self.questions.len() - 1 {
                spans.push(Span::raw(" | "));
            }
        }

        let tabs = Paragraph::new(Line::from(spans))
            .alignment(Alignment::Center);
        frame.render_widget(tabs, area);
    }

    fn render_options(&mut self, frame: &mut Frame, area: Rect, question: &ParsedQuestion, theme: &Theme) {
        let current_answers = &self.answers[self.current_question];

        let items: Vec<ListItem> = question.options
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let is_selected = current_answers.contains(&opt.label);
                let marker = if question.multi_select {
                    if is_selected { "[x]" } else { "[ ]" }
                } else {
                    if is_selected { "(*)" } else { "( )" }
                };

                let style = if i == self.selected_option {
                    Style::default().fg(theme.colors.primary.to_color()).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(theme.colors.success.to_color())
                } else {
                    Style::default().fg(theme.colors.foreground.to_color())
                };

                ListItem::new(vec![
                    Line::from(Span::styled(format!("{} {}", marker, opt.label), style)),
                    Line::from(Span::styled(
                        format!("    {}", opt.description),
                        Style::default().fg(theme.colors.muted.to_color()),
                    )),
                ])
            })
            .chain(std::iter::once({
                // "Other" option
                let is_other_selected = self.selected_option >= question.options.len();
                let has_custom = current_answers.iter().any(|a| a.starts_with("Other:"));
                let marker = if question.multi_select {
                    if has_custom { "[x]" } else { "[ ]" }
                } else {
                    if has_custom { "(*)" } else { "( )" }
                };

                let style = if is_other_selected {
                    Style::default().fg(theme.colors.primary.to_color()).add_modifier(Modifier::BOLD)
                } else if has_custom {
                    Style::default().fg(theme.colors.success.to_color())
                } else {
                    Style::default().fg(theme.colors.foreground.to_color())
                };

                ListItem::new(vec![
                    Line::from(Span::styled(format!("{} Other...", marker), style)),
                    Line::from(Span::styled(
                        "    Enter a custom response",
                        Style::default().fg(theme.colors.muted.to_color()),
                    )),
                ])
            }))
            .collect();

        let list = List::new(items)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }
}

impl Default for AskUserDialog {
    fn default() -> Self {
        Self::new()
    }
}
