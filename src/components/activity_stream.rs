use crossterm::event::{Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;
use crate::spindles::{ActivityMessage, SharedActivityStore};

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

    fn render_activity<'a>(
        activity: &'a ActivityMessage,
        theme: &'a Theme,
        tool_name_lookup: Option<&str>,
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
                vec![Line::from(vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(a.content.clone(), content_style),
                ])]
            }
            ActivityMessage::ToolCall(tc) => {
                vec![Line::from(vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(tc.tool_name.clone(), Style::default().fg(theme.colors.accent.to_color()).add_modifier(Modifier::BOLD)),
                ])]
            }
            ActivityMessage::ToolResult(tr) => {
                let result_style = if tr.is_error {
                    Style::default().fg(theme.colors.error.to_color())
                } else {
                    Style::default().fg(theme.colors.success.to_color())
                };
                let status = if tr.is_error { "failed" } else { "succeeded" };
                // Use looked-up tool name if available, otherwise show truncated ID
                let display_name = tool_name_lookup
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        // Show last 8 chars of tool_id as fallback
                        let id = &tr.tool_id;
                        if id.len() > 12 {
                            format!("...{}", &id[id.len()-8..])
                        } else {
                            id.clone()
                        }
                    });
                vec![Line::from(vec![
                    Span::styled(format!("[{}] ", time_short), time_style),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(display_name, Style::default().fg(theme.colors.accent.to_color()).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled(status.to_string(), result_style),
                ])]
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
        let (instance_info, activities_with_names): (Option<(u32, u32)>, Vec<(ActivityMessage, Option<String>)>) = {
            let store = self.store.lock().unwrap();

            // Get current instance info from store (updated from incoming activities)
            let instance_info = store.current_instance();

            let activities = store.get_visible(self.scroll_offset, area.height.saturating_sub(2) as usize);

            // Clone activities and look up tool names while we have the lock
            let activities_with_names: Vec<(ActivityMessage, Option<String>)> = activities
                .into_iter()
                .map(|activity| {
                    let tool_name = if let ActivityMessage::ToolResult(tr) = activity {
                        store.get_tool_name(&tr.tool_id).map(|s| s.to_string())
                    } else {
                        None
                    };
                    (activity.clone(), tool_name)
                })
                .collect();

            (instance_info, activities_with_names)
        }; // store lock released here

        // Now render without holding the lock
        let lines: Vec<Line> = activities_with_names
            .iter()
            .flat_map(|(activity, tool_name)| {
                Self::render_activity(activity, theme, tool_name.as_deref())
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
