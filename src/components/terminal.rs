use crossterm::event::Event;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::pty::grid::Grid;

pub struct TerminalWidget {
    grid: Grid,
}

impl TerminalWidget {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
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
}

impl Component for TerminalWidget {
    fn handle_event(&mut self, _event: &Event) -> Option<Action> {
        None
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::PtyOutput(data) => {
                self.process_output(data);
            }
            Action::PtyResize { cols, rows } => {
                self.resize(*cols as usize, *rows as usize);
            }
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let block = Block::default()
            .title(" Terminal ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let grid_widget = GridWidget {
            grid: &self.grid,
            show_cursor: focused,
        };
        frame.render_widget(grid_widget, inner);
    }
}

struct GridWidget<'a> {
    grid: &'a Grid,
    show_cursor: bool,
}

impl<'a> Widget for GridWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cells = self.grid.cells();
        let (cursor_x, cursor_y) = self.grid.cursor();

        for (y, row) in cells.iter().enumerate() {
            if y >= area.height as usize {
                break;
            }
            for (x, cell) in row.iter().enumerate() {
                if x >= area.width as usize {
                    break;
                }
                let buf_x = area.x + x as u16;
                let buf_y = area.y + y as u16;

                let mut style = cell.style;
                if self.show_cursor && x == cursor_x && y == cursor_y {
                    style = style.bg(Color::White).fg(Color::Black);
                }

                let buf_cell = buf.cell_mut((buf_x, buf_y));
                if let Some(buf_cell) = buf_cell {
                    buf_cell.set_char(cell.c);
                    buf_cell.set_style(style);
                }
            }
        }
    }
}
