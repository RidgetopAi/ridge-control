use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::config::Theme;

const DEFAULT_DURATION_MS: u64 = 4000;
const MAX_VISIBLE_NOTIFICATIONS: usize = 5;
const NOTIFICATION_WIDTH: u16 = 40;
const NOTIFICATION_HEIGHT: u16 = 3;
const NOTIFICATION_MARGIN: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationLevel {
    pub fn icon(&self) -> &'static str {
        match self {
            NotificationLevel::Info => "󰋼",
            NotificationLevel::Success => "󰄬",
            NotificationLevel::Warning => "󰀦",
            NotificationLevel::Error => "󰅚",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u64,
    pub level: NotificationLevel,
    pub title: String,
    pub message: Option<String>,
    pub created_at: Instant,
    pub duration: Duration,
    pub dismissable: bool,
}

impl Notification {
    pub fn new(level: NotificationLevel, title: impl Into<String>) -> Self {
        Self {
            id: 0,
            level,
            title: title.into(),
            message: None,
            created_at: Instant::now(),
            duration: Duration::from_millis(DEFAULT_DURATION_MS),
            dismissable: true,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn persistent(mut self) -> Self {
        self.duration = Duration::from_secs(3600);
        self
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    fn remaining_ratio(&self) -> f32 {
        let elapsed = self.created_at.elapsed().as_millis() as f32;
        let total = self.duration.as_millis() as f32;
        (1.0 - elapsed / total).max(0.0)
    }
}

pub struct NotificationManager {
    notifications: VecDeque<Notification>,
    next_id: u64,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: VecDeque::new(),
            next_id: 1,
        }
    }

    pub fn push(&mut self, mut notification: Notification) {
        notification.id = self.next_id;
        self.next_id += 1;
        self.notifications.push_back(notification);
        
        while self.notifications.len() > MAX_VISIBLE_NOTIFICATIONS * 2 {
            self.notifications.pop_front();
        }
    }

    pub fn info(&mut self, title: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Info, title));
    }

    pub fn info_with_message(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Info, title).with_message(message));
    }

    pub fn success(&mut self, title: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Success, title));
    }

    pub fn success_with_message(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Success, title).with_message(message));
    }

    pub fn warning(&mut self, title: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Warning, title));
    }

    pub fn warning_with_message(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Warning, title).with_message(message));
    }

    pub fn error(&mut self, title: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Error, title));
    }

    pub fn error_with_message(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(Notification::new(NotificationLevel::Error, title).with_message(message));
    }

    pub fn tick(&mut self) {
        self.notifications.retain(|n| !n.is_expired());
    }

    pub fn dismiss_first(&mut self) {
        if let Some(n) = self.notifications.front() {
            if n.dismissable {
                self.notifications.pop_front();
            }
        }
    }

    pub fn dismiss_all(&mut self) {
        self.notifications.retain(|n| !n.dismissable);
    }

    pub fn dismiss_by_id(&mut self, id: u64) {
        self.notifications.retain(|n| n.id != id || !n.dismissable);
    }

    pub fn has_notifications(&self) -> bool {
        !self.notifications.is_empty()
    }

    pub fn count(&self) -> usize {
        self.notifications.len()
    }

    pub fn visible(&self) -> impl Iterator<Item = &Notification> {
        self.notifications.iter().take(MAX_VISIBLE_NOTIFICATIONS)
    }

    pub fn render(&self, frame: &mut Frame, screen: Rect, theme: &Theme) {
        if self.notifications.is_empty() {
            return;
        }

        let visible: Vec<_> = self.visible().collect();
        let count = visible.len();

        for (idx, notification) in visible.iter().enumerate() {
            let height = if notification.message.is_some() {
                NOTIFICATION_HEIGHT + 1
            } else {
                NOTIFICATION_HEIGHT
            };

            let y_offset = (idx as u16) * (height + NOTIFICATION_MARGIN);
            let x = screen.width.saturating_sub(NOTIFICATION_WIDTH + 2);
            let y = screen.y + 1 + y_offset;

            if y + height > screen.height {
                break;
            }

            let area = Rect::new(x, y, NOTIFICATION_WIDTH, height);
            self.render_notification(frame, area, notification, theme);
        }

        if self.notifications.len() > MAX_VISIBLE_NOTIFICATIONS {
            let more_count = self.notifications.len() - MAX_VISIBLE_NOTIFICATIONS;
            let more_y = 1 + (count as u16) * (NOTIFICATION_HEIGHT + NOTIFICATION_MARGIN);
            if more_y < screen.height {
                let more_area = Rect::new(
                    screen.width.saturating_sub(NOTIFICATION_WIDTH + 2),
                    more_y,
                    NOTIFICATION_WIDTH,
                    1,
                );
                let more_text = format!("... and {} more", more_count);
                let style = Style::default().fg(theme.colors.muted.to_color());
                frame.render_widget(
                    Paragraph::new(more_text).style(style).alignment(Alignment::Right),
                    more_area,
                );
            }
        }
    }

    fn render_notification(
        &self,
        frame: &mut Frame,
        area: Rect,
        notification: &Notification,
        theme: &Theme,
    ) {
        frame.render_widget(Clear, area);

        let (fg, bg) = match notification.level {
            NotificationLevel::Info => (
                theme.notifications.info_fg.to_color(),
                theme.notifications.info_bg.to_color(),
            ),
            NotificationLevel::Success => (
                theme.notifications.success_fg.to_color(),
                theme.notifications.success_bg.to_color(),
            ),
            NotificationLevel::Warning => (
                theme.notifications.warning_fg.to_color(),
                theme.notifications.warning_bg.to_color(),
            ),
            NotificationLevel::Error => (
                theme.notifications.error_fg.to_color(),
                theme.notifications.error_bg.to_color(),
            ),
        };

        let icon = notification.level.icon();
        let title = format!("{} {}", icon, notification.title);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg).bg(bg))
            .style(Style::default().bg(bg));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let title_style = Style::default()
            .fg(fg)
            .bg(bg)
            .add_modifier(Modifier::BOLD);

        let mut lines = vec![Line::from(Span::styled(
            truncate_string(&title, inner.width as usize),
            title_style,
        ))];

        if let Some(ref msg) = notification.message {
            let msg_style = Style::default().fg(fg).bg(bg);
            lines.push(Line::from(Span::styled(
                truncate_string(msg, inner.width as usize),
                msg_style,
            )));
        }

        let progress_width = ((inner.width as f32) * notification.remaining_ratio()) as usize;
        let progress_bar: String = "─".repeat(progress_width);
        let progress_style = Style::default()
            .fg(fg)
            .bg(bg)
            .add_modifier(Modifier::DIM);
        lines.push(Line::from(Span::styled(progress_bar, progress_style)));

        let para = Paragraph::new(lines)
            .style(Style::default().bg(bg))
            .wrap(Wrap { trim: true });
        frame.render_widget(para, inner);
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else if max_len > 3 {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    } else {
        s.chars().take(max_len).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_new() {
        let n = Notification::new(NotificationLevel::Info, "Test");
        assert_eq!(n.title, "Test");
        assert!(n.message.is_none());
        assert!(n.dismissable);
    }

    #[test]
    fn test_notification_with_message() {
        let n = Notification::new(NotificationLevel::Success, "Title")
            .with_message("Body text");
        assert_eq!(n.title, "Title");
        assert_eq!(n.message.as_deref(), Some("Body text"));
    }

    #[test]
    fn test_notification_with_duration() {
        let n = Notification::new(NotificationLevel::Warning, "Test")
            .with_duration(Duration::from_secs(10));
        assert_eq!(n.duration, Duration::from_secs(10));
    }

    #[test]
    fn test_notification_persistent() {
        let n = Notification::new(NotificationLevel::Error, "Test").persistent();
        assert!(n.duration >= Duration::from_secs(3600));
    }

    #[test]
    fn test_notification_is_expired() {
        let n = Notification::new(NotificationLevel::Info, "Test")
            .with_duration(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        assert!(n.is_expired());
    }

    #[test]
    fn test_notification_level_icon() {
        assert!(!NotificationLevel::Info.icon().is_empty());
        assert!(!NotificationLevel::Success.icon().is_empty());
        assert!(!NotificationLevel::Warning.icon().is_empty());
        assert!(!NotificationLevel::Error.icon().is_empty());
    }

    #[test]
    fn test_manager_new() {
        let mgr = NotificationManager::new();
        assert!(!mgr.has_notifications());
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_manager_push() {
        let mut mgr = NotificationManager::new();
        mgr.push(Notification::new(NotificationLevel::Info, "Test"));
        assert!(mgr.has_notifications());
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn test_manager_convenience_methods() {
        let mut mgr = NotificationManager::new();
        mgr.info("Info");
        mgr.success("Success");
        mgr.warning("Warning");
        mgr.error("Error");
        assert_eq!(mgr.count(), 4);
    }

    #[test]
    fn test_manager_tick_removes_expired() {
        let mut mgr = NotificationManager::new();
        mgr.push(
            Notification::new(NotificationLevel::Info, "Test")
                .with_duration(Duration::from_millis(1)),
        );
        std::thread::sleep(Duration::from_millis(10));
        mgr.tick();
        assert!(!mgr.has_notifications());
    }

    #[test]
    fn test_manager_dismiss_first() {
        let mut mgr = NotificationManager::new();
        mgr.info("First");
        mgr.info("Second");
        mgr.dismiss_first();
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn test_manager_dismiss_all() {
        let mut mgr = NotificationManager::new();
        mgr.info("One");
        mgr.info("Two");
        mgr.info("Three");
        mgr.dismiss_all();
        assert!(!mgr.has_notifications());
    }

    #[test]
    fn test_manager_visible_limit() {
        let mut mgr = NotificationManager::new();
        for i in 0..10 {
            mgr.info(format!("Notification {}", i));
        }
        let visible_count = mgr.visible().count();
        assert_eq!(visible_count, MAX_VISIBLE_NOTIFICATIONS);
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("a very long string", 10), "a very lo…");
        assert_eq!(truncate_string("abc", 3), "abc");
    }
}
