use crossterm::event::Event;
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;

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

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let paragraph = Paragraph::new(self.content.as_str())
            .block(block)
            .style(Style::default().fg(theme.colors.muted.to_color()));

        frame.render_widget(paragraph, area);
    }
}
