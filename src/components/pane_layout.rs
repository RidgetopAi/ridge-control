use ratatui::layout::{Constraint, Direction, Layout, Rect};
use serde::{Deserialize, Serialize};

const MIN_PANE_PERCENT: u16 = 10;
const MAX_PANE_PERCENT: u16 = 90;
const RESIZE_STEP: u16 = 2;
const BORDER_HIT_WIDTH: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizableBorder {
    MainVertical,
    RightHorizontal,
    LeftHorizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeDirection {
    Grow,
    Shrink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneLayout {
    pub main_split_percent: u16,
    pub right_split_percent: u16,
    pub left_split_percent: u16,
}

impl Default for PaneLayout {
    fn default() -> Self {
        Self {
            main_split_percent: 67,
            right_split_percent: 50,
            left_split_percent: 60,
        }
    }
}

impl PaneLayout {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn resize_main(&mut self, direction: ResizeDirection) {
        match direction {
            ResizeDirection::Grow => {
                if self.main_split_percent + RESIZE_STEP <= MAX_PANE_PERCENT {
                    self.main_split_percent += RESIZE_STEP;
                }
            }
            ResizeDirection::Shrink => {
                if self.main_split_percent >= MIN_PANE_PERCENT + RESIZE_STEP {
                    self.main_split_percent -= RESIZE_STEP;
                }
            }
        }
    }
    
    pub fn resize_right(&mut self, direction: ResizeDirection) {
        match direction {
            ResizeDirection::Grow => {
                if self.right_split_percent + RESIZE_STEP <= MAX_PANE_PERCENT {
                    self.right_split_percent += RESIZE_STEP;
                }
            }
            ResizeDirection::Shrink => {
                if self.right_split_percent >= MIN_PANE_PERCENT + RESIZE_STEP {
                    self.right_split_percent -= RESIZE_STEP;
                }
            }
        }
    }
    
    pub fn resize_left(&mut self, direction: ResizeDirection) {
        match direction {
            ResizeDirection::Grow => {
                if self.left_split_percent + RESIZE_STEP <= MAX_PANE_PERCENT {
                    self.left_split_percent += RESIZE_STEP;
                }
            }
            ResizeDirection::Shrink => {
                if self.left_split_percent >= MIN_PANE_PERCENT + RESIZE_STEP {
                    self.left_split_percent -= RESIZE_STEP;
                }
            }
        }
    }
    
    pub fn set_main_split(&mut self, percent: u16) {
        self.main_split_percent = percent.clamp(MIN_PANE_PERCENT, MAX_PANE_PERCENT);
    }
    
    pub fn set_right_split(&mut self, percent: u16) {
        self.right_split_percent = percent.clamp(MIN_PANE_PERCENT, MAX_PANE_PERCENT);
    }
    
    pub fn set_left_split(&mut self, percent: u16) {
        self.left_split_percent = percent.clamp(MIN_PANE_PERCENT, MAX_PANE_PERCENT);
    }
    
    pub fn reset_to_defaults(&mut self) {
        *self = Self::default();
    }
    
    pub fn main_constraints(&self) -> [Constraint; 2] {
        [
            Constraint::Percentage(self.main_split_percent),
            Constraint::Percentage(100 - self.main_split_percent),
        ]
    }
    
    pub fn right_constraints(&self) -> [Constraint; 2] {
        [
            Constraint::Percentage(self.right_split_percent),
            Constraint::Percentage(100 - self.right_split_percent),
        ]
    }
    
    pub fn left_constraints(&self) -> [Constraint; 2] {
        [
            Constraint::Percentage(self.left_split_percent),
            Constraint::Percentage(100 - self.left_split_percent),
        ]
    }
    
    pub fn calculate_border_position(&self, area: Rect, border: ResizableBorder, show_conversation: bool) -> Rect {
        match border {
            ResizableBorder::MainVertical => {
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.main_constraints())
                    .split(area);
                Rect::new(
                    main_chunks[0].x + main_chunks[0].width,
                    area.y,
                    BORDER_HIT_WIDTH,
                    area.height,
                )
            }
            ResizableBorder::RightHorizontal => {
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.main_constraints())
                    .split(area);
                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(self.right_constraints())
                    .split(main_chunks[1]);
                Rect::new(
                    right_chunks[0].x,
                    right_chunks[0].y + right_chunks[0].height,
                    right_chunks[0].width,
                    BORDER_HIT_WIDTH,
                )
            }
            ResizableBorder::LeftHorizontal => {
                if !show_conversation {
                    return Rect::default();
                }
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.main_constraints())
                    .split(area);
                let left_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(self.left_constraints())
                    .split(main_chunks[0]);
                Rect::new(
                    left_chunks[0].x,
                    left_chunks[0].y + left_chunks[0].height,
                    left_chunks[0].width,
                    BORDER_HIT_WIDTH,
                )
            }
        }
    }
    
    pub fn hit_test_border(&self, x: u16, y: u16, area: Rect, show_conversation: bool) -> Option<ResizableBorder> {
        let main_border = self.calculate_border_position(area, ResizableBorder::MainVertical, show_conversation);
        if x >= main_border.x.saturating_sub(1) && x <= main_border.x + BORDER_HIT_WIDTH 
            && y >= main_border.y && y < main_border.y + main_border.height {
            return Some(ResizableBorder::MainVertical);
        }
        
        let right_border = self.calculate_border_position(area, ResizableBorder::RightHorizontal, show_conversation);
        if y >= right_border.y.saturating_sub(1) && y <= right_border.y + BORDER_HIT_WIDTH
            && x >= right_border.x && x < right_border.x + right_border.width {
            return Some(ResizableBorder::RightHorizontal);
        }
        
        if show_conversation {
            let left_border = self.calculate_border_position(area, ResizableBorder::LeftHorizontal, show_conversation);
            if y >= left_border.y.saturating_sub(1) && y <= left_border.y + BORDER_HIT_WIDTH
                && x >= left_border.x && x < left_border.x + left_border.width {
                return Some(ResizableBorder::LeftHorizontal);
            }
        }
        
        None
    }
    
    pub fn handle_mouse_drag(&mut self, x: u16, y: u16, area: Rect, border: ResizableBorder, show_conversation: bool) {
        match border {
            ResizableBorder::MainVertical => {
                if area.width == 0 {
                    return;
                }
                let relative_x = x.saturating_sub(area.x);
                let percent = (relative_x as u32 * 100 / area.width as u32) as u16;
                self.set_main_split(percent);
            }
            ResizableBorder::RightHorizontal => {
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.main_constraints())
                    .split(area);
                let right_area = main_chunks[1];
                if right_area.height == 0 {
                    return;
                }
                let relative_y = y.saturating_sub(right_area.y);
                let percent = (relative_y as u32 * 100 / right_area.height as u32) as u16;
                self.set_right_split(percent);
            }
            ResizableBorder::LeftHorizontal => {
                if !show_conversation {
                    return;
                }
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.main_constraints())
                    .split(area);
                let left_area = main_chunks[0];
                if left_area.height == 0 {
                    return;
                }
                let relative_y = y.saturating_sub(left_area.y);
                let percent = (relative_y as u32 * 100 / left_area.height as u32) as u16;
                self.set_left_split(percent);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    None,
    Dragging(ResizableBorder),
}

impl Default for DragState {
    fn default() -> Self {
        Self::None
    }
}

impl DragState {
    pub fn is_dragging(&self) -> bool {
        matches!(self, DragState::Dragging(_))
    }
    
    pub fn start(&mut self, border: ResizableBorder) {
        *self = DragState::Dragging(border);
    }
    
    pub fn stop(&mut self) {
        *self = DragState::None;
    }
    
    pub fn border(&self) -> Option<ResizableBorder> {
        match self {
            DragState::Dragging(b) => Some(*b),
            DragState::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pane_layout_default() {
        let layout = PaneLayout::new();
        assert_eq!(layout.main_split_percent, 67);
        assert_eq!(layout.right_split_percent, 50);
        assert_eq!(layout.left_split_percent, 60);
    }
    
    #[test]
    fn test_resize_main_grow() {
        let mut layout = PaneLayout::new();
        layout.resize_main(ResizeDirection::Grow);
        assert_eq!(layout.main_split_percent, 69);
    }
    
    #[test]
    fn test_resize_main_shrink() {
        let mut layout = PaneLayout::new();
        layout.resize_main(ResizeDirection::Shrink);
        assert_eq!(layout.main_split_percent, 65);
    }
    
    #[test]
    fn test_resize_clamp_min() {
        let mut layout = PaneLayout::new();
        layout.main_split_percent = 12;
        layout.resize_main(ResizeDirection::Shrink);
        assert_eq!(layout.main_split_percent, 10); // Cannot go below MIN_PANE_PERCENT
    }
    
    #[test]
    fn test_resize_clamp_max() {
        let mut layout = PaneLayout::new();
        layout.main_split_percent = 89;
        layout.resize_main(ResizeDirection::Grow);
        assert_eq!(layout.main_split_percent, 89); // Cannot go above MAX_PANE_PERCENT
    }
    
    #[test]
    fn test_set_main_split_clamp() {
        let mut layout = PaneLayout::new();
        layout.set_main_split(5);
        assert_eq!(layout.main_split_percent, MIN_PANE_PERCENT);
        layout.set_main_split(95);
        assert_eq!(layout.main_split_percent, MAX_PANE_PERCENT);
    }
    
    #[test]
    fn test_reset_to_defaults() {
        let mut layout = PaneLayout::new();
        layout.main_split_percent = 80;
        layout.right_split_percent = 30;
        layout.reset_to_defaults();
        assert_eq!(layout.main_split_percent, 67);
        assert_eq!(layout.right_split_percent, 50);
    }
    
    #[test]
    fn test_main_constraints() {
        let layout = PaneLayout::new();
        let constraints = layout.main_constraints();
        assert_eq!(constraints[0], Constraint::Percentage(67));
        assert_eq!(constraints[1], Constraint::Percentage(33));
    }
    
    #[test]
    fn test_drag_state() {
        let mut state = DragState::None;
        assert!(!state.is_dragging());
        assert!(state.border().is_none());
        
        state.start(ResizableBorder::MainVertical);
        assert!(state.is_dragging());
        assert_eq!(state.border(), Some(ResizableBorder::MainVertical));
        
        state.stop();
        assert!(!state.is_dragging());
    }
    
    #[test]
    fn test_hit_test_border_main() {
        let layout = PaneLayout::new();
        let area = Rect::new(0, 0, 100, 50);
        let border = layout.hit_test_border(67, 25, area, false);
        assert_eq!(border, Some(ResizableBorder::MainVertical));
    }
    
    #[test]
    fn test_hit_test_border_none() {
        let layout = PaneLayout::new();
        let area = Rect::new(0, 0, 100, 50);
        let border = layout.hit_test_border(30, 25, area, false);
        assert!(border.is_none());
    }
    
    #[test]
    fn test_handle_mouse_drag_main() {
        let mut layout = PaneLayout::new();
        let area = Rect::new(0, 0, 100, 50);
        layout.handle_mouse_drag(50, 25, area, ResizableBorder::MainVertical, false);
        assert_eq!(layout.main_split_percent, 50);
    }
    
    #[test]
    fn test_handle_mouse_drag_right() {
        let mut layout = PaneLayout::new();
        layout.set_main_split(50);
        let area = Rect::new(0, 0, 100, 50);
        layout.handle_mouse_drag(75, 15, area, ResizableBorder::RightHorizontal, false);
        assert!(layout.right_split_percent >= MIN_PANE_PERCENT && layout.right_split_percent <= MAX_PANE_PERCENT);
    }
}
