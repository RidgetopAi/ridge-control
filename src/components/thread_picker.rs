//! Thread picker component for listing and resuming previous conversations
//!
//! Displays saved threads with fuzzy search, allowing users to continue
//! previous conversations from the command palette.

use crossterm::event::{Event, KeyCode, KeyModifiers};
use nucleo::{Config, Matcher, Utf32String};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::action::Action;
use crate::agent::thread::ThreadSummary;
use crate::config::Theme;

/// Fuzzy matcher result with score and indices
struct MatchResult {
    thread_idx: usize,
    score: u32,
    indices: Vec<u32>,
}

/// Thread picker component for selecting from saved threads
pub struct ThreadPicker {
    /// Whether the picker is currently visible
    visible: bool,
    /// Current search query
    query: String,
    /// List of thread summaries to display
    threads: Vec<ThreadSummary>,
    /// Nucleo fuzzy matcher
    matcher: Matcher,
    /// Filtered and scored results
    filtered_results: Vec<MatchResult>,
    /// List selection state
    list_state: ListState,
}

impl ThreadPicker {
    pub fn new() -> Self {
        let config = Config::DEFAULT;
        Self {
            visible: false,
            query: String::new(),
            threads: Vec::new(),
            matcher: Matcher::new(config),
            filtered_results: Vec::new(),
            list_state: ListState::default(),
        }
    }

    /// Check if the picker is currently visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show the picker with the given thread summaries
    pub fn show(&mut self, threads: Vec<ThreadSummary>) {
        self.visible = true;
        self.query.clear();
        self.threads = threads;
        self.update_filtered_results();
        // Select first item if available
        if !self.filtered_results.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    /// Hide the picker and clear state
    pub fn hide(&mut self) {
        self.visible = false;
        self.query.clear();
        self.threads.clear();
        self.filtered_results.clear();
        self.list_state.select(None);
    }

    /// Get the current search query
    #[allow(dead_code)]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Update filtered results based on current query
    fn update_filtered_results(&mut self) {
        self.filtered_results.clear();

        if self.query.is_empty() {
            // Show all threads when no query (already sorted by updated_at)
            for (idx, _) in self.threads.iter().enumerate() {
                self.filtered_results.push(MatchResult {
                    thread_idx: idx,
                    score: 0,
                    indices: Vec::new(),
                });
            }
        } else {
            // Fuzzy match against query
            let pattern = nucleo::pattern::Pattern::parse(
                &self.query,
                nucleo::pattern::CaseMatching::Smart,
                nucleo::pattern::Normalization::Smart,
            );

            for (idx, thread) in self.threads.iter().enumerate() {
                // Match against title and model
                let title_utf32: Utf32String = thread.title.as_str().into();
                let model_utf32: Utf32String = thread.model.as_str().into();

                let mut indices = Vec::new();
                let title_score = pattern.indices(
                    title_utf32.slice(..),
                    &mut self.matcher,
                    &mut indices,
                );

                // Also check model if title didn't match
                let model_score = if title_score.is_none() {
                    let mut model_indices = Vec::new();
                    pattern.indices(
                        model_utf32.slice(..),
                        &mut self.matcher,
                        &mut model_indices,
                    )
                } else {
                    None
                };

                // Use the best score
                let final_score = title_score.or(model_score);
                if let Some(score) = final_score {
                    self.filtered_results.push(MatchResult {
                        thread_idx: idx,
                        score,
                        indices,
                    });
                }
            }

            // Sort by score (higher is better)
            self.filtered_results.sort_by(|a, b| b.score.cmp(&a.score));
        }

        // Reset selection to first item
        if !self.filtered_results.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    /// Select the next item in the list
    fn select_next(&mut self) {
        if self.filtered_results.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = (current + 1) % self.filtered_results.len();
        self.list_state.select(Some(next));
    }

    /// Select the previous item in the list
    fn select_prev(&mut self) {
        if self.filtered_results.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let prev = if current == 0 {
            self.filtered_results.len() - 1
        } else {
            current - 1
        };
        self.list_state.select(Some(prev));
    }

    /// Execute selection - returns the thread ID to load
    fn execute_selected(&mut self) -> Option<String> {
        let selected_idx = self.list_state.selected()?;
        let result = self.filtered_results.get(selected_idx)?;
        let thread = self.threads.get(result.thread_idx)?;
        let thread_id = thread.id.clone();
        self.hide();
        Some(thread_id)
    }

    /// Handle keyboard events, returns Action if event was consumed
    pub fn handle_event(&mut self, event: &Event) -> Option<Action> {
        if !self.visible {
            return None;
        }

        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Esc => {
                    self.hide();
                    return Some(Action::ThreadPickerHide);
                }
                KeyCode::Enter => {
                    if let Some(thread_id) = self.execute_selected() {
                        return Some(Action::ThreadLoad(thread_id));
                    }
                }
                KeyCode::Up | KeyCode::BackTab => {
                    self.select_prev();
                }
                KeyCode::Down | KeyCode::Tab => {
                    self.select_next();
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.select_next();
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.select_prev();
                }
                KeyCode::Char(c) => {
                    self.query.push(c);
                    self.update_filtered_results();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.update_filtered_results();
                }
                _ => {}
            }
        }

        None
    }

    /// Render the thread picker dialog
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        // Calculate dialog size (centered, 60% width, 50% height)
        let dialog_width = (area.width * 60 / 100).clamp(50, 100);
        let dialog_height = (area.height * 50 / 100).clamp(10, 30);

        let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

        // Clear the area behind
        frame.render_widget(Clear, dialog_area);

        // Main block
        let block = Block::default()
            .title(" Continue Thread ")
            .title_style(Style::default().fg(theme.command_palette.border.to_color()).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.command_palette.border.to_color()));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split inner area: input line at top, info, results below
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Input
                Constraint::Length(1), // Separator/info
                Constraint::Min(1),    // Results
            ])
            .split(inner);

        // Input line with prompt
        let input_line = Line::from(vec![
            Span::styled(": ", Style::default().fg(theme.colors.primary.to_color()).add_modifier(Modifier::BOLD)),
            Span::styled(&self.query, Style::default().fg(theme.command_palette.input_fg.to_color())),
            Span::styled("▎", Style::default().fg(theme.colors.primary.to_color())), // Cursor
        ]);
        frame.render_widget(Paragraph::new(input_line), chunks[0]);

        // Info line
        let count = self.filtered_results.len();
        let total = self.threads.len();
        let info = if self.query.is_empty() {
            format!("{} threads", total)
        } else {
            format!("{}/{} matching", count, total)
        };
        let info_line = Paragraph::new(info)
            .style(Style::default().fg(theme.command_palette.description_fg.to_color()))
            .alignment(Alignment::Right);
        frame.render_widget(info_line, chunks[1]);

        // Results list
        let items: Vec<ListItem> = self
            .filtered_results
            .iter()
            .map(|result| {
                let thread = &self.threads[result.thread_idx];
                self.render_thread_item(thread, &result.indices, theme)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(theme.command_palette.selected_bg.to_color())
                    .fg(theme.command_palette.selected_fg.to_color())
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        // Clone list_state for rendering (ratatui requires &mut for StatefulWidget)
        let mut list_state = self.list_state.clone();
        frame.render_stateful_widget(list, chunks[2], &mut list_state);
    }

    /// Render a single thread item
    fn render_thread_item(&self, thread: &ThreadSummary, indices: &[u32], theme: &Theme) -> ListItem<'static> {
        let mut spans = Vec::new();

        // Thread icon
        spans.push(Span::styled(
            "📝 ",
            Style::default(),
        ));

        // Highlight matched characters in title
        if indices.is_empty() {
            spans.push(Span::styled(
                thread.title.clone(),
                Style::default().fg(theme.command_palette.item_fg.to_color()),
            ));
        } else {
            let chars: Vec<char> = thread.title.chars().collect();
            let indices_set: std::collections::HashSet<u32> = indices.iter().copied().collect();

            for (i, ch) in chars.iter().enumerate() {
                let style = if indices_set.contains(&(i as u32)) {
                    Style::default().fg(theme.command_palette.match_highlight.to_color()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.command_palette.item_fg.to_color())
                };
                spans.push(Span::styled(ch.to_string(), style));
            }
        }

        // Model info
        spans.push(Span::styled(
            format!(" - {}", Self::abbreviate_model(&thread.model)),
            Style::default().fg(theme.command_palette.description_fg.to_color()),
        ));

        ListItem::new(Line::from(spans))
    }

    /// Abbreviate model name for display
    fn abbreviate_model(model: &str) -> String {
        // Common abbreviations
        if model.contains("claude") {
            if model.contains("opus") {
                return "claude-opus".to_string();
            } else if model.contains("sonnet") {
                return "claude-sonnet".to_string();
            } else if model.contains("haiku") {
                return "claude-haiku".to_string();
            }
            return "claude".to_string();
        }
        if model.contains("gpt-4o") {
            return "gpt-4o".to_string();
        }
        if model.contains("gpt-4") {
            return "gpt-4".to_string();
        }
        if model.contains("gemini") {
            return "gemini".to_string();
        }
        if model.contains("grok") {
            return "grok".to_string();
        }
        // Fallback: truncate if too long
        if model.len() > 20 {
            format!("{}...", &model[..17])
        } else {
            model.to_string()
        }
    }
}

impl Default for ThreadPicker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_threads() -> Vec<ThreadSummary> {
        vec![
            ThreadSummary {
                id: "T-001".to_string(),
                title: "Debug ANSI stripping issue".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                updated_at: Utc::now(),
                segment_count: 12,
            },
            ThreadSummary {
                id: "T-002".to_string(),
                title: "Implement settings menu".to_string(),
                model: "gpt-4o".to_string(),
                updated_at: Utc::now(),
                segment_count: 45,
            },
            ThreadSummary {
                id: "T-003".to_string(),
                title: "API review".to_string(),
                model: "claude-opus-4-20250514".to_string(),
                updated_at: Utc::now(),
                segment_count: 8,
            },
        ]
    }

    #[test]
    fn test_thread_picker_visibility() {
        let mut picker = ThreadPicker::new();
        assert!(!picker.is_visible());

        picker.show(create_test_threads());
        assert!(picker.is_visible());

        picker.hide();
        assert!(!picker.is_visible());
    }

    #[test]
    fn test_thread_picker_show_populates() {
        let mut picker = ThreadPicker::new();
        let threads = create_test_threads();

        picker.show(threads.clone());

        assert_eq!(picker.threads.len(), 3);
        assert_eq!(picker.filtered_results.len(), 3);
        assert_eq!(picker.list_state.selected(), Some(0));
    }

    #[test]
    fn test_fuzzy_filtering() {
        let mut picker = ThreadPicker::new();
        picker.show(create_test_threads());

        // Initially shows all threads
        assert_eq!(picker.filtered_results.len(), 3);

        // Filter with "debug" should narrow results
        picker.query = "debug".to_string();
        picker.update_filtered_results();

        // Should match "Debug ANSI stripping issue"
        assert!(!picker.filtered_results.is_empty());
        let first_result = &picker.filtered_results[0];
        assert_eq!(picker.threads[first_result.thread_idx].id, "T-001");
    }

    #[test]
    fn test_selection_navigation() {
        let mut picker = ThreadPicker::new();
        picker.show(create_test_threads());

        assert_eq!(picker.list_state.selected(), Some(0));

        picker.select_next();
        assert_eq!(picker.list_state.selected(), Some(1));

        picker.select_prev();
        assert_eq!(picker.list_state.selected(), Some(0));

        // Test wrap around
        picker.select_prev();
        assert_eq!(picker.list_state.selected(), Some(2));
    }

    #[test]
    fn test_execute_selected() {
        let mut picker = ThreadPicker::new();
        picker.show(create_test_threads());

        // Select second item
        picker.select_next();

        let thread_id = picker.execute_selected();
        assert_eq!(thread_id, Some("T-002".to_string()));
        assert!(!picker.is_visible()); // Should hide after selection
    }

    #[test]
    fn test_abbreviate_model() {
        assert_eq!(ThreadPicker::abbreviate_model("claude-sonnet-4-20250514"), "claude-sonnet");
        assert_eq!(ThreadPicker::abbreviate_model("claude-opus-4-20250514"), "claude-opus");
        assert_eq!(ThreadPicker::abbreviate_model("gpt-4o-2024-08-06"), "gpt-4o");
        assert_eq!(ThreadPicker::abbreviate_model("gemini-1.5-pro"), "gemini");
        assert_eq!(ThreadPicker::abbreviate_model("short"), "short");
    }

    #[test]
    fn test_hide_clears_state() {
        let mut picker = ThreadPicker::new();
        picker.show(create_test_threads());
        picker.query = "test".to_string();

        picker.hide();

        assert!(picker.query.is_empty());
        assert!(picker.threads.is_empty());
        assert!(picker.filtered_results.is_empty());
        assert_eq!(picker.list_state.selected(), None);
    }
}
