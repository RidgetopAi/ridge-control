use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use regex::Regex;

use crate::config::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub line_index: usize,
    pub start: usize,
    pub end: usize,
}

impl SearchMatch {
    pub fn new(line_index: usize, start: usize, end: usize) -> Self {
        Self { line_index, start, end }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    query: String,
    matches: Vec<SearchMatch>,
    current_match: usize,
    case_sensitive: bool,
    active: bool,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
    }

    pub fn clear_query(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
    }

    pub fn is_case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    pub fn toggle_case_sensitivity(&mut self) {
        self.case_sensitive = !self.case_sensitive;
    }

    pub fn set_case_sensitive(&mut self, sensitive: bool) {
        self.case_sensitive = sensitive;
    }

    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn current_match_index(&self) -> usize {
        self.current_match
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.matches.get(self.current_match)
    }

    pub fn set_matches(&mut self, matches: Vec<SearchMatch>) {
        self.matches = matches;
        if self.current_match >= self.matches.len() && !self.matches.is_empty() {
            self.current_match = 0;
        }
    }

    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = if self.current_match == 0 {
                self.matches.len() - 1
            } else {
                self.current_match - 1
            };
        }
    }

    pub fn search_in_lines<'a, I>(&mut self, lines: I)
    where
        I: Iterator<Item = (usize, &'a str)>,
    {
        if self.query.is_empty() {
            self.matches.clear();
            return;
        }

        let mut matches = Vec::new();
        let query = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };

        for (line_idx, line) in lines {
            let search_line = if self.case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };

            let mut start = 0;
            while let Some(pos) = search_line[start..].find(&query) {
                let absolute_start = start + pos;
                matches.push(SearchMatch::new(
                    line_idx,
                    absolute_start,
                    absolute_start + query.len(),
                ));
                start = absolute_start + 1;
            }
        }

        self.matches = matches;
        if self.current_match >= self.matches.len() && !self.matches.is_empty() {
            self.current_match = 0;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SearchAction {
        match key.code {
            KeyCode::Esc => {
                self.deactivate();
                SearchAction::Close
            }
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_match();
                } else {
                    self.next_match();
                }
                SearchAction::NavigateToMatch
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.next_match();
                SearchAction::NavigateToMatch
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.prev_match();
                SearchAction::NavigateToMatch
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_case_sensitivity();
                SearchAction::RefreshSearch
            }
            KeyCode::Backspace => {
                self.pop_char();
                SearchAction::RefreshSearch
            }
            KeyCode::Char(c) => {
                self.push_char(c);
                SearchAction::RefreshSearch
            }
            _ => SearchAction::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchAction {
    None,
    Close,
    NavigateToMatch,
    RefreshSearch,
}

pub struct SearchBar<'a> {
    search_state: &'a SearchState,
    theme: &'a Theme,
    focused: bool,
}

impl<'a> SearchBar<'a> {
    pub fn new(search_state: &'a SearchState, theme: &'a Theme) -> Self {
        Self {
            search_state,
            theme,
            focused: true,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            self.theme.colors.primary.to_color()
        } else {
            self.theme.colors.muted.to_color()
        };

        let case_indicator = if self.search_state.is_case_sensitive() {
            "[Aa]"
        } else {
            "[aa]"
        };

        let match_info = if self.search_state.query().is_empty() {
            String::new()
        } else if self.search_state.match_count() == 0 {
            " (no matches)".to_string()
        } else {
            format!(
                " ({}/{})",
                self.search_state.current_match_index() + 1,
                self.search_state.match_count()
            )
        };

        let title = format!(" Search {} {} ", case_indicator, match_info);

        let block = Block::default()
            .title(title)
            .title_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let query_with_cursor = format!("{}▌", self.search_state.query());

        let paragraph = Paragraph::new(Line::from(vec![
            Span::styled(
                "/",
                Style::default()
                    .fg(self.theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                query_with_cursor,
                Style::default().fg(self.theme.colors.foreground.to_color()),
            ),
        ]))
        .block(block);

        frame.render_widget(paragraph, area);
    }

    pub fn height() -> u16 {
        3
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Filter State and UI (TRC-022: Log Filtering/Grep Functionality)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct FilterState {
    pattern: String,
    case_sensitive: bool,
    use_regex: bool,
    inverted: bool,
    active: bool,
    compiled_regex: Option<Result<Regex, String>>,
}

impl FilterState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.pattern.clear();
        self.compiled_regex = None;
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn set_pattern(&mut self, pattern: String) {
        self.pattern = pattern;
        self.recompile_regex();
    }

    pub fn push_char(&mut self, c: char) {
        self.pattern.push(c);
        self.recompile_regex();
    }

    pub fn pop_char(&mut self) {
        self.pattern.pop();
        self.recompile_regex();
    }

    pub fn clear_pattern(&mut self) {
        self.pattern.clear();
        self.compiled_regex = None;
    }

    pub fn is_case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    pub fn set_case_sensitive(&mut self, sensitive: bool) {
        self.case_sensitive = sensitive;
        self.recompile_regex();
    }

    pub fn toggle_case_sensitivity(&mut self) {
        self.case_sensitive = !self.case_sensitive;
        self.recompile_regex();
    }

    pub fn is_regex(&self) -> bool {
        self.use_regex
    }

    pub fn set_regex(&mut self, use_regex: bool) {
        self.use_regex = use_regex;
        self.recompile_regex();
    }

    pub fn toggle_regex(&mut self) {
        self.use_regex = !self.use_regex;
        self.recompile_regex();
    }

    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    pub fn set_inverted(&mut self, inverted: bool) {
        self.inverted = inverted;
    }

    pub fn toggle_inverted(&mut self) {
        self.inverted = !self.inverted;
    }

    pub fn has_regex_error(&self) -> bool {
        matches!(&self.compiled_regex, Some(Err(_)))
    }

    pub fn regex_error(&self) -> Option<&str> {
        match &self.compiled_regex {
            Some(Err(e)) => Some(e.as_str()),
            _ => None,
        }
    }

    fn recompile_regex(&mut self) {
        if !self.use_regex || self.pattern.is_empty() {
            self.compiled_regex = None;
            return;
        }

        let pattern = if self.case_sensitive {
            self.pattern.clone()
        } else {
            format!("(?i){}", self.pattern)
        };

        self.compiled_regex = Some(
            Regex::new(&pattern).map_err(|e| e.to_string())
        );
    }

    pub fn matches_line(&self, line: &str) -> bool {
        if self.pattern.is_empty() {
            return true;
        }

        let matches = if self.use_regex {
            match &self.compiled_regex {
                Some(Ok(re)) => re.is_match(line),
                Some(Err(_)) => false,
                None => true,
            }
        } else if self.case_sensitive {
            line.contains(&self.pattern)
        } else {
            line.to_lowercase().contains(&self.pattern.to_lowercase())
        };

        if self.inverted {
            !matches
        } else {
            matches
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FilterAction {
        match key.code {
            KeyCode::Esc => {
                self.deactivate();
                FilterAction::Close
            }
            KeyCode::Enter => {
                self.active = false;
                FilterAction::Apply
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_case_sensitivity();
                FilterAction::Refresh
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_regex();
                FilterAction::Refresh
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_inverted();
                FilterAction::Refresh
            }
            KeyCode::Backspace => {
                self.pop_char();
                FilterAction::Refresh
            }
            KeyCode::Char(c) => {
                self.push_char(c);
                FilterAction::Refresh
            }
            _ => FilterAction::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterAction {
    None,
    Close,
    Apply,
    Refresh,
}

pub struct FilterBar<'a> {
    filter_state: &'a FilterState,
    theme: &'a Theme,
    focused: bool,
}

impl<'a> FilterBar<'a> {
    pub fn new(filter_state: &'a FilterState, theme: &'a Theme) -> Self {
        Self {
            filter_state,
            theme,
            focused: true,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            self.theme.colors.primary.to_color()
        } else {
            self.theme.colors.muted.to_color()
        };

        let case_indicator = if self.filter_state.is_case_sensitive() {
            "[Aa]"
        } else {
            "[aa]"
        };

        let regex_indicator = if self.filter_state.is_regex() {
            "[.*]"
        } else {
            "[lit]"
        };

        let invert_indicator = if self.filter_state.is_inverted() {
            "[!]"
        } else {
            ""
        };

        let error_indicator = if self.filter_state.has_regex_error() {
            " ⚠"
        } else {
            ""
        };

        let title = format!(
            " Filter {} {} {}{} ",
            case_indicator, regex_indicator, invert_indicator, error_indicator
        );

        let block = Block::default()
            .title(title)
            .title_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let pattern_with_cursor = format!("{}▌", self.filter_state.pattern());

        let paragraph = Paragraph::new(Line::from(vec![
            Span::styled(
                "grep:",
                Style::default()
                    .fg(self.theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                pattern_with_cursor,
                Style::default().fg(self.theme.colors.foreground.to_color()),
            ),
        ]))
        .block(block);

        frame.render_widget(paragraph, area);
    }

    pub fn height() -> u16 {
        3
    }
}

pub fn highlight_matches_in_line(
    line: &str,
    line_index: usize,
    matches: &[SearchMatch],
    current_match_index: usize,
    normal_style: Style,
    match_style: Style,
    current_match_style: Style,
) -> Vec<Span<'static>> {
    let line_matches: Vec<&SearchMatch> = matches
        .iter()
        .filter(|m| m.line_index == line_index)
        .collect();

    if line_matches.is_empty() {
        return vec![Span::styled(line.to_string(), normal_style)];
    }

    let mut spans = Vec::new();
    let mut last_end = 0;

    for m in &line_matches {
        if m.start > last_end {
            spans.push(Span::styled(
                line[last_end..m.start].to_string(),
                normal_style,
            ));
        }

        let is_current = matches
            .iter()
            .position(|sm| sm.line_index == m.line_index && sm.start == m.start)
            .map(|idx| idx == current_match_index)
            .unwrap_or(false);

        let style = if is_current {
            current_match_style
        } else {
            match_style
        };

        spans.push(Span::styled(line[m.start..m.end].to_string(), style));
        last_end = m.end;
    }

    if last_end < line.len() {
        spans.push(Span::styled(line[last_end..].to_string(), normal_style));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_state_new() {
        let state = SearchState::new();
        assert!(!state.is_active());
        assert!(state.query().is_empty());
        assert_eq!(state.match_count(), 0);
        assert!(!state.is_case_sensitive());
    }

    #[test]
    fn test_search_state_activate_deactivate() {
        let mut state = SearchState::new();
        state.activate();
        assert!(state.is_active());
        
        state.push_char('t');
        state.push_char('e');
        state.push_char('s');
        state.push_char('t');
        assert_eq!(state.query(), "test");
        
        state.deactivate();
        assert!(!state.is_active());
        assert!(state.query().is_empty());
    }

    #[test]
    fn test_search_in_lines() {
        let mut state = SearchState::new();
        state.set_query("error".to_string());
        
        let lines = vec![
            (0, "This is a test"),
            (1, "Error on line 2"),
            (2, "Another error here"),
            (3, "No match on this line"),
            (4, "Multiple error error matches"),
        ];
        
        state.search_in_lines(lines.into_iter());
        
        assert_eq!(state.match_count(), 4);
        assert_eq!(state.matches()[0].line_index, 1);
        assert_eq!(state.matches()[1].line_index, 2);
        assert_eq!(state.matches()[2].line_index, 4);
        assert_eq!(state.matches()[3].line_index, 4);
    }

    #[test]
    fn test_search_case_sensitive() {
        let mut state = SearchState::new();
        state.set_query("Error".to_string());
        state.set_case_sensitive(true);
        
        let lines = vec![
            (0, "error lowercase"),
            (1, "Error capitalized"),
            (2, "ERROR uppercase"),
        ];
        
        state.search_in_lines(lines.into_iter());
        assert_eq!(state.match_count(), 1);
        assert_eq!(state.matches()[0].line_index, 1);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut state = SearchState::new();
        state.set_query("error".to_string());
        state.set_case_sensitive(false);
        
        let lines = vec![
            (0, "error lowercase"),
            (1, "Error capitalized"),
            (2, "ERROR uppercase"),
        ];
        
        state.search_in_lines(lines.into_iter());
        assert_eq!(state.match_count(), 3);
    }

    #[test]
    fn test_search_navigation() {
        let mut state = SearchState::new();
        state.set_matches(vec![
            SearchMatch::new(0, 0, 5),
            SearchMatch::new(1, 0, 5),
            SearchMatch::new(2, 0, 5),
        ]);
        
        assert_eq!(state.current_match_index(), 0);
        
        state.next_match();
        assert_eq!(state.current_match_index(), 1);
        
        state.next_match();
        assert_eq!(state.current_match_index(), 2);
        
        state.next_match();
        assert_eq!(state.current_match_index(), 0);
        
        state.prev_match();
        assert_eq!(state.current_match_index(), 2);
    }

    #[test]
    fn test_search_empty_query() {
        let mut state = SearchState::new();
        state.set_query(String::new());
        
        let lines = vec![(0, "test"), (1, "lines")];
        state.search_in_lines(lines.into_iter());
        
        assert_eq!(state.match_count(), 0);
    }

    #[test]
    fn test_search_toggle_case() {
        let mut state = SearchState::new();
        assert!(!state.is_case_sensitive());
        
        state.toggle_case_sensitivity();
        assert!(state.is_case_sensitive());
        
        state.toggle_case_sensitivity();
        assert!(!state.is_case_sensitive());
    }

    #[test]
    fn test_search_push_pop_char() {
        let mut state = SearchState::new();
        
        state.push_char('a');
        state.push_char('b');
        state.push_char('c');
        assert_eq!(state.query(), "abc");
        
        state.pop_char();
        assert_eq!(state.query(), "ab");
        
        state.pop_char();
        state.pop_char();
        assert_eq!(state.query(), "");
        
        state.pop_char();
        assert_eq!(state.query(), "");
    }

    #[test]
    fn test_highlight_matches() {
        let matches = vec![
            SearchMatch::new(0, 5, 10),
        ];
        
        let spans = highlight_matches_in_line(
            "This ERROR here",
            0,
            &matches,
            0,
            Style::default(),
            Style::default().fg(Color::Yellow),
            Style::default().fg(Color::Green),
        );
        
        assert_eq!(spans.len(), 3);
    }

    #[test]
    fn test_search_match_struct() {
        let m = SearchMatch::new(5, 10, 15);
        assert_eq!(m.line_index, 5);
        assert_eq!(m.start, 10);
        assert_eq!(m.end, 15);
    }

    #[test]
    fn test_filter_state_new() {
        let state = FilterState::new();
        assert!(!state.is_active());
        assert!(state.pattern().is_empty());
        assert!(!state.is_regex());
        assert!(!state.is_inverted());
    }

    #[test]
    fn test_filter_state_activate_deactivate() {
        let mut state = FilterState::new();
        state.activate();
        assert!(state.is_active());
        
        state.push_char('t');
        state.push_char('e');
        state.push_char('s');
        state.push_char('t');
        assert_eq!(state.pattern(), "test");
        
        state.deactivate();
        assert!(!state.is_active());
        assert!(state.pattern().is_empty());
    }

    #[test]
    fn test_filter_matches_line_literal() {
        let mut state = FilterState::new();
        state.set_pattern("error".to_string());
        
        assert!(state.matches_line("This has an error"));
        assert!(state.matches_line("ERROR uppercase"));
        assert!(!state.matches_line("This is clean"));
    }

    #[test]
    fn test_filter_matches_line_case_sensitive() {
        let mut state = FilterState::new();
        state.set_pattern("Error".to_string());
        state.set_case_sensitive(true);
        
        assert!(!state.matches_line("error lowercase"));
        assert!(state.matches_line("Error capitalized"));
        assert!(!state.matches_line("ERROR uppercase"));
    }

    #[test]
    fn test_filter_matches_line_regex() {
        let mut state = FilterState::new();
        state.set_pattern(r"error|warn".to_string());
        state.set_regex(true);
        
        assert!(state.matches_line("This has an error"));
        assert!(state.matches_line("This is a warning"));
        assert!(!state.matches_line("This is info"));
    }

    #[test]
    fn test_filter_matches_line_inverted() {
        let mut state = FilterState::new();
        state.set_pattern("error".to_string());
        state.set_inverted(true);
        
        assert!(!state.matches_line("This has an error"));
        assert!(state.matches_line("This is clean"));
    }

    #[test]
    fn test_filter_toggle_options() {
        let mut state = FilterState::new();
        
        assert!(!state.is_case_sensitive());
        state.toggle_case_sensitivity();
        assert!(state.is_case_sensitive());
        
        assert!(!state.is_regex());
        state.toggle_regex();
        assert!(state.is_regex());
        
        assert!(!state.is_inverted());
        state.toggle_inverted();
        assert!(state.is_inverted());
    }

    #[test]
    fn test_filter_invalid_regex_fallback() {
        let mut state = FilterState::new();
        state.set_pattern(r"[invalid".to_string());
        state.set_regex(true);
        
        assert!(!state.matches_line("[invalid pattern"));
        assert!(state.has_regex_error());
    }

    #[test]
    fn test_filter_empty_pattern() {
        let state = FilterState::new();
        assert!(state.matches_line("anything"));
        assert!(state.matches_line(""));
    }
}
