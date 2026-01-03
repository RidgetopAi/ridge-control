// Conversation viewer - some state getters for future UI integration
#![allow(dead_code)]

use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use crate::action::Action;
use crate::agent::ContextStats;
use crate::components::search::{SearchState, SearchBar, SearchAction};
use crate::components::spinner::{Spinner, SpinnerStyle};
use crate::components::tool_call_widget::{ToolCallManager, ToolCallWidget, ToolStatus};
use crate::components::Component;
use crate::config::Theme;
use crate::llm::{ContentBlock, Message, Role, ToolUse, ToolResult};
use crate::util::strip_ansi;

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
    /// Whether tool results are collapsed (press 'R' to toggle)
    tool_results_collapsed: bool,
    /// Search state (TRC-021)
    search_state: SearchState,
    /// Cached text lines for search
    cached_text: Vec<String>,
    /// Mapping from visual line index to cached_text index (accounts for text wrapping)
    visual_to_cached: Vec<usize>,
    /// Inner width used for wrapping calculation (to calculate column offset in wrapped lines)
    cached_width: u16,
    /// Visible lines extracted from terminal buffer (what's actually displayed on screen)
    /// This is the source of truth for text selection - indexed by visible row
    visible_lines: Vec<String>,
    /// Text selection state for copy support
    selection: Option<TextSelection>,
    /// Whether we're currently dragging to select
    selecting: bool,
    // Phase 3: Streaming optimization - caching fields
    /// Cached rendered lines for stable message content (invalidated on message changes)
    cached_message_lines: Vec<Line<'static>>,
    /// Hash of last rendered message content (for cache invalidation)
    cached_message_hash: u64,
    /// Last streaming buffer length (for incremental updates)
    last_streaming_len: usize,
    /// Last thinking buffer length (for incremental updates)
    last_thinking_len: usize,
}

/// Text selection in the conversation viewer
/// Positions are stored as ABSOLUTE visual line indices (not relative to viewport)
#[derive(Debug, Clone, Copy)]
pub struct TextSelection {
    /// Start position (absolute_line, column) - absolute_line is scroll_offset + visible_row
    pub start: (usize, usize),
    /// End position (absolute_line, column)
    pub end: (usize, usize),
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
            tool_results_collapsed: false,
            search_state: SearchState::new(),
            cached_text: Vec::new(),
            visual_to_cached: Vec::new(),
            cached_width: 0,
            visible_lines: Vec::new(),
            selection: None,
            selecting: false,
            // Phase 3: Initialize caching fields
            cached_message_lines: Vec::new(),
            cached_message_hash: 0,
            last_streaming_len: 0,
            last_thinking_len: 0,
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
    
    /// Set auto-scroll state
    pub fn set_auto_scroll(&mut self, enabled: bool) {
        self.auto_scroll = enabled;
    }
    
    /// Clear the conversation viewer state for a new thread
    pub fn clear(&mut self) {
        self.scroll_offset = 0;
        self.line_count = 0;
        self.auto_scroll = true;
        self.tool_call_manager.clear();
        self.tool_navigation_mode = false;
        self.tool_results_collapsed = false;
        self.search_state = SearchState::new();
        self.cached_text.clear();
        self.visual_to_cached.clear();
        self.cached_width = 0;
        self.visible_lines.clear();
        self.selection = None;
        self.selecting = false;
        // Phase 3: Clear caching state
        self.cached_message_lines.clear();
        self.cached_message_hash = 0;
        self.last_streaming_len = 0;
        self.last_thinking_len = 0;
    }
    
    /// Toggle tool results collapse state
    pub fn toggle_tool_results_collapse(&mut self) {
        self.tool_results_collapsed = !self.tool_results_collapsed;
    }
    
    /// Get tool results collapse state
    pub fn is_tool_results_collapsed(&self) -> bool {
        self.tool_results_collapsed
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
    /// For file_write, captures original file content for diff view
    pub fn register_tool_use(&mut self, tool_use: ToolUse) {
        if tool_use.name == "file_write" {
            // Extract path and read original content for diff view
            let original_content = tool_use.input.get("path")
                .and_then(|v| v.as_str())
                .and_then(|path| {
                    // Expand ~ to home directory
                    let expanded_path = if path.starts_with("~/") {
                        dirs::home_dir()
                            .map(|home| home.join(&path[2..]))
                            .unwrap_or_else(|| std::path::PathBuf::from(path))
                    } else {
                        std::path::PathBuf::from(path)
                    };
                    // Read existing file content (None if file doesn't exist)
                    std::fs::read_to_string(&expanded_path).ok()
                });
            self.tool_call_manager.add_tool_call_with_original(tool_use, original_content);
        } else {
            self.tool_call_manager.add_tool_call(tool_use);
        }
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

    // =====================================================================
    // Text Selection Support
    // =====================================================================

    /// Convert screen coordinates to text position (absolute_line, column)
    /// Returns ABSOLUTE line position (scroll_offset + visible_row) for scroll-independent selection
    fn screen_to_text_pos(&self, screen_x: u16, screen_y: u16) -> Option<(usize, usize)> {
        if self.inner_area.width == 0 || self.inner_area.height == 0 {
            return None;
        }

        if screen_x < self.inner_area.x || screen_y < self.inner_area.y {
            return None;
        }
        if screen_x >= self.inner_area.x + self.inner_area.width
            || screen_y >= self.inner_area.y + self.inner_area.height {
            return None;
        }

        let screen_col = (screen_x - self.inner_area.x) as usize;
        let visible_row = (screen_y - self.inner_area.y) as usize;

        // Convert to ABSOLUTE line position by adding scroll offset
        // This allows selection to persist across scroll operations
        let absolute_line = (self.scroll_offset as usize) + visible_row;

        // Clamp to valid range (total line count)
        let line = absolute_line.min(self.line_count.saturating_sub(1));

        Some((line, screen_col))
    }

    /// Start text selection at screen coordinates
    pub fn start_selection(&mut self, screen_x: u16, screen_y: u16) {
        if let Some((line, col)) = self.screen_to_text_pos(screen_x, screen_y) {
            self.selection = Some(TextSelection {
                start: (line, col),
                end: (line, col),
            });
            self.selecting = true;
            self.auto_scroll = false;
        }
    }

    /// Update selection end point during drag
    /// Handles auto-scroll when mouse is above or below the viewport
    pub fn update_selection(&mut self, screen_x: u16, screen_y: u16) {
        if !self.selecting {
            return;
        }

        // Check if mouse is above or below the viewport and auto-scroll
        let scroll_amount = self.check_drag_scroll(screen_y);
        if scroll_amount != 0 {
            self.apply_scroll_delta(scroll_amount);
        }

        // Now update selection end position
        // If mouse is outside viewport, clamp to edge
        let clamped_y = screen_y.clamp(
            self.inner_area.y,
            self.inner_area.y + self.inner_area.height.saturating_sub(1)
        );

        if let Some((line, col)) = self.screen_to_text_pos(screen_x, clamped_y) {
            if let Some(ref mut sel) = self.selection {
                sel.end = (line, col);
            }
        }
    }

    /// Check if mouse position during drag requires scrolling
    /// Returns scroll delta: negative for up, positive for down, 0 for no scroll
    fn check_drag_scroll(&self, screen_y: u16) -> i16 {
        if self.inner_area.height == 0 {
            return 0;
        }

        // Scroll up if mouse is above viewport
        if screen_y < self.inner_area.y {
            let distance = self.inner_area.y - screen_y;
            return -(distance.min(3) as i16); // Scroll up to 3 lines at a time
        }

        // Scroll down if mouse is below viewport
        let viewport_bottom = self.inner_area.y + self.inner_area.height;
        if screen_y >= viewport_bottom {
            let distance = screen_y - viewport_bottom + 1;
            return distance.min(3) as i16; // Scroll up to 3 lines at a time
        }

        0
    }

    /// Apply scroll delta during drag selection
    fn apply_scroll_delta(&mut self, delta: i16) {
        let max_scroll = self.line_count.saturating_sub(self.visible_height as usize) as i32;
        let new_scroll = (self.scroll_offset as i32 + delta as i32).clamp(0, max_scroll);
        self.scroll_offset = new_scroll as u16;
    }

    /// End selection (mouse up)
    pub fn end_selection(&mut self) {
        self.selecting = false;
    }

    /// Clear current selection
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.selecting = false;
    }

    /// Check if there's an active selection
    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    /// Check if currently in the middle of a drag selection
    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    /// Get the currently selected text
    /// Uses visible_lines (extracted from frame buffer) for accurate text extraction
    /// Selection positions are absolute visual line indices - convert to visible row for extraction
    pub fn get_selected_text(&self) -> Option<String> {
        let sel = self.selection?;

        if self.visible_lines.is_empty() {
            return None;
        }

        // Normalize selection (start should be before end)
        let (start, end) = if sel.start.0 < sel.end.0
            || (sel.start.0 == sel.end.0 && sel.start.1 <= sel.end.1) {
            (sel.start, sel.end)
        } else {
            (sel.end, sel.start)
        };

        // Selection positions are absolute (scroll_offset + visible_row)
        // Convert to visible row indices for visible_lines lookup
        let scroll = self.scroll_offset as usize;
        let visible_height = self.visible_lines.len();

        // Check if selection is at least partially within viewport
        let start_abs = start.0;
        let end_abs = end.0;

        // Selection must overlap with current viewport to extract text
        if end_abs < scroll || start_abs >= scroll + visible_height {
            // Selection is entirely outside viewport - can't extract
            return None;
        }

        // Clamp selection to visible portion
        let vis_start_line = start_abs.saturating_sub(scroll);
        let vis_end_line = (end_abs - scroll).min(visible_height.saturating_sub(1));
        let start_col = start.1;
        let end_col = end.1;

        let mut result = String::new();

        for vis_row in vis_start_line..=vis_end_line {
            let line_text = match self.visible_lines.get(vis_row) {
                Some(text) => text,
                None => continue,
            };

            let line_chars: Vec<char> = line_text.chars().collect();

            // Determine if this is start/end line based on absolute positions
            let abs_line = scroll + vis_row;
            let is_start_line = abs_line == start_abs;
            let is_end_line = abs_line == end_abs;

            if is_start_line && is_end_line {
                // Single line selection
                let start_idx = start_col.min(line_chars.len());
                let end_idx = (end_col + 1).min(line_chars.len());
                if start_idx < end_idx {
                    result.push_str(&line_chars[start_idx..end_idx].iter().collect::<String>());
                }
            } else if is_start_line {
                // First line of multi-line selection
                let start_idx = start_col.min(line_chars.len());
                result.push_str(&line_chars[start_idx..].iter().collect::<String>());
                result.push('\n');
            } else if is_end_line {
                // Last line of multi-line selection
                let end_idx = (end_col + 1).min(line_chars.len());
                result.push_str(&line_chars[..end_idx].iter().collect::<String>());
            } else {
                // Middle lines - take entire line
                result.push_str(line_text);
                result.push('\n');
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
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
                    Some(Action::ConversationCopy)
                } else {
                    None
                }
            }
            MouseEventKind::ScrollUp => Some(Action::ConversationScrollUp(3)),
            MouseEventKind::ScrollDown => Some(Action::ConversationScrollDown(3)),
            _ => None,
        }
    }

    /// Check if position is within selection (for highlighting)
    pub fn is_position_selected(&self, line: usize, col: usize) -> bool {
        let sel = match self.selection {
            Some(s) => s,
            None => return false,
        };
        
        let (start, end) = if sel.start.0 < sel.end.0 
            || (sel.start.0 == sel.end.0 && sel.start.1 <= sel.end.1) {
            (sel.start, sel.end)
        } else {
            (sel.end, sel.start)
        };
        
        if line < start.0 || line > end.0 {
            return false;
        }
        
        if line == start.0 && line == end.0 {
            col >= start.1 && col <= end.1
        } else if line == start.0 {
            col >= start.1
        } else if line == end.0 {
            col <= end.1
        } else {
            true
        }
    }

    /// Render selection highlight overlay on the frame buffer
    /// Selection positions are ABSOLUTE line indices - convert to visible positions for rendering
    fn render_selection_highlight(&self, frame: &mut Frame, inner: Rect, theme: &Theme) {
        let sel = match self.selection {
            Some(s) => s,
            None => return,
        };

        // Normalize selection (start before end)
        let (start, end) = if sel.start.0 < sel.end.0
            || (sel.start.0 == sel.end.0 && sel.start.1 <= sel.end.1) {
            (sel.start, sel.end)
        } else {
            (sel.end, sel.start)
        };

        // Selection highlight style
        let highlight_style = Style::default()
            .bg(theme.colors.primary.to_color())
            .fg(theme.colors.background.to_color());

        let buf = frame.buffer_mut();
        let scroll = self.scroll_offset as usize;
        let visible_end = scroll + inner.height as usize;

        // Selection positions are absolute - iterate over the absolute range
        for abs_line in start.0..=end.0 {
            // Skip lines outside visible window
            if abs_line < scroll || abs_line >= visible_end {
                continue;
            }

            // Convert absolute line to visible row (0-based within viewport)
            let visible_row = abs_line - scroll;

            // Screen Y = inner.y + visible_row
            let screen_y = inner.y + visible_row as u16;

            // Determine column range based on absolute line position
            let col_start = if abs_line == start.0 { start.1 } else { 0 };
            let col_end = if abs_line == end.0 { end.1 } else { inner.width as usize - 1 };

            // Render highlight
            for col in col_start..=col_end {
                let screen_x = inner.x + col as u16;
                if screen_x >= inner.x + inner.width {
                    break;
                }

                if let Some(cell) = buf.cell_mut((screen_x, screen_y)) {
                    cell.set_style(highlight_style);
                }
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

    /// Cache rendered lines for text selection
    /// Each entry in cached_text corresponds to one Line from the render.
    /// Note: Text wrapping may cause visual lines to differ from cached_text indices,
    /// but this provides reasonable selection for most content.
    fn cache_rendered_lines(&mut self, lines: &[Line], width: u16) {
        self.cached_text.clear();
        self.visual_to_cached.clear();
        self.cached_width = width;

        for line in lines.iter() {
            // Extract text content from the line (concatenate all spans)
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            self.cached_text.push(text);
        }

        // Initial 1:1 mapping - will be rebuilt after we know line_count
        for i in 0..self.cached_text.len() {
            self.visual_to_cached.push(i);
        }
    }

    /// Build the visual-to-cached mapping
    /// Simple 1:1 mapping - visual line index equals cached line index (clamped)
    fn build_visual_to_cached_mapping(&mut self) {
        let cached_count = self.cached_text.len();
        let visual_count = self.line_count;

        self.visual_to_cached.clear();

        if cached_count == 0 || visual_count == 0 {
            return;
        }

        // Simple 1:1 mapping, clamped to valid range
        for i in 0..visual_count {
            self.visual_to_cached.push(i.min(cached_count - 1));
        }
    }

    /// Extract visible text from the terminal buffer after rendering
    /// This gives us exactly what's displayed on screen for accurate text selection
    fn extract_visible_text(&mut self, frame: &mut Frame, inner: Rect) {
        self.visible_lines.clear();
        let buf = frame.buffer_mut();

        // We need to extract ALL visual lines, not just the visible window
        // The scroll_offset tells us which visual line starts at the top of the window
        // So we need to store: scroll_offset + row -> visible_lines[row]
        //
        // For text selection to work correctly with scrolling, we'll store lines
        // indexed by their absolute visual position (scroll_offset + visible_row)

        for row in 0..inner.height {
            let y = inner.y + row;
            let mut line = String::new();

            for col in 0..inner.width {
                let x = inner.x + col;
                if let Some(cell) = buf.cell((x, y)) {
                    line.push_str(cell.symbol());
                }
            }

            // Trim trailing whitespace but preserve leading whitespace (indentation)
            self.visible_lines.push(line.trim_end().to_string());
        }
    }

    /// Phase 3: Compute hash for cache invalidation
    /// Includes message content, collapse states, and tool manager state
    fn compute_message_hash(&self, messages: &[Message]) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Hash message count and content
        messages.len().hash(&mut hasher);
        for msg in messages {
            // Hash role by discriminant since Role doesn't implement Hash
            std::mem::discriminant(&msg.role).hash(&mut hasher);
            for content in &msg.content {
                match content {
                    ContentBlock::Text(t) => {
                        0u8.hash(&mut hasher);
                        t.hash(&mut hasher);
                    }
                    ContentBlock::Thinking(t) => {
                        1u8.hash(&mut hasher);
                        t.hash(&mut hasher);
                    }
                    ContentBlock::ToolUse(tu) => {
                        2u8.hash(&mut hasher);
                        tu.id.hash(&mut hasher);
                        tu.name.hash(&mut hasher);
                    }
                    ContentBlock::ToolResult(tr) => {
                        3u8.hash(&mut hasher);
                        tr.tool_use_id.hash(&mut hasher);
                        tr.is_error.hash(&mut hasher);
                    }
                    ContentBlock::Image(_) => {
                        4u8.hash(&mut hasher);
                    }
                }
            }
        }

        // Hash collapse states (affect rendering)
        self.thinking_collapsed.hash(&mut hasher);
        self.tool_results_collapsed.hash(&mut hasher);

        // Hash tool manager state (affects tool rendering)
        self.tool_call_manager.len().hash(&mut hasher);
        for tc in self.tool_call_manager.tool_calls() {
            tc.tool_id().hash(&mut hasher);
            tc.expanded.hash(&mut hasher);
            // Hash status discriminant
            std::mem::discriminant(&tc.status).hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Render conversation with messages and current streaming buffers
    /// TRC-017: Now accepts separate thinking_buffer for extended thinking display
    /// TRC-021: Added search support
    /// Phase 3: Added context_stats for token usage display + caching optimization
    /// model_info: Optional (provider, model) tuple for header display
    #[allow(clippy::too_many_arguments)] // Parameters are semantically distinct
    pub fn render_conversation(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        focused: bool,
        messages: &[Message],
        streaming_buffer: &str,
        thinking_buffer: &str,
        theme: &Theme,
        model_info: Option<(&str, &str)>,
        context_stats: Option<&ContextStats>,
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

        // Build title with status indicators (TRC-017: include thinking indicator, TRC-021: search, Phase 3: context stats)
        let title = self.build_title(streaming_buffer, thinking_buffer, model_info, context_stats);

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(conversation_area);
        self.visible_height = inner.height;

        // Phase 3: Compute hash for cache invalidation
        let current_hash = self.compute_message_hash(messages);

        // Phase 3: Use cached lines if message content hasn't changed
        // This avoids re-rendering stable message content on every frame
        if current_hash != self.cached_message_hash {
            // Cache miss - rebuild message lines
            let mut message_lines: Vec<Line<'static>> = Vec::new();

            for message in messages {
                // Check if this is a tool-result-only message (should not show "User:" header)
                let is_tool_result_only = message.role == Role::User
                    && !message.content.is_empty()
                    && message.content.iter().all(|block| matches!(block, ContentBlock::ToolResult(_)));

                // Add role header (skip for tool-result-only messages)
                if !is_tool_result_only {
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
                    message_lines.push(Line::from(Span::styled(role_text, role_style)));
                }

                // Add content blocks
                for content_block in &message.content {
                    match content_block {
                        ContentBlock::Text(text) => {
                            let clean_text = strip_ansi(text);
                            message_lines.extend(self.render_text_with_diff_blocks(&clean_text, theme));
                        }
                        ContentBlock::Thinking(text) => {
                            // TRC-017: Collapsible thinking blocks
                            message_lines.extend(self.render_thinking_block(text, theme));
                        }
                        ContentBlock::ToolUse(tool) => {
                            // Phase 2: ToolCallManager is single source of truth
                            // ToolCallWidget renders tool + result together
                            message_lines.extend(self.render_tool_use_enhanced(tool, theme));
                        }
                        ContentBlock::ToolResult(result) => {
                            // Phase 2: Skip if tool is tracked in ToolCallManager
                            // (ToolCallWidget already displays the result inline)
                            if self.tool_call_manager.get(&result.tool_use_id).is_none() {
                                // Fallback for tools not in manager (e.g., loaded from history)
                                message_lines.extend(self.render_tool_result(result, theme));
                            }
                        }
                        ContentBlock::Image(_) => {
                            message_lines.push(Line::from(Span::styled(
                                "  [Image]",
                                Style::default().fg(theme.colors.muted.to_color()),
                            )));
                        }
                    }
                }

                // Add spacing between messages
                message_lines.push(Line::from(""));
            }

            // Update cache
            self.cached_message_lines = message_lines;
            self.cached_message_hash = current_hash;
        }

        // Start with cached message lines (clone for this render)
        let mut lines: Vec<Line> = self.cached_message_lines.clone();

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
                let clean_thinking = strip_ansi(thinking_buffer);
                let thinking_lines: Vec<&str> = clean_thinking.lines().collect();
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
            let clean_streaming = strip_ansi(streaming_buffer);
            for line in clean_streaming.lines() {
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

        // Handle empty state
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No messages yet. Send a message to start a conversation.",
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        // Cache rendered lines for text selection (must happen before paragraph takes ownership)
        self.cache_rendered_lines(&lines, inner.width);

        // Create paragraph with wrap to measure actual wrapped line count
        let paragraph = Paragraph::new(lines)
            .block(block.clone())
            .wrap(Wrap { trim: false });

        // Calculate actual line count after text wrapping
        // This accounts for long lines that wrap to multiple visual lines
        self.line_count = paragraph.line_count(inner.width);

        // Now that we know the actual visual line count, rebuild the mapping
        // This ensures visual line indices map correctly to cached text indices
        self.build_visual_to_cached_mapping();

        // Auto-scroll to bottom if enabled and new content
        if self.auto_scroll && self.line_count > self.visible_height as usize {
            self.scroll_offset = (self.line_count - self.visible_height as usize) as u16;
        }
        
        // Clamp scroll offset to valid range (prevents scrolling past content)
        let max_scroll = self.line_count.saturating_sub(self.visible_height as usize) as u16;
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        // Apply scroll offset and render
        let scrolled_paragraph = paragraph.scroll((self.scroll_offset, 0));
        frame.render_widget(scrolled_paragraph, conversation_area);

        // Extract visible text from buffer for accurate text selection
        self.extract_visible_text(frame, inner);

        // Render selection highlight overlay
        self.render_selection_highlight(frame, inner, theme);

        // Render scrollbar if content exceeds visible area
        if self.line_count > self.visible_height as usize {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            // ScrollbarState uses content length minus viewport as the scrollable range
            let scrollable_range = self.line_count.saturating_sub(self.visible_height as usize);
            let mut scrollbar_state = ScrollbarState::new(scrollable_range)
                .position(self.scroll_offset as usize);

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
    /// Phase 3: Added context_stats for token usage display
    /// model_info: Optional (provider, model) for display in header
    fn build_title(
        &self,
        streaming_buffer: &str,
        thinking_buffer: &str,
        model_info: Option<(&str, &str)>,
        context_stats: Option<&ContextStats>,
    ) -> String {
        let mut title_parts = vec![" Conversation".to_string()];
        
        // Add model/provider indicator if available
        if let Some((provider, model)) = model_info {
            if !provider.is_empty() && !model.is_empty() {
                let short_model = Self::abbreviate_model_name(model);
                title_parts.push(format!(" [{}:{}]", provider, short_model));
            }
        }

        // Phase 3: Add context/token stats
        if let Some(stats) = context_stats {
            if stats.tokens_budget > 0 {
                let truncated_indicator = if stats.truncated { "↓" } else { "" };
                title_parts.push(format!(" 󰊤{}{}", stats.format_compact(), truncated_indicator));
            }
        }
        
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
    
    /// Abbreviate long model names for display
    fn abbreviate_model_name(model: &str) -> String {
        // Extract the core model name, remove date suffixes and provider prefixes
        // e.g., "claude-sonnet-4-20250514" -> "sonnet-4"
        // e.g., "gpt-4o-2024-08-06" -> "gpt-4o"
        // e.g., "gemini-2.0-flash" -> "gemini-2.0-flash"
        
        let model = model.to_lowercase();
        
        // Remove common date patterns (YYYYMMDD or YYYY-MM-DD at end)
        let without_date = if let Some(pos) = model.rfind(['-', '_']) {
            let suffix = &model[pos + 1..];
            if suffix.len() >= 8 && suffix.chars().all(|c| c.is_ascii_digit() || c == '-') {
                &model[..pos]
            } else {
                model.as_str()
            }
        } else {
            model.as_str()
        };
        
        // For Claude models, simplify to key parts with version
        // Model formats: claude-3-5-haiku-*, claude-opus-4-5-*, claude-sonnet-4-*, etc.
        if without_date.contains("claude") {
            // Extract version number from model name
            let version = if without_date.contains("4-5") || without_date.contains("4.5") {
                "4.5"
            } else if without_date.contains("-4-") || without_date.ends_with("-4") {
                "4"
            } else if without_date.contains("3-5") || without_date.contains("3.5") {
                "3.5"
            } else if without_date.contains("-3-") || without_date.ends_with("-3") {
                "3"
            } else {
                "" // Unknown version
            };

            if without_date.contains("opus") {
                return if version.is_empty() { "opus".to_string() } else { format!("opus-{}", version) };
            } else if without_date.contains("sonnet") {
                return if version.is_empty() { "sonnet".to_string() } else { format!("sonnet-{}", version) };
            } else if without_date.contains("haiku") {
                return if version.is_empty() { "haiku".to_string() } else { format!("haiku-{}", version) };
            }
        }
        
        // For other models, just return without date, capped at reasonable length
        let result = without_date.to_string();
        if result.len() > 20 {
            format!("{}…", &result[..19])
        } else {
            result
        }
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
            // Show thinking content (strip ANSI escapes)
            let clean_text = strip_ansi(text);
            let thinking_lines: Vec<&str> = clean_text.lines().collect();
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

    /// Render text content, detecting and styling ```diff code blocks
    fn render_text_with_diff_blocks(&self, text: &str, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut in_diff_block = false;
        let mut in_code_block = false;

        for line in text.lines() {
            let trimmed = line.trim();

            // Check for code block markers
            if trimmed.starts_with("```") {
                if trimmed == "```diff" {
                    in_diff_block = true;
                    in_code_block = true;
                    // Render the marker in muted color
                    lines.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(theme.colors.muted.to_color()),
                    )));
                    continue;
                } else if in_code_block && trimmed == "```" {
                    // End of code block
                    in_diff_block = false;
                    in_code_block = false;
                    lines.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(theme.colors.muted.to_color()),
                    )));
                    continue;
                } else if trimmed.starts_with("```") {
                    // Start of non-diff code block
                    in_code_block = true;
                    lines.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(theme.colors.muted.to_color()),
                    )));
                    continue;
                }
            }

            if in_diff_block {
                // Apply diff styling - reuse the existing method with adjusted prefix
                let styled = self.style_diff_line_for_text(line, theme);
                lines.push(styled);
            } else {
                // Normal text rendering
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(theme.colors.foreground.to_color()),
                )));
            }
        }

        lines
    }

    /// Style a diff line within markdown text (uses "  " prefix instead of "    ")
    fn style_diff_line_for_text(&self, line: &str, theme: &Theme) -> Line<'static> {
        let trimmed = line.trim_start();

        if trimmed.starts_with("+++") || trimmed.starts_with("---") {
            Line::from(Span::styled(
                format!("  {}", line),
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD),
            ))
        } else if trimmed.starts_with("@@") {
            Line::from(Span::styled(
                format!("  {}", line),
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            ))
        } else if trimmed.starts_with('+') {
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    "+".to_string(),
                    Style::default()
                        .fg(theme.colors.success.to_color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    trimmed[1..].to_string(),
                    Style::default().fg(theme.colors.success.to_color()),
                ),
            ])
        } else if trimmed.starts_with('-') {
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    "-".to_string(),
                    Style::default()
                        .fg(theme.colors.error.to_color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    trimmed[1..].to_string(),
                    Style::default().fg(theme.colors.error.to_color()),
                ),
            ])
        } else {
            Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(theme.colors.foreground.to_color()),
            ))
        }
    }

    fn render_tool_result(&self, result: &ToolResult, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        let (status_icon, color) = if result.is_error {
            ("󰅚", theme.colors.error.to_color())
        } else {
            ("󰄬", theme.colors.success.to_color())
        };

        let collapse_indicator = if self.tool_results_collapsed { "▶" } else { "▼" };

        // Render result content
        let content_str = match &result.content {
            crate::llm::ToolResultContent::Text(text) => text.clone(),
            crate::llm::ToolResultContent::Json(json) => {
                serde_json::to_string_pretty(json).unwrap_or_else(|_| json.to_string())
            }
            crate::llm::ToolResultContent::Image(_) => "[Image result]".to_string(),
        };
        
        let content_lines: Vec<&str> = content_str.lines().collect();
        let total_lines = content_lines.len();

        // Header with collapse indicator
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", collapse_indicator),
                Style::default().fg(theme.colors.accent.to_color()),
            ),
            Span::styled(
                format!("{} Tool Result", status_icon),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({} lines)", total_lines),
                Style::default().fg(theme.colors.muted.to_color()),
            ),
        ]));

        if self.tool_results_collapsed {
            // Collapsed: show summary only
            lines.push(Line::from(Span::styled(
                "    [Collapsed - press 'R' to expand]",
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::DIM),
            )));
        } else {
            // Expanded: show content with higher limit (50 lines)
            let max_lines = 50;
            let truncated = total_lines > max_lines;

            // Detect if this looks like diff output (edit preview)
            let is_diff_output = content_str.contains("\n--- ") && content_str.contains("\n+++ ");

            for line in content_lines.iter().take(max_lines) {
                let styled_line = if is_diff_output && !result.is_error {
                    // Apply diff styling
                    self.style_diff_line(line, theme)
                } else {
                    Line::from(Span::styled(
                        format!("    {}", line),
                        Style::default().fg(if result.is_error {
                            theme.colors.error.to_color()
                        } else {
                            theme.colors.foreground.to_color()
                        }),
                    ))
                };
                lines.push(styled_line);
            }

            if truncated {
                lines.push(Line::from(Span::styled(
                    format!("    ... ({} more lines - press 'R' to collapse)", total_lines - max_lines),
                    Style::default()
                        .fg(theme.colors.muted.to_color())
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }

        lines
    }

    /// Style a single diff line with appropriate colors
    fn style_diff_line(&self, line: &str, theme: &Theme) -> Line<'static> {
        let trimmed = line.trim_start();

        if trimmed.starts_with("+++") || trimmed.starts_with("---") {
            // File headers - bold accent color
            Line::from(Span::styled(
                format!("    {}", line),
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD),
            ))
        } else if trimmed.starts_with("@@") {
            // Hunk headers - muted italic
            Line::from(Span::styled(
                format!("    {}", line),
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            ))
        } else if trimmed.starts_with('+') {
            // Addition - green
            Line::from(vec![
                Span::styled(
                    "    ",
                    Style::default(),
                ),
                Span::styled(
                    "+".to_string(),
                    Style::default()
                        .fg(theme.colors.success.to_color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    trimmed[1..].to_string(),
                    Style::default().fg(theme.colors.success.to_color()),
                ),
            ])
        } else if trimmed.starts_with('-') {
            // Deletion - red
            Line::from(vec![
                Span::styled(
                    "    ",
                    Style::default(),
                ),
                Span::styled(
                    "-".to_string(),
                    Style::default()
                        .fg(theme.colors.error.to_color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    trimmed[1..].to_string(),
                    Style::default().fg(theme.colors.error.to_color()),
                ),
            ])
        } else {
            // Context line or other - normal foreground
            Line::from(Span::styled(
                format!("    {}", line),
                Style::default().fg(theme.colors.foreground.to_color()),
            ))
        }
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: u16) {
        let max_scroll = self.line_count.saturating_sub(self.visible_height as usize) as u16;
        self.scroll_offset = self.scroll_offset.saturating_add(n).min(max_scroll);
        // Re-enable auto-scroll if at bottom
        if self.scroll_offset >= max_scroll {
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
            // Tool result toggle
            Action::ToolResultToggleCollapse => self.toggle_tool_results_collapse(),
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
            KeyCode::Char('j') | KeyCode::Down => Some(Action::ConversationScrollDown(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::ConversationScrollUp(1)),
            KeyCode::Char('g') => Some(Action::ConversationScrollToTop),
            KeyCode::Char('G') => Some(Action::ConversationScrollToBottom),
            KeyCode::PageUp => Some(Action::ConversationScrollUp(10)),
            KeyCode::PageDown => Some(Action::ConversationScrollDown(10)),
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
            // Toggle tool results collapse with 'R'
            KeyCode::Char('R') => {
                Some(Action::ToolResultToggleCollapse)
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

    #[test]
    fn test_message_hash_consistency() {
        // Phase 3: Test that same messages produce same hash
        let viewer = ConversationViewer::new();

        let messages = vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text("Hello".to_string())],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text("Hi there".to_string())],
            },
        ];

        let hash1 = viewer.compute_message_hash(&messages);
        let hash2 = viewer.compute_message_hash(&messages);

        assert_eq!(hash1, hash2, "Same messages should produce same hash");
    }

    #[test]
    fn test_message_hash_changes_on_content_change() {
        // Phase 3: Test that different messages produce different hash
        let viewer = ConversationViewer::new();

        let messages1 = vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text("Hello".to_string())],
            },
        ];

        let messages2 = vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text("Hello World".to_string())],
            },
        ];

        let hash1 = viewer.compute_message_hash(&messages1);
        let hash2 = viewer.compute_message_hash(&messages2);

        assert_ne!(hash1, hash2, "Different messages should produce different hash");
    }

    #[test]
    fn test_cache_cleared_on_clear() {
        // Phase 3: Test that cache is cleared when viewer is cleared
        let mut viewer = ConversationViewer::new();

        // Set some cache state
        viewer.cached_message_hash = 12345;
        viewer.last_streaming_len = 100;
        viewer.last_thinking_len = 50;

        viewer.clear();

        assert_eq!(viewer.cached_message_hash, 0);
        assert_eq!(viewer.last_streaming_len, 0);
        assert_eq!(viewer.last_thinking_len, 0);
        assert!(viewer.cached_message_lines.is_empty());
    }
}
