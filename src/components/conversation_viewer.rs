use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use crate::action::Action;
use crate::components::spinner::{Spinner, SpinnerStyle};
use crate::components::Component;
use crate::config::Theme;
use crate::llm::{ContentBlock, Message, Role, ToolUse, ToolResult};

/// Displays LLM conversation history with streaming response support
pub struct ConversationViewer {
    scroll_offset: u16,
    line_count: usize,
    visible_height: u16,
    auto_scroll: bool,
    inner_area: Rect,
    streaming_spinner: Spinner,
}

impl ConversationViewer {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            line_count: 0,
            visible_height: 10,
            auto_scroll: true,
            inner_area: Rect::default(),
            streaming_spinner: Spinner::new(SpinnerStyle::BrailleDots),
        }
    }
    
    pub fn tick_spinner(&mut self) {
        self.streaming_spinner.tick();
    }

    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = area;
        self.visible_height = area.height;
    }

    /// Render conversation with messages and current streaming buffer
    pub fn render_conversation(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        focused: bool,
        messages: &[Message],
        streaming_buffer: &str,
        theme: &Theme,
    ) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let title = if streaming_buffer.is_empty() {
            " Conversation ".to_string()
        } else {
            format!(" {} Streaming... ", self.streaming_spinner.current_frame())
        };

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        self.visible_height = inner.height;

        // Build lines from conversation
        let mut lines: Vec<Line> = Vec::new();

        for message in messages {
            // Add role header
            let (role_text, role_style) = match message.role {
                Role::User => (
                    "󰀄 User",
                    Style::default()
                        .fg(theme.colors.primary.to_color())
                        .add_modifier(Modifier::BOLD),
                ),
                Role::Assistant => (
                    "󰚩 Assistant",
                    Style::default()
                        .fg(theme.colors.secondary.to_color())
                        .add_modifier(Modifier::BOLD),
                ),
            };
            lines.push(Line::from(Span::styled(role_text, role_style)));

            // Add content blocks
            for block in &message.content {
                match block {
                    ContentBlock::Text(text) => {
                        for line in text.lines() {
                            lines.push(Line::from(Span::styled(
                                format!("  {}", line),
                                Style::default().fg(theme.colors.foreground.to_color()),
                            )));
                        }
                    }
                    ContentBlock::Thinking(text) => {
                        lines.push(Line::from(Span::styled(
                            "  󰔡 Thinking:",
                            Style::default()
                                .fg(theme.colors.accent.to_color())
                                .add_modifier(Modifier::ITALIC),
                        )));
                        for line in text.lines() {
                            lines.push(Line::from(Span::styled(
                                format!("    {}", line),
                                Style::default()
                                    .fg(theme.colors.muted.to_color())
                                    .add_modifier(Modifier::ITALIC),
                            )));
                        }
                    }
                    ContentBlock::ToolUse(tool) => {
                        lines.extend(self.render_tool_use(tool, theme));
                    }
                    ContentBlock::ToolResult(result) => {
                        lines.extend(self.render_tool_result(result, theme));
                    }
                    ContentBlock::Image(_) => {
                        lines.push(Line::from(Span::styled(
                            "  [Image]",
                            Style::default().fg(theme.colors.muted.to_color()),
                        )));
                    }
                }
            }

            // Add spacing between messages
            lines.push(Line::from(""));
        }

        // Add streaming buffer if present
        if !streaming_buffer.is_empty() {
            lines.push(Line::from(Span::styled(
                "󰚩 Assistant",
                Style::default()
                    .fg(theme.colors.secondary.to_color())
                    .add_modifier(Modifier::BOLD),
            )));
            for line in streaming_buffer.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(theme.colors.foreground.to_color()),
                )));
            }
            // Add cursor indicator for streaming
            if let Some(last) = lines.last_mut() {
                last.spans.push(Span::styled(
                    "▌",
                    Style::default()
                        .fg(theme.colors.accent.to_color())
                        .add_modifier(Modifier::SLOW_BLINK),
                ));
            }
        }

        self.line_count = lines.len();

        // Auto-scroll to bottom if enabled and new content
        if self.auto_scroll && self.line_count > self.visible_height as usize {
            self.scroll_offset = (self.line_count - self.visible_height as usize) as u16;
        }

        // Handle empty state
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No messages yet. Send a message to start a conversation.",
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        frame.render_widget(paragraph, area);

        // Render scrollbar if content exceeds visible area
        if self.line_count > self.visible_height as usize {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            let mut scrollbar_state = ScrollbarState::new(self.line_count)
                .position(self.scroll_offset as usize)
                .viewport_content_length(self.visible_height as usize);

            frame.render_stateful_widget(
                scrollbar,
                inner,
                &mut scrollbar_state,
            );
        }
    }

    fn render_tool_use(&self, tool: &ToolUse, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(
                "  󰒓 Tool: ",
                Style::default()
                    .fg(theme.colors.warning.to_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                tool.name.clone(),
                Style::default().fg(theme.colors.accent.to_color()),
            ),
        ]));

        // Pretty-print JSON input (truncated if too long)
        let input_str = serde_json::to_string_pretty(&tool.input)
            .unwrap_or_else(|_| tool.input.to_string());
        
        let max_lines = 10;
        let input_lines: Vec<&str> = input_str.lines().collect();
        let truncated = input_lines.len() > max_lines;
        
        for line in input_lines.iter().take(max_lines) {
            lines.push(Line::from(Span::styled(
                format!("    {}", line),
                Style::default().fg(theme.colors.muted.to_color()),
            )));
        }

        if truncated {
            lines.push(Line::from(Span::styled(
                format!("    ... ({} more lines)", input_lines.len() - max_lines),
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        lines
    }

    fn render_tool_result(&self, result: &ToolResult, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        let (icon, color) = if result.is_error {
            ("󰅚", theme.colors.error.to_color())
        } else {
            ("󰄬", theme.colors.success.to_color())
        };

        lines.push(Line::from(Span::styled(
            format!("  {} Tool Result", icon),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));

        // Render result content
        let content_str = match &result.content {
            crate::llm::ToolResultContent::Text(text) => text.clone(),
            crate::llm::ToolResultContent::Json(json) => {
                serde_json::to_string_pretty(json).unwrap_or_else(|_| json.to_string())
            }
            crate::llm::ToolResultContent::Image(_) => "[Image result]".to_string(),
        };

        let max_lines = 8;
        let content_lines: Vec<&str> = content_str.lines().collect();
        let truncated = content_lines.len() > max_lines;

        for line in content_lines.iter().take(max_lines) {
            lines.push(Line::from(Span::styled(
                format!("    {}", line),
                Style::default().fg(if result.is_error {
                    theme.colors.error.to_color()
                } else {
                    theme.colors.foreground.to_color()
                }),
            )));
        }

        if truncated {
            lines.push(Line::from(Span::styled(
                format!("    ... ({} more lines)", content_lines.len() - max_lines),
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        lines
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
        // Re-enable auto-scroll if at bottom
        if self.line_count <= self.visible_height as usize
            || self.scroll_offset as usize >= self.line_count - self.visible_height as usize
        {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll_offset = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
        if self.line_count > self.visible_height as usize {
            self.scroll_offset = (self.line_count - self.visible_height as usize) as u16;
        }
    }

    pub fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = !self.auto_scroll;
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn is_auto_scroll(&self) -> bool {
        self.auto_scroll
    }
}

impl Default for ConversationViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ConversationViewer {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.handle_key(*key),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::ScrollUp(n) => self.scroll_up(*n),
            Action::ScrollDown(n) => self.scroll_down(*n),
            Action::ScrollToTop => self.scroll_to_top(),
            Action::ScrollToBottom => self.scroll_to_bottom(),
            _ => {}
        }
    }

    fn render(&self, _frame: &mut Frame, _area: Rect, _focused: bool, _theme: &Theme) {
        // Use render_conversation() instead for full functionality
    }
}

impl ConversationViewer {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp(1)),
            KeyCode::Char('g') => Some(Action::ScrollToTop),
            KeyCode::Char('G') => Some(Action::ScrollToBottom),
            KeyCode::PageUp => Some(Action::ScrollPageUp),
            KeyCode::PageDown => Some(Action::ScrollPageDown),
            KeyCode::Char('a') => {
                self.toggle_auto_scroll();
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_viewer_new() {
        let viewer = ConversationViewer::new();
        assert_eq!(viewer.scroll_offset, 0);
        assert!(viewer.auto_scroll);
    }

    #[test]
    fn test_scroll_operations() {
        let mut viewer = ConversationViewer::new();
        viewer.line_count = 100;
        viewer.visible_height = 20;

        // First scroll up to disable auto_scroll
        viewer.scroll_up(5);
        assert_eq!(viewer.scroll_offset, 0); // Saturates at 0
        assert!(!viewer.auto_scroll); // scroll_up disables auto_scroll

        // Now scroll down
        viewer.scroll_down(10);
        assert_eq!(viewer.scroll_offset, 10);

        viewer.scroll_to_bottom();
        assert!(viewer.auto_scroll);
        assert_eq!(viewer.scroll_offset, 80); // 100 - 20
    }

    #[test]
    fn test_toggle_auto_scroll() {
        let mut viewer = ConversationViewer::new();
        assert!(viewer.is_auto_scroll());

        viewer.toggle_auto_scroll();
        assert!(!viewer.is_auto_scroll());

        viewer.toggle_auto_scroll();
        assert!(viewer.is_auto_scroll());
    }
}
