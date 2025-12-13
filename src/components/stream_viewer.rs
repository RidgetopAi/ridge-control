use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::action::Action;
use crate::components::search::{SearchState, SearchBar, SearchAction, highlight_matches_in_line};
use crate::components::Component;
use crate::config::Theme;
use crate::streams::{StreamClient, StreamData};

pub struct StreamViewer {
    scroll_offset: u16,
    line_count: usize,
    visible_height: u16,
    selected_stream_name: String,
    search_state: SearchState,
    cached_lines: Vec<String>,
}

impl StreamViewer {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            line_count: 0,
            visible_height: 10,
            selected_stream_name: String::new(),
            search_state: SearchState::new(),
            cached_lines: Vec::new(),
        }
    }

    pub fn set_visible_height(&mut self, height: u16) {
        self.visible_height = height.saturating_sub(2);
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.line_count > self.visible_height as usize {
            self.scroll_offset = (self.line_count - self.visible_height as usize) as u16;
        }
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
    }

    pub fn close_search(&mut self) {
        self.search_state.deactivate();
    }

    pub fn update_search(&mut self) {
        self.search_state.search_in_lines(
            self.cached_lines.iter().enumerate().map(|(idx, line)| (idx, line.as_str()))
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

    fn update_cached_lines(&mut self, stream: Option<&StreamClient>) {
        self.cached_lines.clear();
        if let Some(stream) = stream {
            for data in stream.buffer().iter() {
                match data {
                    StreamData::Text(text) => {
                        for line in text.lines() {
                            self.cached_lines.push(line.to_string());
                        }
                    }
                    StreamData::Binary(bin) => {
                        self.cached_lines.push(format!("[binary: {} bytes]", bin.len()));
                    }
                }
            }
        }
        self.line_count = self.cached_lines.len();
    }

    pub fn render_stream(&self, frame: &mut Frame, area: Rect, focused: bool, stream: Option<&StreamClient>) {
        let border_color = if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let title = if let Some(s) = stream {
            format!(" {} [{}] ", s.name(), s.state())
        } else {
            " Stream Viewer ".to_string()
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        if let Some(stream) = stream {
            let buffer = stream.buffer();

            if buffer.is_empty() {
                let msg = Paragraph::new(Line::from(Span::styled(
                    "No data received yet...",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )))
                .block(block);
                frame.render_widget(msg, area);
            } else {
                let lines: Vec<Line> = buffer
                    .iter()
                    .flat_map(|data| {
                        match data {
                            StreamData::Text(text) => {
                                text.lines()
                                    .map(|line| {
                                        Line::from(Span::styled(line.to_string(), Style::default().fg(Color::White)))
                                    })
                                    .collect::<Vec<_>>()
                            }
                            StreamData::Binary(bin) => {
                                vec![Line::from(Span::styled(
                                    format!("[binary: {} bytes]", bin.len()),
                                    Style::default().fg(Color::Yellow),
                                ))]
                            }
                        }
                    })
                    .collect();

                let paragraph = Paragraph::new(lines)
                    .block(block)
                    .wrap(Wrap { trim: false })
                    .scroll((self.scroll_offset, 0));

                frame.render_widget(paragraph, area);
            }
        } else {
            let msg = Paragraph::new(Line::from(Span::styled(
                "Select a stream from the menu",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )))
            .block(block);
            frame.render_widget(msg, area);
        }
    }

    pub fn render_stream_themed(&mut self, frame: &mut Frame, area: Rect, focused: bool, stream: Option<&StreamClient>, theme: &Theme) {
        self.update_cached_lines(stream);

        let (stream_area, search_area) = if self.search_state.is_active() {
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

        let search_indicator = if self.search_state.is_active() { " 󰍉" } else { "" };
        let title = if let Some(s) = stream {
            format!(" {} [{}]{} ", s.name(), s.state(), search_indicator)
        } else {
            format!(" Stream Viewer{} ", search_indicator)
        };

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        if stream.is_none() || self.cached_lines.is_empty() {
            let msg_text = if stream.is_some() {
                "No data received yet..."
            } else {
                "Select a stream from the menu"
            };
            let msg = Paragraph::new(Line::from(Span::styled(
                msg_text,
                Style::default().fg(theme.colors.muted.to_color()).add_modifier(Modifier::ITALIC),
            )))
            .block(block);
            frame.render_widget(msg, stream_area);
            
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

        let lines: Vec<Line> = self.cached_lines
            .iter()
            .enumerate()
            .skip(self.scroll_offset as usize)
            .take(self.visible_height as usize + 1)
            .map(|(line_idx, line_text)| {
                if self.search_state.is_active() && !self.search_state.query().is_empty() {
                    let spans = highlight_matches_in_line(
                        line_text,
                        line_idx,
                        self.search_state.matches(),
                        self.search_state.current_match_index(),
                        normal_style,
                        match_style,
                        current_match_style,
                    );
                    Line::from(spans)
                } else {
                    Line::from(Span::styled(line_text.clone(), normal_style))
                }
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, stream_area);

        if let Some(search_rect) = search_area {
            let search_bar = SearchBar::new(&self.search_state, theme);
            search_bar.render(frame, search_rect);
        }
    }
}

impl Default for StreamViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for StreamViewer {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.handle_key(*key),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::ScrollUp(n) => self.scroll_up(*n),
            Action::ScrollDown(n) => self.scroll_down(*n),
            Action::ScrollToTop => self.scroll_to_top(),
            Action::ScrollToBottom => self.scroll_to_bottom(),
            Action::StreamViewerSearchStart => self.start_search(),
            Action::StreamViewerSearchClose => self.close_search(),
            Action::StreamViewerSearchNext => self.search_next(),
            Action::StreamViewerSearchPrev => self.search_prev(),
            Action::StreamViewerSearchQuery(query) => {
                self.search_state.set_query(query.clone());
                self.update_search();
            }
            Action::StreamViewerSearchToggleCase => {
                self.search_state.toggle_case_sensitivity();
                self.update_search();
            }
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);
        
        let block = Block::default()
            .title(" Stream Viewer ")
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);
        
        let msg = Paragraph::new(Line::from(Span::styled(
            "Select a stream from the menu",
            Style::default().fg(theme.colors.muted.to_color()).add_modifier(Modifier::ITALIC),
        )))
        .block(block);
        frame.render_widget(msg, area);
    }
}

impl StreamViewer {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if self.search_state.is_active() {
            match self.search_state.handle_key(key) {
                SearchAction::Close => {
                    self.close_search();
                    return Some(Action::StreamViewerSearchClose);
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
                Some(Action::StreamViewerSearchStart)
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.search_state.match_count() > 0 {
                    self.search_next();
                    Some(Action::StreamViewerSearchNext)
                } else {
                    None
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.search_state.match_count() > 0 {
                    self.search_prev();
                    Some(Action::StreamViewerSearchPrev)
                } else {
                    None
                }
            }
            KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp(1)),
            KeyCode::Char('g') => Some(Action::ScrollToTop),
            KeyCode::Char('G') => Some(Action::ScrollToBottom),
            KeyCode::PageUp => Some(Action::ScrollPageUp),
            KeyCode::PageDown => Some(Action::ScrollPageDown),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_viewer_new() {
        let viewer = StreamViewer::new();
        assert_eq!(viewer.scroll_offset, 0);
        assert!(!viewer.is_search_active());
    }

    #[test]
    fn test_stream_viewer_scroll() {
        let mut viewer = StreamViewer::new();
        viewer.line_count = 100;
        viewer.visible_height = 20;

        viewer.scroll_down(10);
        assert_eq!(viewer.scroll_offset, 10);

        viewer.scroll_up(5);
        assert_eq!(viewer.scroll_offset, 5);

        viewer.scroll_to_top();
        assert_eq!(viewer.scroll_offset, 0);

        viewer.scroll_to_bottom();
        assert_eq!(viewer.scroll_offset, 80);
    }

    #[test]
    fn test_stream_viewer_search_activate() {
        let mut viewer = StreamViewer::new();
        assert!(!viewer.is_search_active());
        
        viewer.start_search();
        assert!(viewer.is_search_active());
        
        viewer.close_search();
        assert!(!viewer.is_search_active());
    }

    #[test]
    fn test_stream_viewer_search_in_cached_lines() {
        let mut viewer = StreamViewer::new();
        viewer.cached_lines = vec![
            "Hello world".to_string(),
            "This is a test".to_string(),
            "Another test here".to_string(),
        ];
        
        viewer.search_state.set_query("test".to_string());
        viewer.update_search();
        
        assert_eq!(viewer.search_state.match_count(), 2);
    }

    #[test]
    fn test_stream_viewer_search_navigation() {
        let mut viewer = StreamViewer::new();
        viewer.cached_lines = vec![
            "error one".to_string(),
            "normal line".to_string(),
            "error two".to_string(),
            "error three".to_string(),
        ];
        viewer.visible_height = 2;
        
        viewer.search_state.set_query("error".to_string());
        viewer.update_search();
        
        assert_eq!(viewer.search_state.match_count(), 3);
        assert_eq!(viewer.search_state.current_match_index(), 0);
        
        viewer.search_next();
        assert_eq!(viewer.search_state.current_match_index(), 1);
        
        viewer.search_next();
        assert_eq!(viewer.search_state.current_match_index(), 2);
        
        viewer.search_prev();
        assert_eq!(viewer.search_state.current_match_index(), 1);
    }
}
