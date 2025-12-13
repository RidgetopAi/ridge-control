use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use crate::action::Action;
use crate::components::search::{SearchState, SearchBar, SearchAction};
use crate::components::spinner::{Spinner, SpinnerStyle};
use crate::components::tool_call_widget::{ToolCallManager, ToolCallWidget, ToolStatus};
use crate::components::Component;
use crate::config::Theme;
use crate::llm::{ContentBlock, Message, Role, ToolUse, ToolResult};

/// Displays LLM conversation history with streaming response support and tool call management
pub struct ConversationViewer {
    scroll_offset: u16,
    line_count: usize,
    visible_height: u16,
    auto_scroll: bool,
    inner_area: Rect,
    streaming_spinner: Spinner,
    tool_spinner: Spinner,
    thinking_spinner: Spinner,
    tool_call_manager: ToolCallManager,
    /// Whether we're in tool call navigation mode
    tool_navigation_mode: bool,
    /// Whether thinking blocks are collapsed (TRC-017)
    thinking_collapsed: bool,
    /// Search state (TRC-021)
    search_state: SearchState,
    /// Cached text lines for search
    cached_text: Vec<String>,
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
            tool_spinner: Spinner::new(SpinnerStyle::Braille),
            thinking_spinner: Spinner::new(SpinnerStyle::DigitalDots),
            tool_call_manager: ToolCallManager::new(),
            tool_navigation_mode: false,
            thinking_collapsed: false,
            search_state: SearchState::new(),
            cached_text: Vec::new(),
        }
    }
    
    pub fn tick_spinner(&mut self) {
        self.streaming_spinner.tick();
        self.tool_spinner.tick();
        self.thinking_spinner.tick();
    }
    
    /// Toggle thinking block collapse state (TRC-017)
    pub fn toggle_thinking_collapse(&mut self) {
        self.thinking_collapsed = !self.thinking_collapsed;
    }
    
    /// Get thinking collapse state (TRC-017)
    pub fn is_thinking_collapsed(&self) -> bool {
        self.thinking_collapsed
    }
    
    /// Set thinking collapse state (TRC-017)
    pub fn set_thinking_collapsed(&mut self, collapsed: bool) {
        self.thinking_collapsed = collapsed;
    }

    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = area;
        self.visible_height = area.height;
    }
    
    /// Get the tool call manager for external access
    pub fn tool_call_manager(&self) -> &ToolCallManager {
        &self.tool_call_manager
    }
    
    /// Get mutable tool call manager
    pub fn tool_call_manager_mut(&mut self) -> &mut ToolCallManager {
        &mut self.tool_call_manager
    }
    
    /// Register a new tool use from LLM
    pub fn register_tool_use(&mut self, tool_use: ToolUse) {
        self.tool_call_manager.add_tool_call(tool_use);
    }
    
    /// Start execution of a tool by ID
    pub fn start_tool_execution(&mut self, tool_id: &str) {
        self.tool_call_manager.start_execution(tool_id);
    }
    
    /// Complete a tool with result
    pub fn complete_tool(&mut self, tool_id: &str, result: ToolResult) {
        self.tool_call_manager.complete_tool(tool_id, result);
    }
    
    /// Reject a tool
    pub fn reject_tool(&mut self, tool_id: &str) {
        self.tool_call_manager.reject_tool(tool_id);
    }
    
    /// Check if there's a pending tool that needs confirmation
    pub fn has_pending_tool(&self) -> bool {
        self.tool_call_manager.has_pending()
    }
    
    /// Check if there's a running tool
    pub fn has_running_tool(&self) -> bool {
        self.tool_call_manager.has_running()
    }
    
    /// Toggle tool call navigation mode
    pub fn toggle_tool_navigation(&mut self) {
        self.tool_navigation_mode = !self.tool_navigation_mode;
    }
    
    /// Check if in tool navigation mode
    pub fn is_tool_navigation_mode(&self) -> bool {
        self.tool_navigation_mode
    }
    
    /// Clear all tool calls (e.g., when conversation is cleared)
    pub fn clear_tool_calls(&mut self) {
        self.tool_call_manager.clear();
    }

    /// Search state accessor (TRC-021)
    pub fn is_search_active(&self) -> bool {
        self.search_state.is_active()
    }

    /// Get search state (TRC-021)
    pub fn search_state(&self) -> &SearchState {
        &self.search_state
    }

    /// Get mutable search state (TRC-021)
    pub fn search_state_mut(&mut self) -> &mut SearchState {
        &mut self.search_state
    }

    /// Start search in conversation (TRC-021)
    pub fn start_search(&mut self) {
        self.search_state.activate();
        self.auto_scroll = false;
    }

    /// Close search (TRC-021)
    pub fn close_search(&mut self) {
        self.search_state.deactivate();
    }

    /// Update search with current cached text (TRC-021)
    pub fn update_search(&mut self) {
        self.search_state.search_in_lines(
            self.cached_text.iter().enumerate().map(|(idx, line)| (idx, line.as_str()))
        );
    }

    /// Navigate to next search match (TRC-021)
    pub fn search_next(&mut self) {
        self.search_state.next_match();
        self.scroll_to_current_search_match();
    }

    /// Navigate to previous search match (TRC-021)
    pub fn search_prev(&mut self) {
        self.search_state.prev_match();
        self.scroll_to_current_search_match();
    }

    fn scroll_to_current_search_match(&mut self) {
        if let Some(m) = self.search_state.current_match() {
            let target_line = m.line_index as u16;
            if target_line < self.scroll_offset {
                self.scroll_offset = target_line.saturating_sub(2);
            } else if target_line >= self.scroll_offset + self.visible_height {
                self.scroll_offset = target_line.saturating_sub(self.visible_height / 2);
            }
        }
    }

    /// Cache text content for search (TRC-021)
    fn cache_text_for_search(&mut self, messages: &[Message], streaming_buffer: &str, thinking_buffer: &str) {
        self.cached_text.clear();
        
        for message in messages {
            let role_text = match message.role {
                Role::User => "User:",
                Role::Assistant => "Assistant:",
            };
            self.cached_text.push(role_text.to_string());
            
            for content_block in &message.content {
                match content_block {
                    ContentBlock::Text(text) => {
                        for line in text.lines() {
                            self.cached_text.push(line.to_string());
                        }
                    }
                    ContentBlock::Thinking(text) => {
                        for line in text.lines() {
                            self.cached_text.push(line.to_string());
                        }
                    }
                    ContentBlock::ToolUse(tool) => {
                        self.cached_text.push(format!("Tool: {}", tool.name));
                    }
                    ContentBlock::ToolResult(result) => {
                        let content_str = match &result.content {
                            crate::llm::ToolResultContent::Text(text) => text.clone(),
                            crate::llm::ToolResultContent::Json(json) => json.to_string(),
                            crate::llm::ToolResultContent::Image(_) => "[Image]".to_string(),
                        };
                        for line in content_str.lines() {
                            self.cached_text.push(line.to_string());
                        }
                    }
                    ContentBlock::Image(_) => {
                        self.cached_text.push("[Image]".to_string());
                    }
                }
            }
        }
        
        if !thinking_buffer.is_empty() {
            for line in thinking_buffer.lines() {
                self.cached_text.push(line.to_string());
            }
        }
        
        if !streaming_buffer.is_empty() {
            for line in streaming_buffer.lines() {
                self.cached_text.push(line.to_string());
            }
        }
    }

    /// Render conversation with messages and current streaming buffers
    /// TRC-017: Now accepts separate thinking_buffer for extended thinking display
    /// TRC-021: Added search support
    pub fn render_conversation(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        focused: bool,
        messages: &[Message],
        streaming_buffer: &str,
        thinking_buffer: &str,
        theme: &Theme,
    ) {
        self.cache_text_for_search(messages, streaming_buffer, thinking_buffer);

        let (conversation_area, search_area) = if self.search_state.is_active() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(SearchBar::height()),
                ])
                .split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };

        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        // Build title with status indicators (TRC-017: include thinking indicator, TRC-021: search)
        let title = self.build_title(streaming_buffer, thinking_buffer);

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(conversation_area);
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
            for content_block in &message.content {
                match content_block {
                    ContentBlock::Text(text) => {
                        for line in text.lines() {
                            lines.push(Line::from(Span::styled(
                                format!("  {}", line),
                                Style::default().fg(theme.colors.foreground.to_color()),
                            )));
                        }
                    }
                    ContentBlock::Thinking(text) => {
                        // TRC-017: Collapsible thinking blocks
                        lines.extend(self.render_thinking_block(text, theme));
                    }
                    ContentBlock::ToolUse(tool) => {
                        lines.extend(self.render_tool_use_enhanced(tool, theme));
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

        // TRC-017: Add streaming thinking buffer if present (before text buffer)
        if !thinking_buffer.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", self.thinking_spinner.current_frame()),
                    Style::default()
                        .fg(theme.colors.accent.to_color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Thinking...",
                    Style::default()
                        .fg(theme.colors.accent.to_color())
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
            
            // Show thinking content if not collapsed
            if !self.thinking_collapsed {
                // Count lines for summary
                let thinking_lines: Vec<&str> = thinking_buffer.lines().collect();
                let max_thinking_lines = 20;
                let truncated = thinking_lines.len() > max_thinking_lines;
                
                for line in thinking_lines.iter().take(max_thinking_lines) {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", line),
                        Style::default()
                            .fg(theme.colors.muted.to_color())
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
                
                if truncated {
                    lines.push(Line::from(Span::styled(
                        format!("    ... ({} more lines - press 'T' to toggle)", thinking_lines.len() - max_thinking_lines),
                        Style::default()
                            .fg(theme.colors.muted.to_color())
                            .add_modifier(Modifier::DIM),
                    )));
                }
            } else {
                // Show collapsed summary
                let line_count = thinking_buffer.lines().count();
                let char_count = thinking_buffer.len();
                lines.push(Line::from(Span::styled(
                    format!("    [Collapsed: {} lines, {} chars - press 'T' to expand]", line_count, char_count),
                    Style::default()
                        .fg(theme.colors.muted.to_color())
                        .add_modifier(Modifier::DIM),
                )));
            }
            
            lines.push(Line::from("")); // Spacing after thinking
        }

        // Add streaming buffer if present
        if !streaming_buffer.is_empty() {
            // Only add assistant header if we don't have streaming thinking
            if thinking_buffer.is_empty() {
                lines.push(Line::from(Span::styled(
                    "󰚩 Assistant",
                    Style::default()
                        .fg(theme.colors.secondary.to_color())
                        .add_modifier(Modifier::BOLD),
                )));
            }
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

        frame.render_widget(paragraph, conversation_area);

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

        // TRC-021: Render search bar if active
        if let Some(search_rect) = search_area {
            let search_bar = SearchBar::new(&self.search_state, theme);
            search_bar.render(frame, search_rect);
        }
    }
    
    /// TRC-017: Updated to include thinking buffer indicator
    /// TRC-021: Added search indicator
    fn build_title(&self, streaming_buffer: &str, thinking_buffer: &str) -> String {
        let mut title_parts = vec![" Conversation".to_string()];
        
        // Add tool status indicators
        let tool_count = self.tool_call_manager.len();
        if tool_count > 0 {
            let pending = self.tool_call_manager.tool_calls().iter()
                .filter(|tc| tc.status == ToolStatus::Pending)
                .count();
            let running = self.tool_call_manager.tool_calls().iter()
                .filter(|tc| tc.status == ToolStatus::Running)
                .count();
            
            if pending > 0 {
                title_parts.push(format!(" [⏳{}]", pending));
            }
            if running > 0 {
                title_parts.push(format!(" [{}{}]", self.tool_spinner.current_frame(), running));
            }
        }
        
        // TRC-017: Add thinking indicator
        if !thinking_buffer.is_empty() {
            let collapse_indicator = if self.thinking_collapsed { "▶" } else { "▼" };
            title_parts.push(format!(" {} 󰔡 Thinking", collapse_indicator));
        }
        
        // Add streaming indicator
        if !streaming_buffer.is_empty() {
            title_parts.push(format!(" {} Streaming...", self.streaming_spinner.current_frame()));
        }

        // TRC-021: Add search indicator
        if self.search_state.is_active() {
            title_parts.push(" 󰍉".to_string());
        }
        
        title_parts.push(" ".to_string());
        title_parts.join("")
    }
    
    /// TRC-017: Render a thinking block (collapsible)
    fn render_thinking_block(&self, text: &str, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        
        let collapse_indicator = if self.thinking_collapsed { "▶" } else { "▼" };
        
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", collapse_indicator),
                Style::default()
                    .fg(theme.colors.accent.to_color()),
            ),
            Span::styled(
                "󰔡 Thinking",
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
        
        if !self.thinking_collapsed {
            // Show thinking content
            let thinking_lines: Vec<&str> = text.lines().collect();
            let max_lines = 30;
            let truncated = thinking_lines.len() > max_lines;
            
            for line in thinking_lines.iter().take(max_lines) {
                lines.push(Line::from(Span::styled(
                    format!("    {}", line),
                    Style::default()
                        .fg(theme.colors.muted.to_color())
                        .add_modifier(Modifier::ITALIC),
                )));
            }
            
            if truncated {
                lines.push(Line::from(Span::styled(
                    format!("    ... ({} more lines)", thinking_lines.len() - max_lines),
                    Style::default()
                        .fg(theme.colors.muted.to_color())
                        .add_modifier(Modifier::DIM),
                )));
            }
        } else {
            // Show collapsed summary
            let line_count = text.lines().count();
            let char_count = text.len();
            lines.push(Line::from(Span::styled(
                format!("    [Collapsed: {} lines, {} chars - press 'T' to expand]", line_count, char_count),
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::DIM),
            )));
        }
        
        lines
    }

    /// Render a tool use with enhanced UI using ToolCallWidget
    fn render_tool_use_enhanced(&self, tool: &ToolUse, theme: &Theme) -> Vec<Line<'static>> {
        // Try to find the tool call in our manager for state info
        if let Some(tracked_tool) = self.tool_call_manager.get(&tool.id) {
            let is_selected = self.tool_call_manager.selected()
                .map(|s| s.tool_id() == tool.id)
                .unwrap_or(false);
            
            let widget = ToolCallWidget::new(tracked_tool, theme)
                .with_spinner(&self.tool_spinner)
                .selected(is_selected);
            
            widget.render_lines()
        } else {
            // Fallback to simple rendering if tool isn't tracked
            self.render_tool_use_simple(tool, theme)
        }
    }
    
    /// Simple tool use rendering (fallback)
    fn render_tool_use_simple(&self, tool: &ToolUse, theme: &Theme) -> Vec<Line<'static>> {
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
    
    /// Select next tool call (for navigation)
    pub fn select_next_tool(&mut self) {
        self.tool_call_manager.select_next();
    }
    
    /// Select previous tool call (for navigation)
    pub fn select_prev_tool(&mut self) {
        self.tool_call_manager.select_prev();
    }
    
    /// Toggle expand/collapse of selected tool
    pub fn toggle_selected_tool(&mut self) {
        self.tool_call_manager.toggle_selected();
    }
    
    /// Expand all tool calls
    pub fn expand_all_tools(&mut self) {
        self.tool_call_manager.expand_all();
    }
    
    /// Collapse all tool calls
    pub fn collapse_all_tools(&mut self) {
        self.tool_call_manager.collapse_all();
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
            Action::ToolCallNextTool => self.select_next_tool(),
            Action::ToolCallPrevTool => self.select_prev_tool(),
            Action::ToolCallToggleExpand => self.toggle_selected_tool(),
            Action::ToolCallExpandAll => self.expand_all_tools(),
            Action::ToolCallCollapseAll => self.collapse_all_tools(),
            // TRC-017: Handle thinking toggle
            Action::ThinkingToggleCollapse => self.toggle_thinking_collapse(),
            // TRC-021: Handle search actions
            Action::ConversationSearchStart => self.start_search(),
            Action::ConversationSearchClose => self.close_search(),
            Action::ConversationSearchNext => self.search_next(),
            Action::ConversationSearchPrev => self.search_prev(),
            Action::ConversationSearchQuery(query) => {
                self.search_state.set_query(query.clone());
                self.update_search();
            }
            Action::ConversationSearchToggleCase => {
                self.search_state.toggle_case_sensitivity();
                self.update_search();
            }
            _ => {}
        }
    }

    fn render(&self, _frame: &mut Frame, _area: Rect, _focused: bool, _theme: &Theme) {
        // Use render_conversation() instead for full functionality
    }
}

impl ConversationViewer {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // TRC-021: Search mode keys
        if self.search_state.is_active() {
            match self.search_state.handle_key(key) {
                SearchAction::Close => {
                    self.close_search();
                    return Some(Action::ConversationSearchClose);
                }
                SearchAction::NavigateToMatch => {
                    self.scroll_to_current_search_match();
                    return None;
                }
                SearchAction::RefreshSearch => {
                    self.update_search();
                    self.scroll_to_current_search_match();
                    return None;
                }
                SearchAction::None => return None,
            }
        }

        // Tool navigation mode keys
        if self.tool_navigation_mode {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => return Some(Action::ToolCallNextTool),
                KeyCode::Char('k') | KeyCode::Up => return Some(Action::ToolCallPrevTool),
                KeyCode::Enter | KeyCode::Char(' ') => return Some(Action::ToolCallToggleExpand),
                KeyCode::Char('e') => return Some(Action::ToolCallExpandAll),
                KeyCode::Char('c') => return Some(Action::ToolCallCollapseAll),
                KeyCode::Esc => {
                    self.tool_navigation_mode = false;
                    return None;
                }
                _ => {}
            }
        }
        
        // Normal conversation viewer keys
        match key.code {
            // TRC-021: Start search with '/'
            KeyCode::Char('/') => {
                self.start_search();
                Some(Action::ConversationSearchStart)
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.search_state.match_count() > 0 {
                    self.search_next();
                    Some(Action::ConversationSearchNext)
                } else {
                    None
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.search_state.match_count() > 0 {
                    self.search_prev();
                    Some(Action::ConversationSearchPrev)
                } else {
                    None
                }
            }
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
            KeyCode::Char('t') => {
                // Enter tool navigation mode
                if !self.tool_call_manager.is_empty() {
                    self.tool_navigation_mode = true;
                }
                None
            }
            // TRC-017: Toggle thinking block collapse with 'T'
            KeyCode::Char('T') => {
                Some(Action::ThinkingToggleCollapse)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_tool_use(name: &str) -> ToolUse {
        ToolUse {
            id: format!("tool_{}", name),
            name: name.to_string(),
            input: json!({"path": "/test/path"}),
        }
    }

    #[test]
    fn test_conversation_viewer_new() {
        let viewer = ConversationViewer::new();
        assert_eq!(viewer.scroll_offset, 0);
        assert!(viewer.auto_scroll);
        assert!(viewer.tool_call_manager.is_empty());
        assert!(!viewer.tool_navigation_mode);
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

    #[test]
    fn test_tool_registration() {
        let mut viewer = ConversationViewer::new();
        
        viewer.register_tool_use(create_test_tool_use("file_read"));
        assert_eq!(viewer.tool_call_manager.len(), 1);
        assert!(viewer.has_pending_tool());
        
        viewer.start_tool_execution("tool_file_read");
        assert!(!viewer.has_pending_tool());
        assert!(viewer.has_running_tool());
    }

    #[test]
    fn test_tool_navigation_mode() {
        let mut viewer = ConversationViewer::new();
        assert!(!viewer.is_tool_navigation_mode());
        
        viewer.toggle_tool_navigation();
        assert!(viewer.is_tool_navigation_mode());
        
        viewer.toggle_tool_navigation();
        assert!(!viewer.is_tool_navigation_mode());
    }

    #[test]
    fn test_tool_navigation() {
        let mut viewer = ConversationViewer::new();
        
        viewer.register_tool_use(create_test_tool_use("tool1"));
        viewer.register_tool_use(create_test_tool_use("tool2"));
        viewer.register_tool_use(create_test_tool_use("tool3"));
        
        assert_eq!(viewer.tool_call_manager.selected_index(), Some(2));
        
        viewer.select_prev_tool();
        assert_eq!(viewer.tool_call_manager.selected_index(), Some(1));
        
        viewer.select_next_tool();
        assert_eq!(viewer.tool_call_manager.selected_index(), Some(2));
    }

    #[test]
    fn test_tool_expand_collapse() {
        let mut viewer = ConversationViewer::new();
        
        viewer.register_tool_use(create_test_tool_use("tool1"));
        viewer.register_tool_use(create_test_tool_use("tool2"));
        
        // Default is expanded
        assert!(viewer.tool_call_manager.tool_calls().iter().all(|tc| tc.expanded));
        
        viewer.collapse_all_tools();
        assert!(viewer.tool_call_manager.tool_calls().iter().all(|tc| !tc.expanded));
        
        viewer.expand_all_tools();
        assert!(viewer.tool_call_manager.tool_calls().iter().all(|tc| tc.expanded));
    }

    #[test]
    fn test_clear_tool_calls() {
        let mut viewer = ConversationViewer::new();
        
        viewer.register_tool_use(create_test_tool_use("tool1"));
        viewer.register_tool_use(create_test_tool_use("tool2"));
        
        assert_eq!(viewer.tool_call_manager.len(), 2);
        
        viewer.clear_tool_calls();
        assert!(viewer.tool_call_manager.is_empty());
    }
}
