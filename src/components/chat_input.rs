
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;

/// Selection position in logical text coordinates (line_index, char_column)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionPos {
    pub line: usize,
    pub col: usize,
}

impl SelectionPos {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// Multi-line text input component for composing LLM messages
pub struct ChatInput {
    /// Text buffer containing all lines
    lines: Vec<String>,
    /// Cursor position: (line_index, column_index)
    cursor: (usize, usize),
    /// Scroll offset for vertical scrolling
    scroll_offset: u16,
    /// Visible height of the input area
    visible_height: u16,
    /// Text selection (start, end) in logical coordinates
    selection: Option<(SelectionPos, SelectionPos)>,
    /// Whether we're currently dragging to select
    selecting: bool,
    /// Inner area (without borders) for coordinate conversion
    inner_area: Rect,
}

impl ChatInput {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: (0, 0),
            scroll_offset: 0,
            visible_height: 5,
            selection: None,
            selecting: false,
            inner_area: Rect::default(),
        }
    }

    /// Get the current text content as a single string
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Check if the input is empty
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Clear the input buffer
    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor = (0, 0);
        self.scroll_offset = 0;
        self.selection = None;
        self.selecting = false;
    }

    /// Paste text at the cursor position
    pub fn paste_text(&mut self, text: &str) {
        // If there's a selection, delete it first
        self.delete_selection();

        for c in text.chars() {
            if c == '\n' {
                self.insert_newline();
            } else if c != '\r' {
                // Skip carriage returns, insert everything else
                self.insert_char(c);
            }
        }
    }

    /// Set the visible height for scroll calculations
    #[allow(dead_code)]
    pub fn set_visible_height(&mut self, height: u16) {
        self.visible_height = height.saturating_sub(2); // Account for borders
    }

    /// Set the inner area for mouse coordinate conversion
    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = area;
        self.visible_height = area.height;
    }

    /// Get line count
    #[allow(dead_code)]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Insert a character at the cursor position
    fn insert_char(&mut self, c: char) {
        // If there's a selection, delete it first
        self.delete_selection();

        let (line, col) = self.cursor;
        if line < self.lines.len() {
            let current_line = &mut self.lines[line];
            let byte_pos = char_to_byte_pos(current_line, col);
            current_line.insert(byte_pos, c);
            self.cursor.1 = col + 1;
        }
        self.ensure_cursor_visible();
    }

    /// Insert a newline at the cursor position
    fn insert_newline(&mut self) {
        // If there's a selection, delete it first
        self.delete_selection();

        let (line, col) = self.cursor;
        if line < self.lines.len() {
            let current_line = &self.lines[line];
            let byte_pos = char_to_byte_pos(current_line, col);
            let remainder = current_line[byte_pos..].to_string();
            self.lines[line] = current_line[..byte_pos].to_string();
            self.lines.insert(line + 1, remainder);
            self.cursor = (line + 1, 0);
        }
        self.ensure_cursor_visible();
    }

    /// Delete the character before the cursor (backspace)
    fn delete_char_before(&mut self) {
        // If there's a selection, just delete the selection
        if self.has_selection() {
            self.delete_selection();
            return;
        }

        let (line, col) = self.cursor;
        if col > 0 {
            let current_line = &mut self.lines[line];
            let byte_pos = char_to_byte_pos(current_line, col - 1);
            let next_byte_pos = char_to_byte_pos(current_line, col);
            current_line.replace_range(byte_pos..next_byte_pos, "");
            self.cursor.1 = col - 1;
        } else if line > 0 {
            // Join with previous line
            let current_line = self.lines.remove(line);
            let prev_line_len = self.lines[line - 1].chars().count();
            self.lines[line - 1].push_str(&current_line);
            self.cursor = (line - 1, prev_line_len);
        }
        self.ensure_cursor_visible();
    }

    /// Delete the character at the cursor (delete key)
    fn delete_char_at(&mut self) {
        // If there's a selection, just delete the selection
        if self.has_selection() {
            self.delete_selection();
            return;
        }

        let (line, col) = self.cursor;
        if line < self.lines.len() {
            let current_line = &self.lines[line];
            let char_count = current_line.chars().count();
            if col < char_count {
                let byte_pos = char_to_byte_pos(current_line, col);
                let next_byte_pos = char_to_byte_pos(current_line, col + 1);
                self.lines[line].replace_range(byte_pos..next_byte_pos, "");
            } else if line + 1 < self.lines.len() {
                // Join with next line
                let next_line = self.lines.remove(line + 1);
                self.lines[line].push_str(&next_line);
            }
        }
    }

    /// Move cursor left
    fn move_left(&mut self) {
        self.clear_selection();
        let (line, col) = self.cursor;
        if col > 0 {
            self.cursor.1 = col - 1;
        } else if line > 0 {
            self.cursor.0 = line - 1;
            self.cursor.1 = self.lines[line - 1].chars().count();
        }
        self.ensure_cursor_visible();
    }

    /// Move cursor right
    fn move_right(&mut self) {
        self.clear_selection();
        let (line, col) = self.cursor;
        if line < self.lines.len() {
            let line_len = self.lines[line].chars().count();
            if col < line_len {
                self.cursor.1 = col + 1;
            } else if line + 1 < self.lines.len() {
                self.cursor = (line + 1, 0);
            }
        }
        self.ensure_cursor_visible();
    }

    /// Move cursor up
    fn move_up(&mut self) {
        self.clear_selection();
        let (line, col) = self.cursor;
        if line > 0 {
            self.cursor.0 = line - 1;
            let prev_line_len = self.lines[line - 1].chars().count();
            self.cursor.1 = col.min(prev_line_len);
        }
        self.ensure_cursor_visible();
    }

    /// Move cursor down
    fn move_down(&mut self) {
        self.clear_selection();
        let (line, col) = self.cursor;
        if line + 1 < self.lines.len() {
            self.cursor.0 = line + 1;
            let next_line_len = self.lines[line + 1].chars().count();
            self.cursor.1 = col.min(next_line_len);
        }
        self.ensure_cursor_visible();
    }

    /// Move cursor to start of line
    fn move_to_line_start(&mut self) {
        self.clear_selection();
        self.cursor.1 = 0;
    }

    /// Move cursor to end of line
    fn move_to_line_end(&mut self) {
        self.clear_selection();
        let (line, _) = self.cursor;
        if line < self.lines.len() {
            self.cursor.1 = self.lines[line].chars().count();
        }
    }

    /// Move cursor to start of buffer
    fn move_to_start(&mut self) {
        self.clear_selection();
        self.cursor = (0, 0);
        self.scroll_offset = 0;
    }

    /// Move cursor to end of buffer
    fn move_to_end(&mut self) {
        self.clear_selection();
        let last_line = self.lines.len().saturating_sub(1);
        self.cursor.0 = last_line;
        self.cursor.1 = self.lines[last_line].chars().count();
        self.ensure_cursor_visible();
    }

    /// Delete from cursor to end of line
    fn delete_to_line_end(&mut self) {
        let (line, col) = self.cursor;
        if line < self.lines.len() {
            let byte_pos = char_to_byte_pos(&self.lines[line], col);
            self.lines[line].truncate(byte_pos);
        }
    }

    /// Delete the entire current line
    fn delete_line(&mut self) {
        let (line, _) = self.cursor;
        if self.lines.len() > 1 {
            self.lines.remove(line);
            if line >= self.lines.len() {
                self.cursor.0 = self.lines.len() - 1;
            }
            let new_line_len = self.lines[self.cursor.0].chars().count();
            self.cursor.1 = self.cursor.1.min(new_line_len);
        } else {
            self.lines[0].clear();
            self.cursor.1 = 0;
        }
        self.ensure_cursor_visible();
    }

    /// Ensure the cursor line is visible (adjust scroll_offset)
    fn ensure_cursor_visible(&mut self) {
        let cursor_line = self.cursor.0 as u16;
        
        if cursor_line < self.scroll_offset {
            self.scroll_offset = cursor_line;
        } else if cursor_line >= self.scroll_offset + self.visible_height {
            self.scroll_offset = cursor_line.saturating_sub(self.visible_height.saturating_sub(1));
        }
    }

    /// Scroll up by n lines
    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down by n lines
    pub fn scroll_down(&mut self, n: u16) {
        let max_scroll = (self.lines.len() as u16).saturating_sub(self.visible_height);
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
    }

    // =====================================================================
    // Text Selection Support
    // =====================================================================

    /// Convert screen coordinates to logical text position (line_idx, char_col)
    /// Takes into account soft-wrapping and scroll offset
    fn screen_to_text_pos(&self, screen_x: u16, screen_y: u16) -> Option<(usize, usize)> {
        // Check if within inner area bounds
        if screen_x < self.inner_area.x || screen_y < self.inner_area.y {
            return None;
        }
        if screen_x >= self.inner_area.x + self.inner_area.width {
            return None;
        }
        if screen_y >= self.inner_area.y + self.inner_area.height {
            return None;
        }

        let rel_x = (screen_x - self.inner_area.x) as usize;
        let rel_y = (screen_y - self.inner_area.y) as usize;
        let wrap_width = self.inner_area.width as usize;

        if wrap_width == 0 {
            return None;
        }

        // Build visual line mapping (same logic as render)
        // Map visual row to (logical_line_idx, char_offset_in_line)
        let mut visual_row = 0;
        let scroll = self.scroll_offset as usize;

        for (line_idx, line) in self.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let num_visual_rows = if chars.is_empty() {
                1
            } else {
                chars.len().div_ceil(wrap_width)
            };

            // Check if target row is within this logical line's visual rows
            let target_row = scroll + rel_y;
            if target_row >= visual_row && target_row < visual_row + num_visual_rows {
                // Found the logical line
                let visual_offset_in_line = target_row - visual_row;
                let char_start = visual_offset_in_line * wrap_width;
                let char_col = (char_start + rel_x).min(chars.len());
                return Some((line_idx, char_col));
            }
            visual_row += num_visual_rows;
        }

        // Click was below all content - return end of last line
        if !self.lines.is_empty() {
            let last_line = self.lines.len() - 1;
            let last_col = self.lines[last_line].chars().count();
            return Some((last_line, last_col));
        }

        None
    }

    /// Start a selection at the given screen position
    pub fn start_selection(&mut self, screen_x: u16, screen_y: u16) {
        if let Some((line, col)) = self.screen_to_text_pos(screen_x, screen_y) {
            let pos = SelectionPos::new(line, col);
            self.selection = Some((pos, pos));
            self.selecting = true;
            // Also move cursor to selection start
            self.cursor = (line, col);
        }
    }

    /// Update selection end point during drag
    pub fn update_selection(&mut self, screen_x: u16, screen_y: u16) {
        if !self.selecting {
            return;
        }
        if let Some((line, col)) = self.screen_to_text_pos(screen_x, screen_y) {
            if let Some((start, _)) = self.selection {
                self.selection = Some((start, SelectionPos::new(line, col)));
                // Move cursor to selection end
                self.cursor = (line, col);
                self.ensure_cursor_visible();
            }
        }
    }

    /// End the selection (mouse up)
    pub fn end_selection(&mut self) {
        self.selecting = false;
    }

    /// Clear the current selection
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.selecting = false;
    }

    /// Check if there's an active selection
    pub fn has_selection(&self) -> bool {
        if let Some((start, end)) = self.selection {
            // Selection is valid if start != end
            start.line != end.line || start.col != end.col
        } else {
            false
        }
    }

    /// Check if currently in the middle of a drag selection
    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    /// Get normalized selection (start before end)
    fn normalized_selection(&self) -> Option<(SelectionPos, SelectionPos)> {
        let (start, end) = self.selection?;
        if start.line < end.line || (start.line == end.line && start.col <= end.col) {
            Some((start, end))
        } else {
            Some((end, start))
        }
    }

    /// Check if a position is within the selection
    fn is_position_selected(&self, line: usize, col: usize) -> bool {
        let (start, end) = match self.normalized_selection() {
            Some(s) => s,
            None => return false,
        };

        if line < start.line || line > end.line {
            return false;
        }

        if line == start.line && line == end.line {
            col >= start.col && col < end.col
        } else if line == start.line {
            col >= start.col
        } else if line == end.line {
            col < end.col
        } else {
            true
        }
    }

    /// Get the selected text
    pub fn get_selected_text(&self) -> Option<String> {
        let (start, end) = self.normalized_selection()?;

        if start.line == end.line {
            // Single line selection
            let line = &self.lines[start.line];
            let chars: Vec<char> = line.chars().collect();
            let start_col = start.col.min(chars.len());
            let end_col = end.col.min(chars.len());
            if start_col >= end_col {
                return None;
            }
            return Some(chars[start_col..end_col].iter().collect());
        }

        // Multi-line selection
        let mut result = String::new();

        for line_idx in start.line..=end.line {
            if line_idx >= self.lines.len() {
                break;
            }
            let line = &self.lines[line_idx];
            let chars: Vec<char> = line.chars().collect();

            if line_idx == start.line {
                let start_col = start.col.min(chars.len());
                result.push_str(&chars[start_col..].iter().collect::<String>());
            } else if line_idx == end.line {
                let end_col = end.col.min(chars.len());
                result.push('\n');
                result.push_str(&chars[..end_col].iter().collect::<String>());
            } else {
                result.push('\n');
                result.push_str(line);
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Delete the selected text and return true if something was deleted
    pub fn delete_selection(&mut self) -> bool {
        let (start, end) = match self.normalized_selection() {
            Some(s) => s,
            None => return false,
        };

        if start.line == end.line {
            // Single line deletion
            let line = &mut self.lines[start.line];
            let byte_start = char_to_byte_pos(line, start.col);
            let byte_end = char_to_byte_pos(line, end.col);
            line.replace_range(byte_start..byte_end, "");
        } else {
            // Multi-line deletion
            // Keep text before start on start line, text after end on end line
            let start_line = &self.lines[start.line];
            let end_line = &self.lines[end.line];

            let start_byte = char_to_byte_pos(start_line, start.col);
            let end_byte = char_to_byte_pos(end_line, end.col);

            let new_line = format!(
                "{}{}",
                &start_line[..start_byte],
                &end_line[end_byte..]
            );

            // Remove lines between start and end (inclusive of end), then replace start
            self.lines.drain(start.line + 1..=end.line);
            self.lines[start.line] = new_line;
        }

        // Move cursor to start of deleted region
        self.cursor = (start.line, start.col);
        self.clear_selection();
        self.ensure_cursor_visible();
        true
    }

    /// Handle mouse events for selection
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
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
                    Some(Action::ChatInputCopy)
                } else {
                    None
                }
            }
            // DIAGNOSTIC: Disable scroll handling to test if this is capturing events
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => None,
            _ => None,
        }
    }

    /// Handle key events for text editing
    /// Returns Some(Action::None) when the event was consumed but no dispatch is needed
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match (key.modifiers, key.code) {
            // Ctrl+J: insert newline (traditional Unix, reliable in WSL2)
            (mods, KeyCode::Char('j')) if mods.contains(KeyModifiers::CONTROL) => {
                self.insert_newline();
                Some(Action::None)
            }
            
            // Alt+Enter, Shift+Enter, or Ctrl+Enter: insert newline (must check BEFORE plain Enter)
            (mods, KeyCode::Enter) if mods.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::CONTROL) => {
                self.insert_newline();
                Some(Action::None) // Consumed, don't bubble up
            }
            
            // Enter: send message (more intuitive UX)
            (_, KeyCode::Enter) => {
                if !self.is_empty() {
                    let message = self.text();
                    self.clear();
                    return Some(Action::LlmSendMessage(message));
                }
                // Empty message: consume the event but do nothing
                Some(Action::None)
            }
            
            // Backspace
            (_, KeyCode::Backspace) => {
                self.delete_char_before();
                Some(Action::None)
            }
            
            // Delete
            (_, KeyCode::Delete) => {
                self.delete_char_at();
                Some(Action::None)
            }
            
            // Arrow keys
            (KeyModifiers::NONE, KeyCode::Left) => {
                self.move_left();
                Some(Action::None)
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                self.move_right();
                Some(Action::None)
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                self.move_up();
                Some(Action::None)
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                self.move_down();
                Some(Action::None)
            }
            
            // Home/End
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.move_to_line_start();
                Some(Action::None)
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                self.move_to_line_end();
                Some(Action::None)
            }
            (KeyModifiers::CONTROL, KeyCode::Home) => {
                self.move_to_start();
                Some(Action::None)
            }
            (KeyModifiers::CONTROL, KeyCode::End) => {
                self.move_to_end();
                Some(Action::None)
            }
            
            // Ctrl+A: start of line (emacs-style)
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.move_to_line_start();
                Some(Action::None)
            }
            
            // Ctrl+E: end of line (emacs-style)
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.move_to_line_end();
                Some(Action::None)
            }
            
            // Ctrl+K: delete to end of line
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                self.delete_to_line_end();
                Some(Action::None)
            }
            
            // Ctrl+U: delete entire line
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.delete_line();
                Some(Action::None)
            }

            // Ctrl+C: copy selected text (if there is a selection)
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.has_selection() {
                    Some(Action::ChatInputCopy)
                } else {
                    None // Let it bubble up (e.g., for SIGINT handling)
                }
            }

            // Regular character input
            (KeyModifiers::NONE, KeyCode::Char(c)) 
            | (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                self.insert_char(c);
                Some(Action::None)
            }
            
            // Tab key should cycle focus, not insert spaces
            // Let it bubble up to the global handler
            (KeyModifiers::NONE, KeyCode::Tab) => {
                None // Let Tab bubble up for focus cycling
            }
            
            _ => None, // Unhandled keys bubble up to global handlers
        }
    }

    /// Render the chat input widget
    pub fn render_input(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        self.set_visible_height_internal(area.height);
        
        let border_style = if focused {
            Style::default().fg(theme.focus.focused_border.to_color())
        } else {
            Style::default().fg(theme.focus.unfocused_border.to_color())
        };

        let title_style = if focused {
            Style::default()
                .fg(theme.focus.focused_title.to_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.focus.unfocused_title.to_color())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Message ", title_style));

        let inner_area = block.inner(area);
        let wrap_width = inner_area.width as usize;
        
        if wrap_width == 0 {
            frame.render_widget(block, area);
            return;
        }
        
        // Style definitions
        let normal_style = Style::default().fg(theme.colors.foreground.to_color());
        let selection_style = Style::default()
            .fg(theme.colors.background.to_color())
            .bg(theme.colors.primary.to_color());
        let cursor_style = Style::default()
            .fg(theme.colors.background.to_color())
            .bg(theme.colors.foreground.to_color());

        // Build visual lines with soft-wrapping
        // Track: (visual_line, logical_line_idx, char_start_offset)
        let mut visual_lines: Vec<(Line, usize, usize)> = Vec::new();
        let mut cursor_visual_row: Option<usize> = None;
        let mut cursor_visual_col: usize = 0;

        for (line_idx, line) in self.lines.iter().enumerate() {
            let is_cursor_line = line_idx == self.cursor.0;
            let chars: Vec<char> = line.chars().collect();

            if chars.is_empty() {
                // Empty line - still takes up one visual row
                if is_cursor_line {
                    cursor_visual_row = Some(visual_lines.len());
                    cursor_visual_col = 0;
                }
                visual_lines.push((Line::from(" "), line_idx, 0));
            } else {
                // Wrap the line into chunks of wrap_width
                let mut char_offset = 0;
                while char_offset < chars.len() {
                    let chunk_end = (char_offset + wrap_width).min(chars.len());

                    // Check if cursor is in this visual line
                    if is_cursor_line {
                        let cursor_col = self.cursor.1;
                        if cursor_col >= char_offset && cursor_col < chunk_end {
                            cursor_visual_row = Some(visual_lines.len());
                            cursor_visual_col = cursor_col - char_offset;
                        } else if cursor_col == chars.len() && chunk_end == chars.len() {
                            // Cursor at end of line
                            cursor_visual_row = Some(visual_lines.len());
                            cursor_visual_col = chunk_end - char_offset;
                        }
                    }

                    // Build spans for this chunk, character by character with selection highlighting
                    let mut spans: Vec<Span> = Vec::new();
                    let mut current_run = String::new();
                    let mut current_selected = self.is_position_selected(line_idx, char_offset);

                    for (i, &c) in chars[char_offset..chunk_end].iter().enumerate() {
                        let char_col = char_offset + i;
                        let is_selected = self.is_position_selected(line_idx, char_col);

                        if is_selected != current_selected {
                            // Flush current run
                            if !current_run.is_empty() {
                                let style = if current_selected { selection_style } else { normal_style };
                                spans.push(Span::styled(current_run.clone(), style));
                                current_run.clear();
                            }
                            current_selected = is_selected;
                        }
                        current_run.push(c);
                    }

                    // Flush remaining run
                    if !current_run.is_empty() {
                        let style = if current_selected { selection_style } else { normal_style };
                        spans.push(Span::styled(current_run, style));
                    }

                    visual_lines.push((Line::from(spans), line_idx, char_offset));
                    char_offset = chunk_end;
                }
            }
        }

        // Calculate scroll to keep cursor visible
        let total_visual_lines = visual_lines.len();
        let visible_height = inner_area.height as usize;

        let scroll_offset = if let Some(cursor_row) = cursor_visual_row {
            if cursor_row < self.scroll_offset as usize {
                cursor_row
            } else if cursor_row >= self.scroll_offset as usize + visible_height {
                cursor_row.saturating_sub(visible_height - 1)
            } else {
                self.scroll_offset as usize
            }
        } else {
            self.scroll_offset as usize
        };

        // Build the final display with cursor overlay
        let visible_start = scroll_offset;
        let visible_end = (scroll_offset + visible_height).min(total_visual_lines);

        let mut display_lines: Vec<Line> = Vec::new();

        for (visual_idx, (vline, line_idx, char_start)) in visual_lines.iter().enumerate().skip(visible_start).take(visible_end - visible_start) {
            let is_cursor_row = cursor_visual_row == Some(visual_idx) && focused;

            if is_cursor_row {
                // Render this line with cursor overlay on top of selection
                let text: String = vline.spans.iter().map(|s| s.content.as_ref()).collect();
                let chars: Vec<char> = text.chars().collect();
                let mut spans = Vec::new();

                for (i, &c) in chars.iter().enumerate() {
                    let char_col = char_start + i;
                    let is_cursor = i == cursor_visual_col;
                    let is_selected = self.is_position_selected(*line_idx, char_col);

                    let style = if is_cursor {
                        cursor_style
                    } else if is_selected {
                        selection_style
                    } else {
                        normal_style
                    };
                    spans.push(Span::styled(c.to_string(), style));
                }

                // Cursor at end of line - add cursor block
                if cursor_visual_col >= chars.len() {
                    spans.push(Span::styled(" ", cursor_style));
                }

                display_lines.push(Line::from(spans));
            } else {
                display_lines.push(vline.clone());
            }
        }

        let paragraph = Paragraph::new(display_lines)
            .block(block);

        frame.render_widget(paragraph, area);

        // Render scrollbar if content exceeds visible area
        if total_visual_lines > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            let mut scrollbar_state = ScrollbarState::new(total_visual_lines)
                .position(scroll_offset);
            
            frame.render_stateful_widget(
                scrollbar,
                area,
                &mut scrollbar_state,
            );
        }
    }

    // Internal method to update visible height without &mut self (for render)
    fn set_visible_height_internal(&self, height: u16) {
        // This is a workaround - in practice the caller should set this
        // before rendering via set_visible_height()
        let _ = height;
    }
}

impl Default for ChatInput {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ChatInput {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.handle_key(*key),
            _ => None,
        }
    }

    fn update(&mut self, _action: &Action) {
        // ChatInput doesn't respond to external actions currently
        // Future: could handle paste, etc.
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        self.render_input(frame, area, focused, theme);
    }
}

/// Convert a character index to a byte index in a string
fn char_to_byte_pos(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_chat_input() {
        let input = ChatInput::new();
        assert!(input.is_empty());
        assert_eq!(input.line_count(), 1);
        assert_eq!(input.text(), "");
    }

    #[test]
    fn test_insert_char() {
        let mut input = ChatInput::new();
        input.insert_char('H');
        input.insert_char('i');
        assert_eq!(input.text(), "Hi");
        assert_eq!(input.cursor, (0, 2));
    }

    #[test]
    fn test_insert_newline() {
        let mut input = ChatInput::new();
        input.insert_char('a');
        input.insert_newline();
        input.insert_char('b');
        assert_eq!(input.text(), "a\nb");
        assert_eq!(input.line_count(), 2);
        assert_eq!(input.cursor, (1, 1));
    }

    #[test]
    fn test_backspace() {
        let mut input = ChatInput::new();
        input.insert_char('a');
        input.insert_char('b');
        input.delete_char_before();
        assert_eq!(input.text(), "a");
        assert_eq!(input.cursor, (0, 1));
    }

    #[test]
    fn test_backspace_joins_lines() {
        let mut input = ChatInput::new();
        input.insert_char('a');
        input.insert_newline();
        input.insert_char('b');
        input.move_to_line_start();
        input.delete_char_before();
        assert_eq!(input.text(), "ab");
        assert_eq!(input.line_count(), 1);
    }

    #[test]
    fn test_cursor_movement() {
        let mut input = ChatInput::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        
        input.move_left();
        assert_eq!(input.cursor, (0, 2));
        
        input.move_to_line_start();
        assert_eq!(input.cursor, (0, 0));
        
        input.move_to_line_end();
        assert_eq!(input.cursor, (0, 3));
    }

    #[test]
    fn test_clear() {
        let mut input = ChatInput::new();
        input.insert_char('a');
        input.insert_newline();
        input.insert_char('b');
        input.clear();
        
        assert!(input.is_empty());
        assert_eq!(input.cursor, (0, 0));
    }

    #[test]
    fn test_delete_line() {
        let mut input = ChatInput::new();
        input.insert_char('a');
        input.insert_newline();
        input.insert_char('b');
        input.insert_newline();
        input.insert_char('c');
        
        // Move to middle line and delete it
        input.move_up();
        input.delete_line();
        
        assert_eq!(input.text(), "a\nc");
        assert_eq!(input.line_count(), 2);
    }

    #[test]
    fn test_unicode_handling() {
        let mut input = ChatInput::new();
        input.insert_char('日');
        input.insert_char('本');
        input.insert_char('語');
        assert_eq!(input.text(), "日本語");
        assert_eq!(input.cursor, (0, 3));
        
        input.delete_char_before();
        assert_eq!(input.text(), "日本");
    }
}
