pub mod confirm_dialog;
pub mod menu;
pub mod placeholder;
pub mod process_monitor;
pub mod stream_viewer;
pub mod terminal;

use crossterm::event::Event;
use ratatui::{layout::Rect, Frame};

use crate::action::Action;

pub use confirm_dialog::ConfirmDialog;

pub trait Component {
    fn handle_event(&mut self, event: &Event) -> Option<Action>;

    fn update(&mut self, action: &Action);

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool);
}
