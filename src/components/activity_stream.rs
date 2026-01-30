use crossterm::event::{Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;
use crate::spindles::{ActivityMessage, SharedActivityStore, ToolCallInfo};

pub struct ActivityStream {
    store: SharedActivityStore,
    scroll_offset: usize,
    visible_height: usize,
    auto_scroll: bool,
    header_run_name: Option<String>,
    header_instance: Option<(u32, u32)>,
}

impl ActivityStream {
    pub fn new(store: SharedActivityStore) -> Self {
        Self {
            store,
            scroll_offset: 0,
            visible_height: 10,
            auto_scroll: true,
            header_run_name: None,
            header_instance: None,
        }
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height.saturating_sub(3);
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, n: usize) {
        let store = self.store.lock().unwrap();
        let max_offset = store.filtered_len().saturating_sub(self.visible_height);
        drop(store);

        self.scroll_offset = (self.scroll_offset + n).min(max_offset);

        if self.scroll_offset >= max_offset {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        let store = self.store.lock().unwrap();
        let max_offset = store.filtered_len().saturating_sub(self.visible_height);
        drop(store);

        self.scroll_offset = max_offset;
        self.auto_scroll = true;
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

    pub fn clear(&mut self) {
        let mut store = self.store.lock().unwrap();
        store.clear();
        self.scroll_offset = 0;
    }

    /// Push a text message to the activity stream (used for stderr output)
    pub fn push_text(&mut self, content: String, timestamp: String) {
        use crate::spindles::TextActivity;
        let activity = ActivityMessage::Text(TextActivity {
            content,
            timestamp,
            session: None,
        });
        let mut store = self.store.lock().unwrap();
        store.push(activity);
    }

    pub fn update_header(&mut self, run_name: Option<String>, instance: Option<(u32, u32)>) {
        self.header_run_name = run_name;
        self.header_instance = instance;
    }

    /// Extract display-relevant detail from tool input based on tool type
    fn extract_tool_detail(tool_name: &str, input: &serde_json::Value) -> Option<String> {
        match tool_name {
            // Edit operations - show path and diff stats
            "Edit" | "edit_file" | "file_edit" => {
                let mut parts = Vec::new();
                if let Some(path) = input.get("file_path").or_else(|| input.get("path")) {
                    if let Some(s) = path.as_str() {
                        parts.push(Self::truncate_path(s, 40));
                    }
                }
                // Compute line diff from old_string/new_string
                if let (Some(old), Some(new)) = (input.get("old_string"), input.get("new_string")) {
                    if let (Some(old_s), Some(new_s)) = (old.as_str(), new.as_str()) {
                        let old_lines = old_s.lines().count();
                        let new_lines = new_s.lines().count();
                        let added = new_lines.saturating_sub(old_lines);
                        let removed = old_lines.saturating_sub(new_lines);
                        if added > 0 || removed > 0 {
                            parts.push(format!("[+{} -{}]", added, removed));
                        } else {
                            parts.push(format!("[~{} lines]", old_lines.max(1)));
                        }
                    }
                }
                if parts.is_empty() { None } else { Some(parts.join(" ")) }
            }
            // Write operations - show path and line count
            "Write" | "file_write" => {
                let mut parts = Vec::new();
                if let Some(path) = input.get("file_path").or_else(|| input.get("path")) {
                    if let Some(s) = path.as_str() {
                        parts.push(Self::truncate_path(s, 40));
                    }
                }
                if let Some(content) = input.get("content") {
                    if let Some(s) = content.as_str() {
                        let lines = s.lines().count();
                        parts.push(format!("[{} lines]", lines));
                    }
                }
                if parts.is_empty() { None } else { Some(parts.join(" ")) }
            }
            // Read operations - show path
            "Read" | "file_read" => {
                if let Some(path) = input.get("file_path").or_else(|| input.get("path")) {
                    if let Some(s) = path.as_str() {
                        return Some(Self::truncate_path(s, 50));
                    }
                }
                None
            }
            // Glob - show pattern
            "Glob" | "glob" | "NotebookEdit" => {
                if let Some(path) = input.get("file_path").or_else(|| input.get("path")) {
                    if let Some(s) = path.as_str() {
                        return Some(Self::truncate_path(s, 50));
                    }
                }
                if let Some(pattern) = input.get("pattern") {
                    if let Some(s) = pattern.as_str() {
                        return Some(format!("\"{}\"", Self::truncate_str(s, 40)));
                    }
                }
                None
            }
            // Bash - show command
            "Bash" | "bash" => {
                if let Some(cmd) = input.get("command") {
                    if let Some(s) = cmd.as_str() {
                        // Show first line of command, truncated
                        let first_line = s.lines().next().unwrap_or(s);
                        return Some(Self::truncate_str(first_line, 60));
                    }
                }
                None
            }
            // Grep - show pattern and optional path
            "Grep" | "grep" => {
                let mut parts = Vec::new();
                if let Some(pattern) = input.get("pattern") {
                    if let Some(s) = pattern.as_str() {
                        parts.push(format!("/{}/ ", Self::truncate_str(s, 30)));
                    }
                }
                if let Some(path) = input.get("path") {
                    if let Some(s) = path.as_str() {
                        parts.push(Self::truncate_path(s, 30));
                    }
                }
                if parts.is_empty() { None } else { Some(parts.join("")) }
            }
            // Task - show description
            "Task" => {
                if let Some(desc) = input.get("description") {
                    if let Some(s) = desc.as_str() {
                        return Some(Self::truncate_str(s, 50));
                    }
                }
                None
            }
            // WebFetch/WebSearch - show URL or query
            "WebFetch" | "WebSearch" => {
                if let Some(url) = input.get("url") {
                    if let Some(s) = url.as_str() {
                        return Some(Self::truncate_str(s, 50));
                    }
                }
                if let Some(query) = input.get("query") {
                    if let Some(s) = query.as_str() {
                        return Some(format!("\"{}\"", Self::truncate_str(s, 45)));
                    }
                }
                None
            }
            // Mandrel MCP tools - extract meaningful params
            name if name.starts_with("mcp__") || name.contains("context_")
                || name.contains("project_") || name.contains("task_")
                || name.contains("decision_") => {
                // Try common parameter names
                if let Some(content) = input.get("content") {
                    if let Some(s) = content.as_str() {
                        return Some(Self::truncate_str(s, 40));
                    }
                }
                if let Some(query) = input.get("query") {
                    if let Some(s) = query.as_str() {
                        return Some(format!("\"{}\"", Self::truncate_str(s, 35)));
                    }
                }
                if let Some(project) = input.get("project") {
                    if let Some(s) = project.as_str() {
                        return Some(format!("→ {}", s));
                    }
                }
                if let Some(title) = input.get("title") {
                    if let Some(s) = title.as_str() {
                        return Some(Self::truncate_str(s, 40));
                    }
                }
                if let Some(name_val) = input.get("name") {
                    if let Some(s) = name_val.as_str() {
                        return Some(s.to_string());
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Truncate a file path, preferring to show the end (filename and parent)
    fn truncate_path(path: &str, max_len: usize) -> String {
        if path.len() <= max_len {
            return path.to_string();
        }
        // Show .../<parent>/<file>
        let parts: Vec<&str> = path.rsplit('/').take(2).collect();
        let suffix = parts.into_iter().rev().collect::<Vec<_>>().join("/");
        if suffix.len() + 4 <= max_len {
            format!(".../{}", suffix)
        } else {
            format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
        }
    }

    /// Truncate a string with ellipsis
    fn truncate_str(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len.saturating_sub(3)])
        }
    }

    fn render_activity<'a>(
        activity: &'a ActivityMessage,
        theme: &'a Theme,
        tool_info_lookup: Option<&ToolCallInfo>,
    ) -> Vec<Line<'a>> {
        let icon = activity.icon();
        let timestamp = activity.timestamp();
        let time_short = if timestamp.len() > 10 {
            &timestamp[11..19]
        } else {
            timestamp
        };

        let time_style = Style::default().fg(theme.colors.muted.to_color());

        match activity {
            ActivityMessage::Thinking(a) => {
                let content_style = Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC);
                // Truncate thinking to first line, max 80 chars
                let first_line = a.content.lines().next().unwrap_or(&a.content);
                let truncated = Self::truncate_str(first_line.trim(), 80);
                vec![Line::from(vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(truncated, content_style),
                ])]
            }
            ActivityMessage::ToolCall(tc) => {
                let tool_style = Style::default().fg(theme.colors.accent.to_color()).add_modifier(Modifier::BOLD);
                let detail_style = Style::default().fg(theme.colors.muted.to_color());

                // Extract relevant info from tool input
                let detail = Self::extract_tool_detail(&tc.tool_name, &tc.input);

                let mut spans = vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(tc.tool_name.clone(), tool_style),
                ];

                if let Some(detail_text) = detail {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(detail_text, detail_style));
                }

                vec![Line::from(spans)]
            }
            ActivityMessage::ToolResult(tr) => {
                let result_style = if tr.is_error {
                    Style::default().fg(theme.colors.error.to_color())
                } else {
                    Style::default().fg(theme.colors.success.to_color())
                };
                let tool_style = Style::default().fg(theme.colors.accent.to_color()).add_modifier(Modifier::BOLD);
                let detail_style = Style::default().fg(theme.colors.muted.to_color());
                let status = if tr.is_error { "failed" } else { "succeeded" };

                // Use looked-up tool info if available
                let (display_name, detail) = tool_info_lookup
                    .map(|info| {
                        let detail = Self::extract_tool_detail(&info.tool_name, &info.input);
                        (info.tool_name.clone(), detail)
                    })
                    .unwrap_or_else(|| {
                        // Fallback: show truncated tool_id
                        let id = &tr.tool_id;
                        let name = if id.len() > 12 {
                            format!("...{}", &id[id.len()-8..])
                        } else {
                            id.clone()
                        };
                        (name, None)
                    });

                let mut spans = vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(display_name, tool_style),
                ];

                if let Some(detail_text) = detail {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(detail_text, detail_style));
                }

                spans.push(Span::raw(" "));
                spans.push(Span::styled(status.to_string(), result_style));

                vec![Line::from(spans)]
            }
            ActivityMessage::Text(t) => {
                vec![Line::from(vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::raw(t.content.clone()),
                ])]
            }
            ActivityMessage::Error(e) => {
                let error_style = Style::default().fg(theme.colors.error.to_color()).add_modifier(Modifier::BOLD);
                vec![Line::from(vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(e.message.clone(), error_style),
                ])]
            }
        }
    }
}

impl Component for ActivityStream {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(KeyEvent { code, modifiers, .. }) => match code {
                KeyCode::Up => {
                    self.scroll_up(1);
                    Some(Action::Noop)
                }
                KeyCode::Down => {
                    self.scroll_down(1);
                    Some(Action::Noop)
                }
                KeyCode::PageUp => {
                    self.scroll_up(self.visible_height);
                    Some(Action::Noop)
                }
                KeyCode::PageDown => {
                    self.scroll_down(self.visible_height);
                    Some(Action::Noop)
                }
                KeyCode::Home => {
                    self.scroll_offset = 0;
                    self.auto_scroll = false;
                    Some(Action::Noop)
                }
                KeyCode::End => {
                    self.scroll_to_bottom();
                    Some(Action::Noop)
                }
                KeyCode::Char('a') if modifiers.is_empty() => {
                    self.toggle_auto_scroll();
                    Some(Action::Noop)
                }
                _ => None,
            },
            Event::Mouse(MouseEvent { kind, .. }) => match kind {
                MouseEventKind::ScrollUp => {
                    self.scroll_up(3);
                    Some(Action::Noop)
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_down(3);
                    Some(Action::Noop)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::ActivityStreamClear => self.clear(),
            Action::ActivityStreamToggleAutoScroll => self.toggle_auto_scroll(),
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = if focused {
            Style::default().fg(theme.focus.focused_border.to_color())
        } else {
            Style::default().fg(theme.focus.unfocused_border.to_color())
        };

        // Collect all data from store under lock, then release
        let (instance_info, activities_with_info): (Option<(u32, u32)>, Vec<(ActivityMessage, Option<ToolCallInfo>)>) = {
            let store = self.store.lock().unwrap();

            // Get current instance info from store (updated from incoming activities)
            let instance_info = store.current_instance();

            let activities = store.get_visible(self.scroll_offset, area.height.saturating_sub(2) as usize);

            // Clone activities and look up tool info while we have the lock
            let activities_with_info: Vec<(ActivityMessage, Option<ToolCallInfo>)> = activities
                .into_iter()
                .map(|activity| {
                    let tool_info = if let ActivityMessage::ToolResult(tr) = activity {
                        store.get_tool_info(&tr.tool_id).cloned()
                    } else {
                        None
                    };
                    (activity.clone(), tool_info)
                })
                .collect();

            (instance_info, activities_with_info)
        }; // store lock released here

        // Now render without holding the lock
        let lines: Vec<Line> = activities_with_info
            .iter()
            .flat_map(|(activity, tool_info)| {
                Self::render_activity(activity, theme, tool_info.as_ref())
            })
            .collect();

        let header_text = match (&self.header_run_name, self.header_instance.or(instance_info)) {
            (Some(name), Some((current, total))) => {
                format!(" Activity Stream - {} | Instance {}/{} ", name, current, total)
            }
            (Some(name), None) => format!(" Activity Stream - {} ", name),
            _ => " Activity Stream ".to_string(),
        };

        let auto_scroll_indicator = if self.auto_scroll { "▼" } else { "○" };

        // Build bottom title with run indicator
        let bottom_title = match instance_info {
            Some((current, total)) => format!(" Run: {}/{} │ {} Auto-scroll ", current, total, auto_scroll_indicator),
            None => format!(" {} Auto-scroll ", auto_scroll_indicator),
        };

        let block = Block::default()
            .title(header_text)
            .title_bottom(bottom_title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 {
            return;
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }
}
