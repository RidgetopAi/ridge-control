#![allow(dead_code)]

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
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
}

impl ChatInput {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: (0, 0),
            scroll_offset: 0,
            visible_height: 5,
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
    }

    /// Set the visible height for scroll calculations
    pub fn set_visible_height(&mut self, height: u16) {
        self.visible_height = height.saturating_sub(2); // Account for borders
    }

    /// Get line count
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Insert a character at the cursor position
    fn insert_char(&mut self, c: char) {
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
        self.cursor.1 = 0;
    }

    /// Move cursor to end of line
    fn move_to_line_end(&mut self) {
        let (line, _) = self.cursor;
        if line < self.lines.len() {
            self.cursor.1 = self.lines[line].chars().count();
        }
    }

    /// Move cursor to start of buffer
    fn move_to_start(&mut self) {
        self.cursor = (0, 0);
        self.scroll_offset = 0;
    }

    /// Move cursor to end of buffer
    fn move_to_end(&mut self) {
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
    fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down by n lines
    fn scroll_down(&mut self, n: u16) {
        let max_scroll = (self.lines.len() as u16).saturating_sub(self.visible_height);
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
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
        
        // Build the display lines with cursor
        let mut display_lines: Vec<Line> = Vec::new();
        let visible_start = self.scroll_offset as usize;
        let visible_end = (self.scroll_offset as usize + inner_area.height as usize).min(self.lines.len());

        for (line_idx, line) in self.lines.iter().enumerate().skip(visible_start).take(visible_end - visible_start) {
            let is_cursor_line = line_idx == self.cursor.0;
            
            if is_cursor_line && focused {
                // Build line with cursor indicator
                let cursor_col = self.cursor.1;
                let chars: Vec<char> = line.chars().collect();
                
                let mut spans = Vec::new();
                
                // Text before cursor
                if cursor_col > 0 {
                    let before: String = chars[..cursor_col.min(chars.len())].iter().collect();
                    spans.push(Span::styled(before, Style::default().fg(theme.colors.foreground.to_color())));
                }
                
                // Cursor character (or space if at end)
                if cursor_col < chars.len() {
                    let cursor_char = chars[cursor_col].to_string();
                    spans.push(Span::styled(
                        cursor_char,
                        Style::default()
                            .fg(theme.colors.background.to_color())
                            .bg(theme.colors.primary.to_color())
                    ));
                } else {
                    // Cursor at end of line - show block cursor
                    spans.push(Span::styled(
                        " ",
                        Style::default()
                            .fg(theme.colors.background.to_color())
                            .bg(theme.colors.primary.to_color())
                    ));
                }
                
                // Text after cursor
                if cursor_col + 1 < chars.len() {
                    let after: String = chars[cursor_col + 1..].iter().collect();
                    spans.push(Span::styled(after, Style::default().fg(theme.colors.foreground.to_color())));
                }
                
                display_lines.push(Line::from(spans));
            } else {
                // For empty lines, use a single space to ensure line takes up space
                let display_text = if line.is_empty() { " " } else { line.as_str() };
                display_lines.push(Line::styled(
                    display_text.to_string(),
                    Style::default().fg(theme.colors.foreground.to_color()),
                ));
            }
        }

        let paragraph = Paragraph::new(display_lines)
            .block(block);

        frame.render_widget(paragraph, area);

        // Render scrollbar if content exceeds visible area
        if self.lines.len() as u16 > inner_area.height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            let mut scrollbar_state = ScrollbarState::new(self.lines.len())
                .position(self.scroll_offset as usize);
            
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
