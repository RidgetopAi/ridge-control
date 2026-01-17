// Terminal widget - some methods for future scroll display features

use crossterm::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Widget},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;
use crate::pty::grid::{Grid, MouseMode};

pub struct TerminalWidget {
    grid: Grid,
    inner_area: Option<Rect>,
}

#[allow(dead_code)]
impl TerminalWidget {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            inner_area: None,
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.grid.resize(cols, rows);
    }

    pub fn process_output(&mut self, data: &[u8]) {
        self.grid.process(data);
    }

    pub fn size(&self) -> (usize, usize) {
        self.grid.size()
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.grid.scroll_up(amount as usize);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.grid.scroll_down(amount as usize);
    }

    pub fn scroll_to_top(&mut self) {
        self.grid.scroll_to_top();
    }

    pub fn scroll_to_bottom(&mut self) {
        self.grid.scroll_to_bottom();
    }
    
    /// Get the mouse tracking mode enabled by the nested application
    pub fn mouse_mode(&self) -> MouseMode {
        self.grid.mouse_mode()
    }

    /// Check if we're in alternate screen mode (running a TUI)
    pub fn is_alternate_screen(&self) -> bool {
        self.grid.is_alternate_screen()
    }

    /// Calculate view_offset - the same offset used by GridWidget::render()
    /// This handles cases where cursor is below the visible area (e.g., Claude Code running)
    fn calculate_view_offset(&self) -> usize {
        let inner = match self.inner_area {
            Some(area) => area,
            None => return 0,
        };
        let area_height = inner.height as usize;
        let scroll_offset = self.grid.scroll_offset();
        let (_, cursor_y) = self.grid.cursor();
        
        if scroll_offset == 0 && cursor_y >= area_height {
            cursor_y - area_height + 1
        } else {
            0
        }
    }

    fn screen_to_grid(&self, screen_x: u16, screen_y: u16) -> Option<(usize, usize)> {
        let inner = self.inner_area?;
        if screen_x < inner.x || screen_y < inner.y {
            return None;
        }
        let x = (screen_x - inner.x) as usize;
        let y = (screen_y - inner.y) as usize;
        
        // Apply view_offset to match what's actually rendered on screen
        // This fixes selection offset when cursor is below visible area
        let view_offset = self.calculate_view_offset();
        let grid_y = y + view_offset;
        
        if x < self.grid.cols() && grid_y < self.grid.rows() {
            Some((x, grid_y))
        } else {
            None
        }
    }

    pub fn start_selection(&mut self, screen_x: u16, screen_y: u16) {
        if let Some((x, y)) = self.screen_to_grid(screen_x, screen_y) {
            self.grid.start_selection(x, y);
        }
    }

    pub fn update_selection(&mut self, screen_x: u16, screen_y: u16) {
        if let Some((x, y)) = self.screen_to_grid(screen_x, screen_y) {
            self.grid.update_selection(x, y);
        }
    }

    pub fn end_selection(&mut self) {
        self.grid.end_selection();
    }

    pub fn clear_selection(&mut self) {
        self.grid.clear_selection();
    }

    pub fn get_selected_text(&self) -> Option<String> {
        self.grid.get_selected_text()
    }

    pub fn has_selection(&self) -> bool {
        self.grid.selection().is_some()
    }

    pub fn is_scrolled(&self) -> bool {
        self.grid.scroll_offset() > 0
    }

    pub fn scroll_info(&self) -> (usize, usize) {
        (self.grid.scroll_offset(), self.grid.max_scroll_offset())
    }
}

impl Component for TerminalWidget {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Mouse(mouse) => self.handle_mouse(*mouse),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::PtyOutput(data) => {
                self.process_output(data);
            }
            Action::PtyResize { cols, rows } => {
                self.resize(*cols as usize, *rows as usize);
            }
            Action::ScrollUp(n) => {
                self.scroll_up(*n);
            }
            Action::ScrollDown(n) => {
                self.scroll_down(*n);
            }
            Action::ScrollToTop => {
                self.scroll_to_top();
            }
            Action::ScrollToBottom => {
                self.scroll_to_bottom();
            }
            Action::ScrollPageUp => {
                let page = self.grid.rows().saturating_sub(1).max(1);
                self.scroll_up(page as u16);
            }
            Action::ScrollPageDown => {
                let page = self.grid.rows().saturating_sub(1).max(1);
                self.scroll_down(page as u16);
            }
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let scroll_offset = self.grid.scroll_offset();
        let max_scroll = self.grid.max_scroll_offset();
        let title = if scroll_offset > 0 {
            format!(" Terminal [{}/{}] ", scroll_offset, max_scroll)
        } else {
            " Terminal ".to_string()
        };

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let grid_widget = GridWidget {
            grid: &self.grid,
            show_cursor: focused && scroll_offset == 0,
            theme,
        };
        frame.render_widget(grid_widget, inner);
    }
}

impl TerminalWidget {
    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.start_selection(mouse.column, mouse.row);
                None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                // Check for drag-to-scroll: if dragging outside the terminal area,
                // scroll in that direction while extending selection
                if let Some(inner) = self.inner_area {
                    if mouse.row < inner.y {
                        // Dragging above terminal - scroll up and extend selection to top
                        self.scroll_up(1);
                        self.update_selection(mouse.column, inner.y);
                        return Some(Action::ScrollUp(1));
                    } else if mouse.row >= inner.y + inner.height {
                        // Dragging below terminal - scroll down and extend selection to bottom
                        self.scroll_down(1);
                        self.update_selection(mouse.column, inner.y + inner.height - 1);
                        return Some(Action::ScrollDown(1));
                    }
                }
                self.update_selection(mouse.column, mouse.row);
                None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.end_selection();
                if self.has_selection() {
                    Some(Action::Copy)
                } else {
                    None
                }
            }
            MouseEventKind::ScrollUp => {
                Some(Action::ScrollUp(3))
            }
            MouseEventKind::ScrollDown => {
                Some(Action::ScrollDown(3))
            }
            _ => None,
        }
    }

    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = Some(area);
    }
}

struct GridWidget<'a> {
    grid: &'a Grid,
    show_cursor: bool,
    theme: &'a Theme,
}

impl<'a> GridWidget<'a> {
    /// Compute the effective style for a cell, factoring in selection and cursor
    #[inline]
    fn cell_style(
        &self,
        cell_style: Style,
        x: usize,
        grid_row: usize,
        cursor_x: usize,
        cursor_y: usize,
        scroll_offset: usize,
    ) -> Style {
        // Check if this position is selected
        if self.grid.is_position_selected(x, grid_row) {
            return self.theme.selection_style().remove_modifier(Modifier::REVERSED);
        }

        // Check if this is the cursor position
        let is_cursor = self.show_cursor
            && self.grid.cursor_visible()
            && scroll_offset == 0
            && x == cursor_x
            && grid_row == cursor_y;

        if is_cursor {
            cell_style
                .bg(self.theme.terminal.cursor_color.to_color())
                .fg(self.theme.colors.background.to_color())
        } else {
            cell_style
        }
    }
}

impl<'a> Widget for GridWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (cursor_x, cursor_y) = self.grid.cursor();
        let scroll_offset = self.grid.scroll_offset();
        let area_height = area.height as usize;
        let area_width = area.width as usize;

        // Calculate view offset to keep cursor visible when it's beyond the render area.
        let view_offset = if scroll_offset == 0 && cursor_y >= area_height {
            cursor_y - area_height + 1
        } else {
            0
        };

        for y in 0..area_height {
            let grid_row = y + view_offset;
            let line = match self.grid.get_visible_line(grid_row) {
                Some(line) => line,
                None => continue,
            };

            let buf_y = area.y + y as u16;
            let line_len = line.len().min(area_width);
            
            if line_len == 0 {
                continue;
            }

            // Batch contiguous cells with same style into runs
            let mut run_start = 0;
            let mut run_chars = String::with_capacity(line_len);
            let mut run_style = self.cell_style(
                line[0].style,
                0,
                grid_row,
                cursor_x,
                cursor_y,
                scroll_offset,
            );
            run_chars.push(line[0].c);

            for (x, cell) in line.iter().enumerate().take(line_len).skip(1) {
                let style = self.cell_style(
                    cell.style,
                    x,
                    grid_row,
                    cursor_x,
                    cursor_y,
                    scroll_offset,
                );

                if style == run_style {
                    // Same style - extend the run
                    run_chars.push(cell.c);
                } else {
                    // Different style - flush the current run and start new one
                    buf.set_string(area.x + run_start as u16, buf_y, &run_chars, run_style);
                    run_start = x;
                    run_chars.clear();
                    run_chars.push(cell.c);
                    run_style = style;
                }
            }

            // Flush the final run
            buf.set_string(area.x + run_start as u16, buf_y, &run_chars, run_style);
        }
    }
}
