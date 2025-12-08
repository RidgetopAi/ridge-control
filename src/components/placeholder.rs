use crossterm::event::Event;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::action::Action;
use crate::components::Component;

pub struct PlaceholderWidget {
    title: String,
    content: String,
}

impl PlaceholderWidget {
    pub fn new(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content: content.into(),
        }
    }

    pub fn menu() -> Self {
        Self::new("Menu", "Coming in i[13]...")
    }
}

impl Component for PlaceholderWidget {
    fn handle_event(&mut self, _event: &Event) -> Option<Action> {
        None
    }

    fn update(&mut self, _action: &Action) {}

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused {
            Color::Magenta
        } else {
            Color::DarkGray
        };

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(self.content.as_str())
            .block(block)
            .style(Style::default().fg(Color::Gray));

        frame.render_widget(paragraph, area);
    }
}
