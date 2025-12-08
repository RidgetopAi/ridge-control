use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::streams::{StreamClient, StreamData};

pub struct StreamViewer {
    scroll_offset: u16,
    line_count: usize,
    visible_height: u16,
    selected_stream_name: String,
}

impl StreamViewer {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            line_count: 0,
            visible_height: 10,
            selected_stream_name: String::new(),
        }
    }

    pub fn set_visible_height(&mut self, height: u16) {
        self.visible_height = height.saturating_sub(2);
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
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        self.render_stream(frame, area, focused, None);
    }
}

impl StreamViewer {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
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
