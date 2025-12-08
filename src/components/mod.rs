pub mod placeholder;
pub mod process_monitor;
pub mod terminal;

use crossterm::event::Event;
use ratatui::{layout::Rect, Frame};

use crate::action::Action;

pub trait Component {
    fn handle_event(&mut self, event: &Event) -> Option<Action>;

    fn update(&mut self, action: &Action);

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool);
}
