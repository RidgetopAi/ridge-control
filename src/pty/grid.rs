use ratatui::style::{Color, Modifier, Style};
use vte::{Params, Parser, Perform};

#[derive(Debug, Clone, Copy, Default)]
pub struct Cell {
    pub c: char,
    pub style: Style,
}

impl Cell {
    pub fn new(c: char, style: Style) -> Self {
        Self { c, style }
    }

    pub fn empty() -> Self {
        Self {
            c: ' ',
            style: Style::default(),
        }
    }
}

struct GridPerformer {
    cells: Vec<Vec<Cell>>,
    cursor_x: usize,
    cursor_y: usize,
    cols: usize,
    rows: usize,
    current_style: Style,
}

impl GridPerformer {
    fn scroll_up(&mut self) {
        self.cells.remove(0);
        self.cells.push(vec![Cell::empty(); self.cols]);
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        if self.cursor_y + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_y += 1;
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_x = 0;
    }

    fn put_char(&mut self, c: char) {
        if self.cursor_x >= self.cols {
            self.newline();
        }
        if self.cursor_y < self.rows && self.cursor_x < self.cols {
            self.cells[self.cursor_y][self.cursor_x] = Cell::new(c, self.current_style);
            self.cursor_x += 1;
        }
    }

    fn clear_screen(&mut self) {
        for row in &mut self.cells {
            for cell in row {
                *cell = Cell::empty();
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    fn clear_line(&mut self) {
        if self.cursor_y < self.rows {
            for x in 0..self.cols {
                self.cells[self.cursor_y][x] = Cell::empty();
            }
        }
    }

    fn clear_line_from_cursor(&mut self) {
        if self.cursor_y < self.rows {
            for x in self.cursor_x..self.cols {
                self.cells[self.cursor_y][x] = Cell::empty();
            }
        }
    }
}

impl Perform for GridPerformer {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.carriage_return(),
            0x08 => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            0x07 => {}
            0x09 => {
                let next_tab = ((self.cursor_x / 8) + 1) * 8;
                self.cursor_x = next_tab.min(self.cols - 1);
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let params: Vec<u16> = params.iter().map(|p| p[0]).collect();

        match action {
            'H' | 'f' => {
                let row = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_y = row.min(self.rows.saturating_sub(1));
                self.cursor_x = col.min(self.cols.saturating_sub(1));
            }
            'A' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_y = self.cursor_y.saturating_sub(n);
            }
            'B' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_y = (self.cursor_y + n).min(self.rows.saturating_sub(1));
            }
            'C' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_x = (self.cursor_x + n).min(self.cols.saturating_sub(1));
            }
            'D' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_x = self.cursor_x.saturating_sub(n);
            }
            'J' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    2 | 3 => self.clear_screen(),
                    0 => {
                        self.clear_line_from_cursor();
                        for y in (self.cursor_y + 1)..self.rows {
                            for x in 0..self.cols {
                                self.cells[y][x] = Cell::empty();
                            }
                        }
                    }
                    _ => {}
                }
            }
            'K' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => self.clear_line_from_cursor(),
                    2 => self.clear_line(),
                    _ => {}
                }
            }
            'm' => {
                if params.is_empty() {
                    self.current_style = Style::default();
                    return;
                }
                let mut i = 0;
                while i < params.len() {
                    match params[i] {
                        0 => self.current_style = Style::default(),
                        1 => self.current_style = self.current_style.add_modifier(Modifier::BOLD),
                        2 => self.current_style = self.current_style.add_modifier(Modifier::DIM),
                        3 => self.current_style = self.current_style.add_modifier(Modifier::ITALIC),
                        4 => self.current_style = self.current_style.add_modifier(Modifier::UNDERLINED),
                        7 => self.current_style = self.current_style.add_modifier(Modifier::REVERSED),
                        22 => {
                            self.current_style = self.current_style
                                .remove_modifier(Modifier::BOLD)
                                .remove_modifier(Modifier::DIM);
                        }
                        23 => self.current_style = self.current_style.remove_modifier(Modifier::ITALIC),
                        24 => self.current_style = self.current_style.remove_modifier(Modifier::UNDERLINED),
                        27 => self.current_style = self.current_style.remove_modifier(Modifier::REVERSED),
                        30 => self.current_style = self.current_style.fg(Color::Black),
                        31 => self.current_style = self.current_style.fg(Color::Red),
                        32 => self.current_style = self.current_style.fg(Color::Green),
                        33 => self.current_style = self.current_style.fg(Color::Yellow),
                        34 => self.current_style = self.current_style.fg(Color::Blue),
                        35 => self.current_style = self.current_style.fg(Color::Magenta),
                        36 => self.current_style = self.current_style.fg(Color::Cyan),
                        37 => self.current_style = self.current_style.fg(Color::White),
                        38 => {
                            if i + 2 < params.len() && params[i + 1] == 5 {
                                let color_idx = params[i + 2];
                                self.current_style = self.current_style.fg(Color::Indexed(color_idx as u8));
                                i += 2;
                            } else if i + 4 < params.len() && params[i + 1] == 2 {
                                let r = params[i + 2] as u8;
                                let g = params[i + 3] as u8;
                                let b = params[i + 4] as u8;
                                self.current_style = self.current_style.fg(Color::Rgb(r, g, b));
                                i += 4;
                            }
                        }
                        39 => self.current_style = self.current_style.fg(Color::Reset),
                        40 => self.current_style = self.current_style.bg(Color::Black),
                        41 => self.current_style = self.current_style.bg(Color::Red),
                        42 => self.current_style = self.current_style.bg(Color::Green),
                        43 => self.current_style = self.current_style.bg(Color::Yellow),
                        44 => self.current_style = self.current_style.bg(Color::Blue),
                        45 => self.current_style = self.current_style.bg(Color::Magenta),
                        46 => self.current_style = self.current_style.bg(Color::Cyan),
                        47 => self.current_style = self.current_style.bg(Color::White),
                        48 => {
                            if i + 2 < params.len() && params[i + 1] == 5 {
                                let color_idx = params[i + 2];
                                self.current_style = self.current_style.bg(Color::Indexed(color_idx as u8));
                                i += 2;
                            } else if i + 4 < params.len() && params[i + 1] == 2 {
                                let r = params[i + 2] as u8;
                                let g = params[i + 3] as u8;
                                let b = params[i + 4] as u8;
                                self.current_style = self.current_style.bg(Color::Rgb(r, g, b));
                                i += 4;
                            }
                        }
                        49 => self.current_style = self.current_style.bg(Color::Reset),
                        90..=97 => {
                            let color = match params[i] {
                                90 => Color::DarkGray,
                                91 => Color::LightRed,
                                92 => Color::LightGreen,
                                93 => Color::LightYellow,
                                94 => Color::LightBlue,
                                95 => Color::LightMagenta,
                                96 => Color::LightCyan,
                                97 => Color::White,
                                _ => Color::Reset,
                            };
                            self.current_style = self.current_style.fg(color);
                        }
                        100..=107 => {
                            let color = match params[i] {
                                100 => Color::DarkGray,
                                101 => Color::LightRed,
                                102 => Color::LightGreen,
                                103 => Color::LightYellow,
                                104 => Color::LightBlue,
                                105 => Color::LightMagenta,
                                106 => Color::LightCyan,
                                107 => Color::White,
                                _ => Color::Reset,
                            };
                            self.current_style = self.current_style.bg(color);
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

pub struct Grid {
    performer: GridPerformer,
    parser: Parser,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![Cell::empty(); cols]; rows];
        Self {
            performer: GridPerformer {
                cells,
                cursor_x: 0,
                cursor_y: 0,
                cols,
                rows,
                current_style: Style::default(),
            },
            parser: Parser::new(),
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.performer.cols = cols;
        self.performer.rows = rows;
        self.performer.cells.resize(rows, vec![Cell::empty(); cols]);
        for row in &mut self.performer.cells {
            row.resize(cols, Cell::empty());
        }
        self.performer.cursor_x = self.performer.cursor_x.min(cols.saturating_sub(1));
        self.performer.cursor_y = self.performer.cursor_y.min(rows.saturating_sub(1));
    }

    pub fn process(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.performer, data);
    }

    pub fn cells(&self) -> &Vec<Vec<Cell>> {
        &self.performer.cells
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.performer.cursor_x, self.performer.cursor_y)
    }

    pub fn size(&self) -> (usize, usize) {
        (self.performer.cols, self.performer.rows)
    }
}
