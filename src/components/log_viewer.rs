use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::collections::VecDeque;

use crate::action::Action;
use crate::components::search::{SearchState, SearchBar, SearchAction, highlight_matches_in_line};
use crate::components::Component;
use crate::config::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            LogLevel::Trace => Color::DarkGray,
            LogLevel::Debug => Color::Cyan,
            LogLevel::Info => Color::Green,
            LogLevel::Warn => Color::Yellow,
            LogLevel::Error => Color::Red,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

impl LogEntry {
    pub fn new(level: LogLevel, target: impl Into<String>, message: impl Into<String>) -> Self {
        let now = chrono::Local::now();
        Self {
            timestamp: now.format("%H:%M:%S%.3f").to_string(),
            level,
            target: target.into(),
            message: message.into(),
        }
    }

    pub fn info(target: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(LogLevel::Info, target, message)
    }

    pub fn warn(target: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(LogLevel::Warn, target, message)
    }

    pub fn error(target: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(LogLevel::Error, target, message)
    }

    pub fn debug(target: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(LogLevel::Debug, target, message)
    }
}

pub struct LogViewer {
    logs: VecDeque<LogEntry>,
    max_entries: usize,
    scroll_offset: u16,
    visible_height: u16,
    inner_area: Rect,
    auto_scroll: bool,
    filter_level: Option<LogLevel>,
    search_state: SearchState,
}

impl LogViewer {
    pub fn new() -> Self {
        Self {
            logs: VecDeque::new(),
            max_entries: 10000,
            scroll_offset: 0,
            visible_height: 10,
            inner_area: Rect::default(),
            auto_scroll: true,
            filter_level: None,
            search_state: SearchState::new(),
        }
    }

    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    pub fn push(&mut self, entry: LogEntry) {
        self.logs.push_back(entry);
        
        while self.logs.len() > self.max_entries {
            self.logs.pop_front();
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }

        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn push_info(&mut self, target: impl Into<String>, message: impl Into<String>) {
        self.push(LogEntry::info(target, message));
    }

    pub fn push_warn(&mut self, target: impl Into<String>, message: impl Into<String>) {
        self.push(LogEntry::warn(target, message));
    }

    pub fn push_error(&mut self, target: impl Into<String>, message: impl Into<String>) {
        self.push(LogEntry::error(target, message));
    }

    pub fn push_debug(&mut self, target: impl Into<String>, message: impl Into<String>) {
        self.push(LogEntry::debug(target, message));
    }

    pub fn clear(&mut self) {
        self.logs.clear();
        self.scroll_offset = 0;
    }

    pub fn len(&self) -> usize {
        self.logs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.logs.is_empty()
    }

    pub fn is_auto_scroll(&self) -> bool {
        self.auto_scroll
    }

    pub fn set_auto_scroll(&mut self, enabled: bool) {
        self.auto_scroll = enabled;
        if enabled {
            self.scroll_to_bottom();
        }
    }

    pub fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = !self.auto_scroll;
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn set_filter_level(&mut self, level: Option<LogLevel>) {
        self.filter_level = level;
        self.scroll_offset = 0;
    }

    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = area;
        self.visible_height = area.height.saturating_sub(1);
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        if n > 0 {
            self.auto_scroll = false;
        }
    }

    pub fn scroll_down(&mut self, n: u16) {
        let filtered_count = self.filtered_entries().count();
        let max_scroll = filtered_count.saturating_sub(self.visible_height as usize) as u16;
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
        
        if self.scroll_offset >= max_scroll && max_scroll > 0 {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = false;
    }

    pub fn scroll_to_bottom(&mut self) {
        let filtered_count = self.filtered_entries().count();
        if filtered_count > self.visible_height as usize {
            self.scroll_offset = (filtered_count - self.visible_height as usize) as u16;
        } else {
            self.scroll_offset = 0;
        }
        self.auto_scroll = true;
    }

    pub fn scroll_page_up(&mut self) {
        self.scroll_up(self.visible_height.saturating_sub(2).max(1));
    }

    pub fn scroll_page_down(&mut self) {
        self.scroll_down(self.visible_height.saturating_sub(2).max(1));
    }

    pub fn is_search_active(&self) -> bool {
        self.search_state.is_active()
    }

    pub fn search_state(&self) -> &SearchState {
        &self.search_state
    }

    pub fn search_state_mut(&mut self) -> &mut SearchState {
        &mut self.search_state
    }

    pub fn start_search(&mut self) {
        self.search_state.activate();
        self.auto_scroll = false;
    }

    pub fn close_search(&mut self) {
        self.search_state.deactivate();
    }

    pub fn update_search(&mut self) {
        let lines: Vec<(usize, String)> = self
            .filtered_entries()
            .enumerate()
            .map(|(idx, entry)| (idx, entry.message.clone()))
            .collect();
        
        self.search_state.search_in_lines(
            lines.iter().map(|(idx, msg)| (*idx, msg.as_str()))
        );
    }

    pub fn search_next(&mut self) {
        self.search_state.next_match();
        self.scroll_to_current_match();
    }

    pub fn search_prev(&mut self) {
        self.search_state.prev_match();
        self.scroll_to_current_match();
    }

    fn scroll_to_current_match(&mut self) {
        if let Some(m) = self.search_state.current_match() {
            let target_line = m.line_index as u16;
            if target_line < self.scroll_offset {
                self.scroll_offset = target_line.saturating_sub(2);
            } else if target_line >= self.scroll_offset + self.visible_height {
                self.scroll_offset = target_line.saturating_sub(self.visible_height / 2);
            }
        }
    }

    fn filtered_entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.logs.iter().filter(|entry| {
            if let Some(ref level) = self.filter_level {
                self.level_includes(&entry.level, level)
            } else {
                true
            }
        })
    }

    fn level_includes(&self, entry_level: &LogLevel, min_level: &LogLevel) -> bool {
        let entry_ord = match entry_level {
            LogLevel::Trace => 0,
            LogLevel::Debug => 1,
            LogLevel::Info => 2,
            LogLevel::Warn => 3,
            LogLevel::Error => 4,
        };
        let min_ord = match min_level {
            LogLevel::Trace => 0,
            LogLevel::Debug => 1,
            LogLevel::Info => 2,
            LogLevel::Warn => 3,
            LogLevel::Error => 4,
        };
        entry_ord >= min_ord
    }

    fn render_themed(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let (log_area, search_area) = if self.search_state.is_active() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(SearchBar::height()),
                ])
                .split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };

        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let auto_scroll_indicator = if self.auto_scroll { "⏬" } else { "⏸" };
        let search_indicator = if self.search_state.is_active() { " 󰍉" } else { "" };
        let title = format!(
            " Logs ({}) {}{} ",
            self.filtered_entries().count(),
            auto_scroll_indicator,
            search_indicator
        );

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        if self.logs.is_empty() {
            let msg = Paragraph::new(Line::from(Span::styled(
                "No log entries",
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )))
            .block(block);
            frame.render_widget(msg, log_area);
            
            if let Some(search_rect) = search_area {
                let search_bar = SearchBar::new(&self.search_state, theme);
                search_bar.render(frame, search_rect);
            }
            return;
        }

        let match_style = Style::default()
            .fg(Color::Black)
            .bg(theme.colors.warning.to_color())
            .add_modifier(Modifier::BOLD);
        let current_match_style = Style::default()
            .fg(Color::Black)
            .bg(theme.colors.success.to_color())
            .add_modifier(Modifier::BOLD);
        let normal_style = Style::default().fg(theme.colors.foreground.to_color());

        let lines: Vec<Line> = self
            .filtered_entries()
            .enumerate()
            .skip(self.scroll_offset as usize)
            .take(self.visible_height as usize + 1)
            .map(|(line_idx, entry)| {
                let level_span = Span::styled(
                    format!("{:5}", entry.level.as_str()),
                    Style::default()
                        .fg(entry.level.color())
                        .add_modifier(Modifier::BOLD),
                );

                let timestamp_span = Span::styled(
                    format!("{} ", entry.timestamp),
                    Style::default().fg(theme.colors.muted.to_color()),
                );

                let target_span = Span::styled(
                    format!("[{}] ", entry.target),
                    Style::default()
                        .fg(theme.colors.accent.to_color())
                        .add_modifier(Modifier::DIM),
                );

                let message_spans = if self.search_state.is_active() && !self.search_state.query().is_empty() {
                    highlight_matches_in_line(
                        &entry.message,
                        line_idx,
                        self.search_state.matches(),
                        self.search_state.current_match_index(),
                        normal_style,
                        match_style,
                        current_match_style,
                    )
                } else {
                    vec![Span::styled(entry.message.clone(), normal_style)]
                };

                let mut spans = vec![timestamp_span, level_span, Span::raw(" "), target_span];
                spans.extend(message_spans);
                Line::from(spans)
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, log_area);

        if let Some(search_rect) = search_area {
            let search_bar = SearchBar::new(&self.search_state, theme);
            search_bar.render(frame, search_rect);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if self.search_state.is_active() {
            match self.search_state.handle_key(key) {
                SearchAction::Close => {
                    self.close_search();
                    return Some(Action::LogViewerSearchClose);
                }
                SearchAction::NavigateToMatch => {
                    self.scroll_to_current_match();
                    return None;
                }
                SearchAction::RefreshSearch => {
                    self.update_search();
                    self.scroll_to_current_match();
                    return None;
                }
                SearchAction::None => return None,
            }
        }

        match key.code {
            KeyCode::Char('/') => {
                self.start_search();
                Some(Action::LogViewerSearchStart)
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.search_state.match_count() > 0 {
                    self.search_next();
                    Some(Action::LogViewerSearchNext)
                } else {
                    None
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.search_state.match_count() > 0 {
                    self.search_prev();
                    Some(Action::LogViewerSearchPrev)
                } else {
                    None
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_down(1);
                Some(Action::LogViewerScrollDown(1))
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_up(1);
                Some(Action::LogViewerScrollUp(1))
            }
            KeyCode::Char('g') => {
                self.scroll_to_top();
                Some(Action::LogViewerScrollToTop)
            }
            KeyCode::Char('G') => {
                self.scroll_to_bottom();
                Some(Action::LogViewerScrollToBottom)
            }
            KeyCode::PageUp => {
                self.scroll_page_up();
                Some(Action::LogViewerScrollPageUp)
            }
            KeyCode::PageDown => {
                self.scroll_page_down();
                Some(Action::LogViewerScrollPageDown)
            }
            KeyCode::Char('a') => {
                self.toggle_auto_scroll();
                Some(Action::LogViewerToggleAutoScroll)
            }
            KeyCode::Char('c') => {
                self.clear();
                Some(Action::LogViewerClear)
            }
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::LogViewerHide),
            _ => None,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(3);
                Some(Action::LogViewerScrollUp(3))
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(3);
                Some(Action::LogViewerScrollDown(3))
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if mouse.row == 0 {
                    self.toggle_auto_scroll();
                    Some(Action::LogViewerToggleAutoScroll)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl Default for LogViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for LogViewer {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.handle_key(*key),
            Event::Mouse(mouse) => self.handle_mouse(*mouse),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::LogViewerScrollUp(n) => self.scroll_up(*n),
            Action::LogViewerScrollDown(n) => self.scroll_down(*n),
            Action::LogViewerScrollToTop => self.scroll_to_top(),
            Action::LogViewerScrollToBottom => self.scroll_to_bottom(),
            Action::LogViewerScrollPageUp => self.scroll_page_up(),
            Action::LogViewerScrollPageDown => self.scroll_page_down(),
            Action::LogViewerToggleAutoScroll => self.toggle_auto_scroll(),
            Action::LogViewerClear => self.clear(),
            Action::LogViewerSearchStart => self.start_search(),
            Action::LogViewerSearchClose => self.close_search(),
            Action::LogViewerSearchNext => self.search_next(),
            Action::LogViewerSearchPrev => self.search_prev(),
            Action::LogViewerSearchQuery(query) => {
                self.search_state.set_query(query.clone());
                self.update_search();
            }
            Action::LogViewerSearchToggleCase => {
                self.search_state.toggle_case_sensitivity();
                self.update_search();
            }
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        self.render_themed(frame, area, focused, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_viewer_new() {
        let viewer = LogViewer::new();
        assert!(viewer.is_empty());
        assert!(viewer.is_auto_scroll());
    }

    #[test]
    fn test_push_log_entry() {
        let mut viewer = LogViewer::new();
        viewer.push_info("test", "Hello world");
        assert_eq!(viewer.len(), 1);
    }

    #[test]
    fn test_max_entries_limit() {
        let mut viewer = LogViewer::new().with_max_entries(5);
        for i in 0..10 {
            viewer.push_info("test", format!("Message {}", i));
        }
        assert_eq!(viewer.len(), 5);
    }

    #[test]
    fn test_auto_scroll_toggle() {
        let mut viewer = LogViewer::new();
        assert!(viewer.is_auto_scroll());
        viewer.toggle_auto_scroll();
        assert!(!viewer.is_auto_scroll());
        viewer.toggle_auto_scroll();
        assert!(viewer.is_auto_scroll());
    }

    #[test]
    fn test_scroll_disables_auto_scroll() {
        let mut viewer = LogViewer::new();
        viewer.visible_height = 5;
        for i in 0..20 {
            viewer.push_info("test", format!("Message {}", i));
        }
        assert!(viewer.is_auto_scroll());
        viewer.scroll_up(1);
        assert!(!viewer.is_auto_scroll());
    }

    #[test]
    fn test_scroll_to_bottom_enables_auto_scroll() {
        let mut viewer = LogViewer::new();
        viewer.visible_height = 5;
        for i in 0..20 {
            viewer.push_info("test", format!("Message {}", i));
        }
        viewer.scroll_up(5);
        assert!(!viewer.is_auto_scroll());
        viewer.scroll_to_bottom();
        assert!(viewer.is_auto_scroll());
    }

    #[test]
    fn test_clear() {
        let mut viewer = LogViewer::new();
        viewer.push_info("test", "Message");
        viewer.push_error("test", "Error");
        assert_eq!(viewer.len(), 2);
        viewer.clear();
        assert!(viewer.is_empty());
    }

    #[test]
    fn test_log_levels() {
        assert_eq!(LogLevel::Info.as_str(), "INFO");
        assert_eq!(LogLevel::Warn.as_str(), "WARN");
        assert_eq!(LogLevel::Error.as_str(), "ERROR");
        assert_eq!(LogLevel::Debug.as_str(), "DEBUG");
        assert_eq!(LogLevel::Trace.as_str(), "TRACE");
    }

    #[test]
    fn test_log_level_colors() {
        assert_eq!(LogLevel::Info.color(), Color::Green);
        assert_eq!(LogLevel::Warn.color(), Color::Yellow);
        assert_eq!(LogLevel::Error.color(), Color::Red);
    }

    #[test]
    fn test_filter_level() {
        let mut viewer = LogViewer::new();
        viewer.push(LogEntry::new(LogLevel::Debug, "test", "debug msg"));
        viewer.push(LogEntry::new(LogLevel::Info, "test", "info msg"));
        viewer.push(LogEntry::new(LogLevel::Warn, "test", "warn msg"));
        viewer.push(LogEntry::new(LogLevel::Error, "test", "error msg"));
        
        assert_eq!(viewer.filtered_entries().count(), 4);
        
        viewer.set_filter_level(Some(LogLevel::Warn));
        assert_eq!(viewer.filtered_entries().count(), 2);
        
        viewer.set_filter_level(Some(LogLevel::Error));
        assert_eq!(viewer.filtered_entries().count(), 1);
        
        viewer.set_filter_level(None);
        assert_eq!(viewer.filtered_entries().count(), 4);
    }
}
