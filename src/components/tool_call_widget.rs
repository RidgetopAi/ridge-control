// TRC-016: Tool call widget - some methods for future use
#![allow(dead_code)]

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::time::Instant;

use crate::config::Theme;
use crate::llm::{ToolUse, ToolResult, ToolResultContent};
use crate::components::spinner::Spinner;
use crate::components::diff_view::{DiffComputer, DiffRenderer};

/// Status of a tool execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    /// Tool call received, awaiting user confirmation
    Pending,
    /// Tool is currently executing
    Running,
    /// Tool completed successfully
    Success,
    /// Tool execution failed
    Error,
    /// Tool execution was rejected by user
    Rejected,
    /// Tool execution timed out
    Timeout,
}

impl ToolStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            ToolStatus::Pending => "⏳",
            ToolStatus::Running => "⋯",
            ToolStatus::Success => "✓",
            ToolStatus::Error => "✗",
            ToolStatus::Rejected => "⊘",
            ToolStatus::Timeout => "⏱",
        }
    }
    
    pub fn label(&self) -> &'static str {
        match self {
            ToolStatus::Pending => "Pending",
            ToolStatus::Running => "Running",
            ToolStatus::Success => "Success",
            ToolStatus::Error => "Error",
            ToolStatus::Rejected => "Rejected",
            ToolStatus::Timeout => "Timeout",
        }
    }
}

/// A single tool call with its state
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub tool_use: ToolUse,
    pub status: ToolStatus,
    pub result: Option<ToolResult>,
    pub expanded: bool,
    pub start_time: Option<Instant>,
    pub end_time: Option<Instant>,
    /// For file_write: original file content (None if new file)
    pub original_content: Option<String>,
    /// For file_write: whether original content was captured
    pub original_captured: bool,
}

impl ToolCall {
    pub fn new(tool_use: ToolUse) -> Self {
        Self {
            tool_use,
            status: ToolStatus::Pending,
            result: None,
            expanded: true,  // Start expanded by default
            start_time: None,
            end_time: None,
            original_content: None,
            original_captured: false,
        }
    }

    /// Create a tool call with original file content (for file_write diff view)
    pub fn with_original_content(mut self, content: Option<String>) -> Self {
        self.original_content = content;
        self.original_captured = true;
        self
    }

    /// Get the new content for file_write (extracts and unescapes from tool input)
    pub fn get_file_write_content(&self) -> Option<String> {
        if self.tool_use.name != "file_write" {
            return None;
        }
        self.tool_use.input.get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get the file path for file_write/file_read
    pub fn get_file_path(&self) -> Option<String> {
        self.tool_use.input.get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Check if this is a file_write tool call
    pub fn is_file_write(&self) -> bool {
        self.tool_use.name == "file_write"
    }
    
    pub fn with_status(mut self, status: ToolStatus) -> Self {
        self.status = status;
        self
    }
    
    pub fn with_result(mut self, result: ToolResult) -> Self {
        self.status = if result.is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Success
        };
        self.result = Some(result);
        self.end_time = Some(Instant::now());
        self
    }
    
    pub fn start_execution(&mut self) {
        self.status = ToolStatus::Running;
        self.start_time = Some(Instant::now());
    }
    
    pub fn complete(&mut self, result: ToolResult) {
        self.status = if result.is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Success
        };
        // Phase 1: Auto-collapse successful tools, keep errors expanded
        if self.status == ToolStatus::Success {
            self.expanded = false;
        }
        self.result = Some(result);
        self.end_time = Some(Instant::now());
    }
    
    pub fn reject(&mut self) {
        self.status = ToolStatus::Rejected;
        self.end_time = Some(Instant::now());
    }
    
    pub fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }
    
    pub fn elapsed_ms(&self) -> Option<u64> {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => Some(end.duration_since(start).as_millis() as u64),
            (Some(start), None) => Some(start.elapsed().as_millis() as u64),
            _ => None,
        }
    }
    
    pub fn tool_id(&self) -> &str {
        &self.tool_use.id
    }
    
    pub fn tool_name(&self) -> &str {
        &self.tool_use.name
    }
    
    /// Get a summary of the tool input for display
    pub fn input_summary(&self) -> String {
        match self.tool_use.name.as_str() {
            "file_read" | "file_write" | "list_directory" | "file_delete" => {
                self.tool_use.input.get("path")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string())
            }
            "bash_execute" => {
                self.tool_use.input.get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| {
                        if s.len() > 50 {
                            format!("{}...", &s[..50])
                        } else {
                            s.to_string()
                        }
                    })
                    .unwrap_or_else(|| "<unknown>".to_string())
            }
            _ => serde_json::to_string(&self.tool_use.input)
                .map(|s| {
                    if s.len() > 60 {
                        format!("{}...", &s[..60])
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|_| "<error>".to_string())
        }
    }
    
    /// Get the result content as a string for display
    pub fn result_text(&self) -> Option<String> {
        self.result.as_ref().map(|r| match &r.content {
            ToolResultContent::Text(text) => text.clone(),
            ToolResultContent::Json(json) => {
                serde_json::to_string_pretty(json).unwrap_or_else(|_| json.to_string())
            }
            ToolResultContent::Image(_) => "[Image result]".to_string(),
        })
    }
}

/// Widget for rendering a tool call in the conversation
pub struct ToolCallWidget<'a> {
    tool_call: &'a ToolCall,
    theme: &'a Theme,
    spinner: Option<&'a Spinner>,
    selected: bool,
}

impl<'a> ToolCallWidget<'a> {
    pub fn new(tool_call: &'a ToolCall, theme: &'a Theme) -> Self {
        Self {
            tool_call,
            theme,
            spinner: None,
            selected: false,
        }
    }
    
    pub fn with_spinner(mut self, spinner: &'a Spinner) -> Self {
        self.spinner = Some(spinner);
        self
    }
    
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    
    /// Render the tool call as lines for embedding in conversation
    /// Phase 1: Compact summary format - icon + tool(args) + timing
    pub fn render_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Header line with expand/collapse indicator, status icon, tool name, and timing
        let expand_icon = if self.tool_call.expanded { "▼" } else { "▶" };

        let status_icon = if self.tool_call.status == ToolStatus::Running {
            self.spinner.map(|s| s.current_frame().to_string())
                .unwrap_or_else(|| "⋯".to_string())
        } else {
            self.tool_call.status.icon().to_string()
        };

        let status_color = self.status_color();
        let header_bg = if self.selected {
            self.theme.colors.muted.to_color()
        } else {
            Color::Reset
        };

        // Phase 1: Compact timing format (ms for <1s, otherwise Xs)
        let timing = self.tool_call.elapsed_ms()
            .map(|ms| {
                if ms < 1000 {
                    format!(" [{}ms]", ms)
                } else {
                    format!(" [{:.1}s]", ms as f64 / 1000.0)
                }
            })
            .unwrap_or_default();

        // Phase 1: Extra info for specific tools (file changes, etc.)
        let extra_info = self.get_compact_extra_info();

        // Phase 4: Get tool type icon for better visual hierarchy
        let tool_type_icon = self.get_tool_type_icon();

        // Phase 1+4: Compact format with tool type icons for visual hierarchy
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", expand_icon),
                Style::default().fg(self.theme.colors.muted.to_color()).bg(header_bg),
            ),
            Span::styled(
                format!("{} ", status_icon),
                Style::default().fg(status_color).bg(header_bg),
            ),
            Span::styled(
                format!("{} ", tool_type_icon),
                Style::default().fg(self.theme.colors.secondary.to_color()).bg(header_bg),
            ),
            Span::styled(
                self.tool_call.tool_name().to_string(),
                Style::default()
                    .fg(self.theme.colors.accent.to_color())
                    .bg(header_bg),
            ),
            Span::styled(
                format!("({})", self.tool_call.input_summary()),
                Style::default().fg(self.theme.colors.muted.to_color()).bg(header_bg),
            ),
            Span::styled(
                extra_info,
                Style::default().fg(self.theme.colors.success.to_color()).bg(header_bg),
            ),
            Span::styled(
                timing,
                Style::default().fg(self.theme.colors.muted.to_color()).bg(header_bg),
            ),
        ]));
        
        // Expanded content
        if self.tool_call.expanded {
            // Special handling for file_write: show diff view
            if self.tool_call.is_file_write() && self.tool_call.original_captured {
                lines.extend(self.render_file_write_diff());
            } else {
                // Default: show JSON input parameters
                lines.extend(self.render_json_input());
            }
            
            // Show result if available
            if let Some(result_text) = self.tool_call.result_text() {
                let result_color = if self.tool_call.result.as_ref().map(|r| r.is_error).unwrap_or(false) {
                    self.theme.colors.error.to_color()
                } else {
                    self.theme.colors.success.to_color()
                };
                
                lines.push(Line::from(Span::styled(
                    "    Result:",
                    Style::default()
                        .fg(result_color)
                        .add_modifier(Modifier::ITALIC),
                )));
                
                let max_result_lines = 12;
                let result_lines: Vec<&str> = result_text.lines().collect();
                let result_truncated = result_lines.len() > max_result_lines;
                
                for line in result_lines.iter().take(max_result_lines) {
                    lines.push(Line::from(Span::styled(
                        format!("      {}", line),
                        Style::default().fg(if self.tool_call.result.as_ref().map(|r| r.is_error).unwrap_or(false) {
                            self.theme.colors.error.to_color()
                        } else {
                            self.theme.colors.foreground.to_color()
                        }),
                    )));
                }
                
                if result_truncated {
                    lines.push(Line::from(Span::styled(
                        format!("      ... ({} more lines)", result_lines.len() - max_result_lines),
                        Style::default()
                            .fg(self.theme.colors.muted.to_color())
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
            }
            
            // Show pending action hint
            if self.tool_call.status == ToolStatus::Pending {
                lines.push(Line::from(vec![
                    Span::styled(
                        "    ",
                        Style::default(),
                    ),
                    Span::styled(
                        "[Y]",
                        Style::default()
                            .fg(self.theme.colors.success.to_color())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Execute  ",
                        Style::default().fg(self.theme.colors.muted.to_color()),
                    ),
                    Span::styled(
                        "[N]",
                        Style::default()
                            .fg(self.theme.colors.error.to_color())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Reject",
                        Style::default().fg(self.theme.colors.muted.to_color()),
                    ),
                ]));
            }
        }
        
        lines
    }

    /// Render file_write as a diff view
    fn render_file_write_diff(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        let path = self.tool_call.get_file_path().unwrap_or_else(|| "<unknown>".to_string());
        let new_content = self.tool_call.get_file_write_content().unwrap_or_default();

        let computer = DiffComputer::new();
        let renderer = DiffRenderer::new(self.theme);

        let diff_lines = if let Some(original) = &self.tool_call.original_content {
            // File exists: compute actual diff
            computer.compute(original, &new_content, &path)
        } else {
            // New file: all lines are additions
            computer.compute_new_file(&new_content, &path)
        };

        // Add summary line
        lines.push(renderer.render_summary(&diff_lines));

        // Render diff with max 40 lines
        lines.extend(renderer.render(&diff_lines, 40));

        lines
    }

    /// Render JSON input (default for non-file_write tools)
    fn render_json_input(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        let input_str = serde_json::to_string_pretty(&self.tool_call.tool_use.input)
            .unwrap_or_else(|_| self.tool_call.tool_use.input.to_string());

        let max_input_lines = 8;
        let input_lines: Vec<&str> = input_str.lines().collect();
        let input_truncated = input_lines.len() > max_input_lines;

        lines.push(Line::from(Span::styled(
            "    Input:",
            Style::default()
                .fg(self.theme.colors.primary.to_color())
                .add_modifier(Modifier::ITALIC),
        )));

        for line in input_lines.iter().take(max_input_lines) {
            lines.push(Line::from(Span::styled(
                format!("      {}", line),
                Style::default().fg(self.theme.colors.muted.to_color()),
            )));
        }

        if input_truncated {
            lines.push(Line::from(Span::styled(
                format!("      ... ({} more lines)", input_lines.len() - max_input_lines),
                Style::default()
                    .fg(self.theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        lines
    }

    fn status_color(&self) -> Color {
        match self.tool_call.status {
            ToolStatus::Pending => self.theme.colors.warning.to_color(),
            ToolStatus::Running => self.theme.colors.accent.to_color(),
            ToolStatus::Success => self.theme.colors.success.to_color(),
            ToolStatus::Error => self.theme.colors.error.to_color(),
            ToolStatus::Rejected => self.theme.colors.muted.to_color(),
            ToolStatus::Timeout => self.theme.colors.error.to_color(),
        }
    }

    /// Phase 4: Get tool type icon for visual categorization
    /// Different icons for file ops, search, shell, web, etc.
    fn get_tool_type_icon(&self) -> &'static str {
        match self.tool_call.tool_name() {
            // File operations
            "file_read" => "",      // File icon
            "file_write" => "",     // Pencil/write icon
            "edit" => "",           // Edit/pencil icon
            "glob" | "find" => "",  // Folder search

            // Search operations
            "grep" => "",           // Search icon
            "ast_search" => "",     // Code search

            // Shell operations
            "bash_execute" | "bash_output" | "bash_kill" => "", // Terminal

            // Web operations
            "web_fetch" | "web_search" => "󰖟",  // Globe

            // LSP operations
            "lsp_definition" | "lsp_references" | "lsp_hover" => "", // Code

            // Mandrel/MCP operations
            name if name.starts_with("mcp_") || name.starts_with("mandrel_") => "󱂛", // Database

            // Task/agent operations
            "task" | "subagent" => "󰜎", // Robot

            // Question/user interaction
            "ask_user" => "", // Chat bubble

            // Default
            _ => "󰡨", // Tool icon
        }
    }

    /// Phase 1: Get compact extra info for specific tool types
    /// Returns info like line counts for file_write, match counts for grep, etc.
    fn get_compact_extra_info(&self) -> String {
        if self.tool_call.status != ToolStatus::Success {
            return String::new();
        }

        match self.tool_call.tool_name() {
            "file_write" | "edit" => {
                // Show +lines/-lines from diff
                if let Some(ref original) = self.tool_call.original_content {
                    if let Some(new_content) = self.tool_call.get_file_write_content() {
                        let old_lines = original.lines().count();
                        let new_lines = new_content.lines().count();
                        let added = new_lines.saturating_sub(old_lines);
                        let removed = old_lines.saturating_sub(new_lines);
                        if added > 0 && removed > 0 {
                            return format!(" [+{} -{}]", added, removed);
                        } else if added > 0 {
                            return format!(" [+{}]", added);
                        } else if removed > 0 {
                            return format!(" [-{}]", removed);
                        }
                    }
                } else if self.tool_call.original_captured {
                    // New file
                    if let Some(new_content) = self.tool_call.get_file_write_content() {
                        let lines = new_content.lines().count();
                        return format!(" [+{} new]", lines);
                    }
                }
                String::new()
            }
            "file_read" => {
                // Show line count from result
                if let Some(result_text) = self.tool_call.result_text() {
                    let lines = result_text.lines().count();
                    if lines > 0 {
                        return format!(" [{} lines]", lines);
                    }
                }
                String::new()
            }
            "grep" => {
                // Show match count from result
                if let Some(result_text) = self.tool_call.result_text() {
                    let matches = result_text.lines().count();
                    if matches > 0 {
                        return format!(" [{} matches]", matches);
                    }
                }
                String::new()
            }
            "glob" => {
                // Show file count from result
                if let Some(result_text) = self.tool_call.result_text() {
                    let files = result_text.lines().count();
                    if files > 0 {
                        return format!(" [{} files]", files);
                    }
                }
                String::new()
            }
            "bash_execute" => {
                // Show exit code if available (lines of output)
                if let Some(result_text) = self.tool_call.result_text() {
                    let lines = result_text.lines().count();
                    if lines > 0 && lines <= 10 {
                        return format!(" [{} lines]", lines);
                    } else if lines > 10 {
                        return format!(" [{}+ lines]", lines);
                    }
                }
                String::new()
            }
            _ => String::new(),
        }
    }
}

/// Phase 4: Verbosity level for tool display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolVerbosity {
    /// Compact view - just summary line (collapsed by default)
    Compact,
    /// Normal view - shows details for selected tool
    #[default]
    Normal,
    /// Verbose view - all tools expanded with full details
    Verbose,
}

/// Manager for tracking tool calls in a conversation
#[derive(Debug, Clone, Default)]
pub struct ToolCallManager {
    tool_calls: Vec<ToolCall>,
    selected_index: Option<usize>,
    /// Phase 4: Verbosity setting for tool display
    verbosity: ToolVerbosity,
}

impl ToolCallManager {
    pub fn new() -> Self {
        Self {
            tool_calls: Vec::new(),
            selected_index: None,
            verbosity: ToolVerbosity::Normal,
        }
    }

    /// Phase 4: Get current verbosity level
    pub fn verbosity(&self) -> ToolVerbosity {
        self.verbosity
    }

    /// Phase 4: Set verbosity level
    pub fn set_verbosity(&mut self, verbosity: ToolVerbosity) {
        self.verbosity = verbosity;
    }

    /// Phase 4: Cycle through verbosity levels
    pub fn cycle_verbosity(&mut self) {
        self.verbosity = match self.verbosity {
            ToolVerbosity::Compact => ToolVerbosity::Normal,
            ToolVerbosity::Normal => ToolVerbosity::Verbose,
            ToolVerbosity::Verbose => ToolVerbosity::Compact,
        };
    }
    
    /// Register a new tool call
    pub fn add_tool_call(&mut self, tool_use: ToolUse) {
        let tool_call = ToolCall::new(tool_use);
        self.tool_calls.push(tool_call);
        // Select the newly added tool call
        self.selected_index = Some(self.tool_calls.len() - 1);
    }

    /// Register a new tool call with original file content (for file_write diff view)
    pub fn add_tool_call_with_original(&mut self, tool_use: ToolUse, original_content: Option<String>) {
        let tool_call = ToolCall::new(tool_use).with_original_content(original_content);
        self.tool_calls.push(tool_call);
        self.selected_index = Some(self.tool_calls.len() - 1);
    }
    
    /// Start execution of a tool call by ID
    pub fn start_execution(&mut self, tool_id: &str) {
        if let Some(tc) = self.tool_calls.iter_mut().find(|tc| tc.tool_id() == tool_id) {
            tc.start_execution();
        }
    }
    
    /// Complete a tool call with result
    pub fn complete_tool(&mut self, tool_id: &str, result: ToolResult) {
        if let Some(tc) = self.tool_calls.iter_mut().find(|tc| tc.tool_id() == tool_id) {
            tc.complete(result);
        }
    }
    
    /// Reject a tool call
    pub fn reject_tool(&mut self, tool_id: &str) {
        if let Some(tc) = self.tool_calls.iter_mut().find(|tc| tc.tool_id() == tool_id) {
            tc.reject();
        }
    }
    
    /// Get a tool call by ID
    pub fn get(&self, tool_id: &str) -> Option<&ToolCall> {
        self.tool_calls.iter().find(|tc| tc.tool_id() == tool_id)
    }
    
    /// Get a mutable tool call by ID
    pub fn get_mut(&mut self, tool_id: &str) -> Option<&mut ToolCall> {
        self.tool_calls.iter_mut().find(|tc| tc.tool_id() == tool_id)
    }
    
    /// Get all tool calls
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }
    
    /// Get the currently selected tool call
    pub fn selected(&self) -> Option<&ToolCall> {
        self.selected_index.and_then(|i| self.tool_calls.get(i))
    }
    
    /// Get the selected tool call mutably
    pub fn selected_mut(&mut self) -> Option<&mut ToolCall> {
        match self.selected_index {
            Some(i) => self.tool_calls.get_mut(i),
            None => None,
        }
    }
    
    /// Get the selected index
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }
    
    /// Select next tool call
    pub fn select_next(&mut self) {
        if self.tool_calls.is_empty() {
            return;
        }
        self.selected_index = Some(match self.selected_index {
            Some(i) => (i + 1) % self.tool_calls.len(),
            None => 0,
        });
    }
    
    /// Select previous tool call
    pub fn select_prev(&mut self) {
        if self.tool_calls.is_empty() {
            return;
        }
        self.selected_index = Some(match self.selected_index {
            Some(0) => self.tool_calls.len() - 1,
            Some(i) => i - 1,
            None => 0,
        });
    }
    
    /// Toggle expand/collapse of selected tool call
    pub fn toggle_selected(&mut self) {
        if let Some(tc) = self.selected_mut() {
            tc.toggle_expanded();
        }
    }
    
    /// Expand all tool calls
    pub fn expand_all(&mut self) {
        for tc in &mut self.tool_calls {
            tc.expanded = true;
        }
    }
    
    /// Collapse all tool calls
    pub fn collapse_all(&mut self) {
        for tc in &mut self.tool_calls {
            tc.expanded = false;
        }
    }
    
    /// Clear all tool calls (e.g., when conversation is cleared)
    pub fn clear(&mut self) {
        self.tool_calls.clear();
        self.selected_index = None;
    }
    
    /// Get the last pending tool call
    pub fn last_pending(&self) -> Option<&ToolCall> {
        self.tool_calls.iter().rev().find(|tc| tc.status == ToolStatus::Pending)
    }
    
    /// Get the last running tool call
    pub fn last_running(&self) -> Option<&ToolCall> {
        self.tool_calls.iter().rev().find(|tc| tc.status == ToolStatus::Running)
    }
    
    /// Check if there are any pending tool calls
    pub fn has_pending(&self) -> bool {
        self.tool_calls.iter().any(|tc| tc.status == ToolStatus::Pending)
    }
    
    /// Check if there are any running tool calls
    pub fn has_running(&self) -> bool {
        self.tool_calls.iter().any(|tc| tc.status == ToolStatus::Running)
    }
    
    /// Get count of tool calls
    pub fn len(&self) -> usize {
        self.tool_calls.len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.tool_calls.is_empty()
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
    
    fn create_test_result(tool_id: &str, is_error: bool) -> ToolResult {
        ToolResult {
            tool_use_id: tool_id.to_string(),
            content: ToolResultContent::Text("Test result".to_string()),
            is_error,
        }
    }

    #[test]
    fn test_tool_call_new() {
        let tool_use = create_test_tool_use("file_read");
        let tool_call = ToolCall::new(tool_use.clone());
        
        assert_eq!(tool_call.status, ToolStatus::Pending);
        assert!(tool_call.expanded);
        assert!(tool_call.result.is_none());
        assert!(tool_call.start_time.is_none());
        assert_eq!(tool_call.tool_name(), "file_read");
    }

    #[test]
    fn test_tool_call_execution_lifecycle() {
        let tool_use = create_test_tool_use("bash_execute");
        let mut tool_call = ToolCall::new(tool_use);

        assert_eq!(tool_call.status, ToolStatus::Pending);

        tool_call.start_execution();
        assert_eq!(tool_call.status, ToolStatus::Running);
        assert!(tool_call.start_time.is_some());

        let result = create_test_result("tool_bash_execute", false);
        tool_call.complete(result);
        assert_eq!(tool_call.status, ToolStatus::Success);
        assert!(tool_call.result.is_some());
        assert!(tool_call.end_time.is_some());
        // Phase 1: Successful tools auto-collapse
        assert!(!tool_call.expanded);
    }

    #[test]
    fn test_tool_call_auto_collapse_on_success() {
        let tool_use = create_test_tool_use("file_read");
        let mut tool_call = ToolCall::new(tool_use);

        // Starts expanded
        assert!(tool_call.expanded);

        // Complete with success - should auto-collapse
        let result = create_test_result("tool_file_read", false);
        tool_call.complete(result);
        assert_eq!(tool_call.status, ToolStatus::Success);
        assert!(!tool_call.expanded); // Auto-collapsed
    }

    #[test]
    fn test_tool_call_error_stays_expanded() {
        let tool_use = create_test_tool_use("file_read");
        let mut tool_call = ToolCall::new(tool_use);

        // Starts expanded
        assert!(tool_call.expanded);

        // Complete with error - should stay expanded
        let result = create_test_result("tool_file_read", true);
        tool_call.complete(result);
        assert_eq!(tool_call.status, ToolStatus::Error);
        assert!(tool_call.expanded); // Stays expanded for visibility
    }

    #[test]
    fn test_tool_call_rejection() {
        let tool_use = create_test_tool_use("file_write");
        let mut tool_call = ToolCall::new(tool_use);
        
        tool_call.reject();
        assert_eq!(tool_call.status, ToolStatus::Rejected);
        assert!(tool_call.end_time.is_some());
    }

    #[test]
    fn test_tool_call_error_result() {
        let tool_use = create_test_tool_use("file_read");
        let mut tool_call = ToolCall::new(tool_use);
        
        let result = create_test_result("tool_file_read", true);
        tool_call.complete(result);
        assert_eq!(tool_call.status, ToolStatus::Error);
    }

    #[test]
    fn test_tool_call_toggle_expanded() {
        let tool_use = create_test_tool_use("test");
        let mut tool_call = ToolCall::new(tool_use);
        
        assert!(tool_call.expanded);
        tool_call.toggle_expanded();
        assert!(!tool_call.expanded);
        tool_call.toggle_expanded();
        assert!(tool_call.expanded);
    }

    #[test]
    fn test_tool_call_input_summary() {
        let file_tool = ToolUse {
            id: "1".to_string(),
            name: "file_read".to_string(),
            input: json!({"path": "/home/user/test.txt"}),
        };
        let tool_call = ToolCall::new(file_tool);
        assert_eq!(tool_call.input_summary(), "/home/user/test.txt");
        
        let bash_tool = ToolUse {
            id: "2".to_string(),
            name: "bash_execute".to_string(),
            input: json!({"command": "ls -la"}),
        };
        let tool_call = ToolCall::new(bash_tool);
        assert_eq!(tool_call.input_summary(), "ls -la");
    }

    #[test]
    fn test_tool_call_manager_add() {
        let mut manager = ToolCallManager::new();
        assert!(manager.is_empty());
        
        manager.add_tool_call(create_test_tool_use("tool1"));
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.selected_index(), Some(0));
        
        manager.add_tool_call(create_test_tool_use("tool2"));
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.selected_index(), Some(1)); // New tool is selected
    }

    #[test]
    fn test_tool_call_manager_navigation() {
        let mut manager = ToolCallManager::new();
        manager.add_tool_call(create_test_tool_use("tool1"));
        manager.add_tool_call(create_test_tool_use("tool2"));
        manager.add_tool_call(create_test_tool_use("tool3"));
        
        assert_eq!(manager.selected_index(), Some(2));
        
        manager.select_prev();
        assert_eq!(manager.selected_index(), Some(1));
        
        manager.select_prev();
        assert_eq!(manager.selected_index(), Some(0));
        
        manager.select_prev(); // Wraps around
        assert_eq!(manager.selected_index(), Some(2));
        
        manager.select_next();
        assert_eq!(manager.selected_index(), Some(0)); // Wraps around
    }

    #[test]
    fn test_tool_call_manager_execution() {
        let mut manager = ToolCallManager::new();
        manager.add_tool_call(create_test_tool_use("test"));
        
        assert!(manager.has_pending());
        assert!(!manager.has_running());
        
        manager.start_execution("tool_test");
        assert!(!manager.has_pending());
        assert!(manager.has_running());
        
        manager.complete_tool("tool_test", create_test_result("tool_test", false));
        assert!(!manager.has_running());
    }

    #[test]
    fn test_tool_call_manager_clear() {
        let mut manager = ToolCallManager::new();
        manager.add_tool_call(create_test_tool_use("tool1"));
        manager.add_tool_call(create_test_tool_use("tool2"));
        
        assert_eq!(manager.len(), 2);
        
        manager.clear();
        assert!(manager.is_empty());
        assert!(manager.selected_index().is_none());
    }

    #[test]
    fn test_tool_call_manager_expand_collapse() {
        let mut manager = ToolCallManager::new();
        manager.add_tool_call(create_test_tool_use("tool1"));
        manager.add_tool_call(create_test_tool_use("tool2"));
        
        // Default is expanded
        for tc in manager.tool_calls() {
            assert!(tc.expanded);
        }
        
        manager.collapse_all();
        for tc in manager.tool_calls() {
            assert!(!tc.expanded);
        }
        
        manager.expand_all();
        for tc in manager.tool_calls() {
            assert!(tc.expanded);
        }
    }

    #[test]
    fn test_tool_status_icons() {
        assert_eq!(ToolStatus::Pending.icon(), "⏳");
        assert_eq!(ToolStatus::Running.icon(), "⋯");
        assert_eq!(ToolStatus::Success.icon(), "✓");
        assert_eq!(ToolStatus::Error.icon(), "✗");
        assert_eq!(ToolStatus::Rejected.icon(), "⊘");
        assert_eq!(ToolStatus::Timeout.icon(), "⏱");
    }

    #[test]
    fn test_tool_status_labels() {
        assert_eq!(ToolStatus::Pending.label(), "Pending");
        assert_eq!(ToolStatus::Running.label(), "Running");
        assert_eq!(ToolStatus::Success.label(), "Success");
        assert_eq!(ToolStatus::Error.label(), "Error");
        assert_eq!(ToolStatus::Rejected.label(), "Rejected");
        assert_eq!(ToolStatus::Timeout.label(), "Timeout");
    }

    #[test]
    fn test_tool_verbosity_cycle() {
        // Phase 4: Test verbosity cycling
        let mut manager = ToolCallManager::new();

        // Default is Normal
        assert_eq!(manager.verbosity(), ToolVerbosity::Normal);

        // Cycle: Normal -> Verbose
        manager.cycle_verbosity();
        assert_eq!(manager.verbosity(), ToolVerbosity::Verbose);

        // Cycle: Verbose -> Compact
        manager.cycle_verbosity();
        assert_eq!(manager.verbosity(), ToolVerbosity::Compact);

        // Cycle: Compact -> Normal
        manager.cycle_verbosity();
        assert_eq!(manager.verbosity(), ToolVerbosity::Normal);
    }
}
