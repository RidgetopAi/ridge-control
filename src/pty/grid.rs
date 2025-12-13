// Terminal grid - some methods for future use
#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};
use vte::{Params, Parser, Perform};

const DEFAULT_SCROLLBACK_SIZE: usize = 10000;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

impl Position {
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: Position,
    pub end: Position,
}

impl Selection {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn normalized(&self) -> (Position, Position) {
        if self.start.y < self.end.y || (self.start.y == self.end.y && self.start.x <= self.end.x) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn contains(&self, pos: Position) -> bool {
        let (start, end) = self.normalized();
        if pos.y < start.y || pos.y > end.y {
            return false;
        }
        if pos.y == start.y && pos.y == end.y {
            return pos.x >= start.x && pos.x <= end.x;
        }
        if pos.y == start.y {
            return pos.x >= start.x;
        }
        if pos.y == end.y {
            return pos.x <= end.x;
        }
        true
    }
}

struct RingBuffer {
    lines: Vec<Vec<Cell>>,
    head: usize,
    len: usize,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            lines: Vec::with_capacity(capacity),
            head: 0,
            len: 0,
            capacity,
        }
    }

    fn push(&mut self, line: Vec<Cell>) {
        if self.lines.len() < self.capacity {
            self.lines.push(line);
            self.len += 1;
        } else {
            self.lines[self.head] = line;
            self.head = (self.head + 1) % self.capacity;
        }
    }

    fn get(&self, index: usize) -> Option<&Vec<Cell>> {
        if index >= self.len {
            return None;
        }
        let real_index = if self.len < self.capacity {
            index
        } else {
            (self.head + index) % self.capacity
        };
        self.lines.get(real_index)
    }

    fn len(&self) -> usize {
        self.len
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.head = 0;
        self.len = 0;
    }
}

struct GridPerformer {
    cells: Vec<Vec<Cell>>,
    cursor_x: usize,
    cursor_y: usize,
    cols: usize,
    rows: usize,
    current_style: Style,
    scrollback: RingBuffer,
    scrollback_size: usize,
}

impl GridPerformer {
    fn scroll_up(&mut self) {
        if !self.cells.is_empty() {
            let top_line = self.cells.remove(0);
            self.scrollback.push(top_line);
        }
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

    fn clear_line_to_cursor(&mut self) {
        if self.cursor_y < self.rows {
            for x in 0..=self.cursor_x.min(self.cols.saturating_sub(1)) {
                self.cells[self.cursor_y][x] = Cell::empty();
            }
        }
    }

    fn delete_lines(&mut self, count: usize) {
        for _ in 0..count {
            if self.cursor_y < self.rows {
                self.cells.remove(self.cursor_y);
                self.cells.push(vec![Cell::empty(); self.cols]);
            }
        }
    }

    fn insert_lines(&mut self, count: usize) {
        for _ in 0..count {
            if self.cursor_y < self.rows {
                self.cells.pop();
                self.cells.insert(self.cursor_y, vec![Cell::empty(); self.cols]);
            }
        }
    }

    fn erase_chars(&mut self, count: usize) {
        if self.cursor_y < self.rows {
            for i in 0..count {
                let x = self.cursor_x + i;
                if x < self.cols {
                    self.cells[self.cursor_y][x] = Cell::empty();
                }
            }
        }
    }

    fn delete_chars(&mut self, count: usize) {
        if self.cursor_y < self.rows {
            let row = &mut self.cells[self.cursor_y];
            for _ in 0..count {
                if self.cursor_x < row.len() {
                    row.remove(self.cursor_x);
                    row.push(Cell::empty());
                }
            }
        }
    }

    fn insert_blanks(&mut self, count: usize) {
        if self.cursor_y < self.rows {
            let row = &mut self.cells[self.cursor_y];
            for _ in 0..count {
                if self.cursor_x < row.len() {
                    row.pop();
                    row.insert(self.cursor_x, Cell::empty());
                }
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
            'B' | 'e' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_y = (self.cursor_y + n).min(self.rows.saturating_sub(1));
            }
            'C' | 'a' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_x = (self.cursor_x + n).min(self.cols.saturating_sub(1));
            }
            'D' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_x = self.cursor_x.saturating_sub(n);
            }
            'E' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_y = (self.cursor_y + n).min(self.rows.saturating_sub(1));
                self.cursor_x = 0;
            }
            'F' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_y = self.cursor_y.saturating_sub(n);
                self.cursor_x = 0;
            }
            'G' | '`' => {
                let col = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_x = col.min(self.cols.saturating_sub(1));
            }
            'd' => {
                let row = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_y = row.min(self.rows.saturating_sub(1));
            }
            'J' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => {
                        self.clear_line_from_cursor();
                        for y in (self.cursor_y + 1)..self.rows {
                            for x in 0..self.cols {
                                self.cells[y][x] = Cell::empty();
                            }
                        }
                    }
                    1 => {
                        self.clear_line_to_cursor();
                        for y in 0..self.cursor_y {
                            for x in 0..self.cols {
                                self.cells[y][x] = Cell::empty();
                            }
                        }
                    }
                    2 | 3 => self.clear_screen(),
                    _ => {}
                }
            }
            'K' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => self.clear_line_from_cursor(),
                    1 => self.clear_line_to_cursor(),
                    2 => self.clear_line(),
                    _ => {}
                }
            }
            'L' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.insert_lines(n);
            }
            'M' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.delete_lines(n);
            }
            'P' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.delete_chars(n);
            }
            'X' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.erase_chars(n);
            }
            '@' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.insert_blanks(n);
            }
            'S' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.scroll_up();
                }
            }
            'T' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    if !self.cells.is_empty() {
                        self.cells.pop();
                        self.cells.insert(0, vec![Cell::empty(); self.cols]);
                    }
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
                        5 => self.current_style = self.current_style.add_modifier(Modifier::SLOW_BLINK),
                        6 => self.current_style = self.current_style.add_modifier(Modifier::RAPID_BLINK),
                        7 => self.current_style = self.current_style.add_modifier(Modifier::REVERSED),
                        8 => self.current_style = self.current_style.add_modifier(Modifier::HIDDEN),
                        9 => self.current_style = self.current_style.add_modifier(Modifier::CROSSED_OUT),
                        22 => {
                            self.current_style = self.current_style
                                .remove_modifier(Modifier::BOLD)
                                .remove_modifier(Modifier::DIM);
                        }
                        23 => self.current_style = self.current_style.remove_modifier(Modifier::ITALIC),
                        24 => self.current_style = self.current_style.remove_modifier(Modifier::UNDERLINED),
                        25 => {
                            self.current_style = self.current_style
                                .remove_modifier(Modifier::SLOW_BLINK)
                                .remove_modifier(Modifier::RAPID_BLINK);
                        }
                        27 => self.current_style = self.current_style.remove_modifier(Modifier::REVERSED),
                        28 => self.current_style = self.current_style.remove_modifier(Modifier::HIDDEN),
                        29 => self.current_style = self.current_style.remove_modifier(Modifier::CROSSED_OUT),
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
            's' => {
            }
            'u' => {
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

pub struct Grid {
    performer: GridPerformer,
    parser: Parser,
    scroll_offset: usize,
    selection: Option<Selection>,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, DEFAULT_SCROLLBACK_SIZE)
    }

    pub fn with_scrollback(cols: usize, rows: usize, scrollback_size: usize) -> Self {
        let cells = vec![vec![Cell::empty(); cols]; rows];
        Self {
            performer: GridPerformer {
                cells,
                cursor_x: 0,
                cursor_y: 0,
                cols,
                rows,
                current_style: Style::default(),
                scrollback: RingBuffer::new(scrollback_size),
                scrollback_size,
            },
            parser: Parser::new(),
            scroll_offset: 0,
            selection: None,
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
        self.scroll_offset = 0;
    }

    pub fn process(&mut self, data: &[u8]) {
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
        }
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

    pub fn scrollback_len(&self) -> usize {
        self.performer.scrollback.len()
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn max_scroll_offset(&self) -> usize {
        self.performer.scrollback.len()
    }

    pub fn scroll_up(&mut self, amount: usize) {
        let max = self.max_scroll_offset();
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = self.max_scroll_offset();
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn get_visible_line(&self, visible_row: usize) -> Option<&Vec<Cell>> {
        if self.scroll_offset == 0 {
            self.performer.cells.get(visible_row)
        } else {
            let total_scrollback = self.performer.scrollback.len();
            let lines_from_scrollback = self.scroll_offset.min(total_scrollback);

            if visible_row < lines_from_scrollback {
                let scrollback_idx = total_scrollback - self.scroll_offset + visible_row;
                self.performer.scrollback.get(scrollback_idx)
            } else {
                let active_row = visible_row - lines_from_scrollback;
                self.performer.cells.get(active_row)
            }
        }
    }

    pub fn start_selection(&mut self, x: usize, y: usize) {
        let pos = Position::new(x, y);
        self.selection = Some(Selection::new(pos, pos));
    }

    pub fn update_selection(&mut self, x: usize, y: usize) {
        if let Some(ref mut sel) = self.selection {
            sel.end = Position::new(x, y);
        }
    }

    pub fn end_selection(&mut self) {
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    pub fn get_selected_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        let (start, end) = sel.normalized();

        let mut text = String::new();
        let rows = self.performer.rows;

        for y in start.y..=end.y.min(rows.saturating_sub(1)) {
            let line = if let Some(line) = self.get_visible_line(y) {
                line
            } else {
                continue;
            };

            let x_start = if y == start.y { start.x } else { 0 };
            let x_end = if y == end.y { end.x } else { line.len().saturating_sub(1) };

            for cell in line.iter().take(x_end + 1).skip(x_start) {
                text.push(cell.c);
            }

            if y < end.y {
                text.push('\n');
            }
        }

        let trimmed = text.trim_end().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    pub fn is_position_selected(&self, x: usize, y: usize) -> bool {
        if let Some(ref sel) = self.selection {
            sel.contains(Position::new(x, y))
        } else {
            false
        }
    }

    pub fn cols(&self) -> usize {
        self.performer.cols
    }

    pub fn rows(&self) -> usize {
        self.performer.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer() {
        let mut buf = RingBuffer::new(3);
        buf.push(vec![Cell::new('a', Style::default())]);
        buf.push(vec![Cell::new('b', Style::default())]);
        buf.push(vec![Cell::new('c', Style::default())]);

        assert_eq!(buf.len(), 3);
        assert_eq!(buf.get(0).unwrap()[0].c, 'a');
        assert_eq!(buf.get(1).unwrap()[0].c, 'b');
        assert_eq!(buf.get(2).unwrap()[0].c, 'c');

        buf.push(vec![Cell::new('d', Style::default())]);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.get(0).unwrap()[0].c, 'b');
        assert_eq!(buf.get(1).unwrap()[0].c, 'c');
        assert_eq!(buf.get(2).unwrap()[0].c, 'd');
    }

    #[test]
    fn test_selection_normalized() {
        let sel = Selection::new(Position::new(5, 2), Position::new(2, 1));
        let (start, end) = sel.normalized();
        assert_eq!(start, Position::new(2, 1));
        assert_eq!(end, Position::new(5, 2));
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection::new(Position::new(2, 1), Position::new(5, 3));
        assert!(sel.contains(Position::new(3, 2)));
        assert!(sel.contains(Position::new(2, 1)));
        assert!(sel.contains(Position::new(5, 3)));
        assert!(!sel.contains(Position::new(1, 1)));
        assert!(!sel.contains(Position::new(6, 3)));
        assert!(!sel.contains(Position::new(0, 0)));
    }

    #[test]
    fn test_grid_scrollback() {
        let mut grid = Grid::with_scrollback(10, 3, 100);

        for i in 0..10 {
            grid.process(format!("Line {}\n", i).as_bytes());
        }

        assert!(grid.scrollback_len() > 0);

        grid.scroll_up(2);
        assert_eq!(grid.scroll_offset(), 2);

        grid.scroll_to_bottom();
        assert_eq!(grid.scroll_offset(), 0);
    }
}
