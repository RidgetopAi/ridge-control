// Terminal widget - some methods for future scroll display features
#![allow(dead_code)]

use crossterm::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    widgets::{Block, Borders, Widget},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;
use crate::pty::grid::Grid;

pub struct TerminalWidget {
    grid: Grid,
    inner_area: Option<Rect>,
}

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

    fn screen_to_grid(&self, screen_x: u16, screen_y: u16) -> Option<(usize, usize)> {
        let inner = self.inner_area?;
        if screen_x < inner.x || screen_y < inner.y {
            return None;
        }
        let x = (screen_x - inner.x) as usize;
        let y = (screen_y - inner.y) as usize;
        if x < self.grid.cols() && y < self.grid.rows() {
            Some((x, y))
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

impl<'a> Widget for GridWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (cursor_x, cursor_y) = self.grid.cursor();
        let scroll_offset = self.grid.scroll_offset();

        for y in 0..area.height as usize {
            let line = match self.grid.get_visible_line(y) {
                Some(line) => line,
                None => continue,
            };

            for x in 0..area.width as usize {
                if x >= line.len() {
                    break;
                }

                let buf_x = area.x + x as u16;
                let buf_y = area.y + y as u16;
                let cell = &line[x];

                let mut style = cell.style;

                if self.grid.is_position_selected(x, y) {
                    style = self.theme.selection_style()
                        .remove_modifier(Modifier::REVERSED);
                }

                let is_cursor = self.show_cursor
                    && scroll_offset == 0
                    && x == cursor_x
                    && y == cursor_y;

                if is_cursor {
                    style = style
                        .bg(self.theme.terminal.cursor_color.to_color())
                        .fg(self.theme.colors.background.to_color());
                }

                if let Some(buf_cell) = buf.cell_mut((buf_x, buf_y)) {
                    buf_cell.set_char(cell.c);
                    buf_cell.set_style(style);
                }
            }
        }
    }
}
