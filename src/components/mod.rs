pub mod confirm_dialog;
pub mod menu;
pub mod placeholder;
pub mod process_monitor;
pub mod stream_viewer;
pub mod terminal;

use crossterm::event::Event;
use ratatui::{layout::Rect, Frame};

use crate::action::Action;

/// Component trait for standardized lifecycle per RIDGE-CONTROL-MASTER.md
/// Pattern: handle_event() → update() → render()
pub trait Component {
    /// Handle crossterm events, return Action if event was consumed
    fn handle_event(&mut self, event: &Event) -> Option<Action>;

    /// Update component state based on dispatched Action
    fn update(&mut self, action: &Action);

    /// Render the component to the frame
    fn render(&self, frame: &mut Frame, area: Rect, focused: bool);
}
