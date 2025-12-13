// TRC-020: Context menus - some methods for future use
#![allow(dead_code)]

use crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use crate::action::{Action, ContextMenuTarget};
use crate::config::Theme;

/// A context menu item with label, optional shortcut hint, and action
#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    pub label: String,
    pub shortcut: Option<String>,
    pub action: Action,
    pub enabled: bool,
    pub is_separator: bool,
}

impl ContextMenuItem {
    pub fn new(label: impl Into<String>, action: Action) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
            action,
            enabled: true,
            is_separator: false,
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn separator() -> Self {
        Self {
            label: String::new(),
            shortcut: None,
            action: Action::None,
            enabled: false,
            is_separator: true,
        }
    }
}

/// Right-click context menu component
pub struct ContextMenu {
    visible: bool,
    position: (u16, u16),
    items: Vec<ContextMenuItem>,
    selected: usize,
    target: ContextMenuTarget,
    max_width: u16,
}

impl ContextMenu {
    pub fn new() -> Self {
        Self {
            visible: false,
            position: (0, 0),
            items: Vec::new(),
            selected: 0,
            target: ContextMenuTarget::Generic,
            max_width: 30,
        }
    }

    pub fn show(&mut self, x: u16, y: u16, target: ContextMenuTarget, items: Vec<ContextMenuItem>) {
        self.visible = true;
        self.position = (x, y);
        self.target = target;
        self.items = items;
        self.selected = 0;
        
        // Skip to first non-separator enabled item
        self.skip_to_next_valid();
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.items.clear();
        self.selected = 0;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn target(&self) -> &ContextMenuTarget {
        &self.target
    }

    pub fn position(&self) -> (u16, u16) {
        self.position
    }

    fn skip_to_next_valid(&mut self) {
        if self.items.is_empty() {
            return;
        }
        
        let start = self.selected;
        loop {
            if let Some(item) = self.items.get(self.selected) {
                if item.enabled && !item.is_separator {
                    return;
                }
            }
            self.selected = (self.selected + 1) % self.items.len();
            if self.selected == start {
                break;
            }
        }
    }

    fn skip_to_prev_valid(&mut self) {
        if self.items.is_empty() {
            return;
        }
        
        let start = self.selected;
        loop {
            if let Some(item) = self.items.get(self.selected) {
                if item.enabled && !item.is_separator {
                    return;
                }
            }
            if self.selected == 0 {
                self.selected = self.items.len().saturating_sub(1);
            } else {
                self.selected -= 1;
            }
            if self.selected == start {
                break;
            }
        }
    }

    fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.items.len();
        self.skip_to_next_valid();
    }

    fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.items.len().saturating_sub(1);
        } else {
            self.selected -= 1;
        }
        self.skip_to_prev_valid();
    }

    fn confirm_selection(&mut self) -> Option<Action> {
        if let Some(item) = self.items.get(self.selected) {
            if item.enabled && !item.is_separator {
                let action = item.action.clone();
                self.hide();
                return Some(action);
            }
        }
        None
    }

    fn calculate_menu_rect(&self, screen: Rect) -> Rect {
        let item_count = self.items.len() as u16;
        
        // Calculate width based on longest item
        let content_width = self.items.iter()
            .filter(|item| !item.is_separator)
            .map(|item| {
                let shortcut_len = item.shortcut.as_ref().map(|s| s.len() + 3).unwrap_or(0);
                item.label.len() + shortcut_len
            })
            .max()
            .unwrap_or(10) as u16;
        
        let width = (content_width + 4).min(self.max_width).max(15);
        let height = item_count + 2; // +2 for borders
        
        // Adjust position to fit on screen
        let mut x = self.position.0;
        let mut y = self.position.1;
        
        // Prevent overflow to the right
        if x + width > screen.width {
            x = screen.width.saturating_sub(width);
        }
        
        // Prevent overflow to the bottom
        if y + height > screen.height {
            y = screen.height.saturating_sub(height);
        }
        
        Rect::new(x, y, width, height)
    }

    pub fn handle_event(&mut self, event: &Event) -> Option<Action> {
        if !self.visible {
            return None;
        }

        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => None,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.hide();
                Some(Action::ContextMenuClose)
            }
            KeyCode::Enter => self.confirm_selection(),
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
                None
            }
            _ => None,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is inside menu
                let menu_rect = self.calculate_menu_rect(Rect::new(0, 0, 200, 50)); // Approximate
                if menu_rect.contains((mouse.column, mouse.row).into()) {
                    let relative_y = mouse.row.saturating_sub(menu_rect.y + 1) as usize;
                    if relative_y < self.items.len() {
                        self.selected = relative_y;
                        return self.confirm_selection();
                    }
                } else {
                    // Click outside menu - close it
                    self.hide();
                    return Some(Action::ContextMenuClose);
                }
                None
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // Right-click outside should close menu
                self.hide();
                Some(Action::ContextMenuClose)
            }
            MouseEventKind::ScrollUp => {
                self.select_prev();
                None
            }
            MouseEventKind::ScrollDown => {
                self.select_next();
                None
            }
            _ => None,
        }
    }

    pub fn render(&self, frame: &mut Frame, screen: Rect, theme: &Theme) {
        if !self.visible || self.items.is_empty() {
            return;
        }

        let menu_rect = self.calculate_menu_rect(screen);
        
        // Clear background
        frame.render_widget(Clear, menu_rect);

        // Build menu items
        let items: Vec<ListItem> = self.items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                if item.is_separator {
                    // Separator line
                    let separator = "─".repeat((menu_rect.width.saturating_sub(2)) as usize);
                    ListItem::new(Line::from(Span::styled(
                        separator,
                        Style::default().fg(theme.colors.muted.to_color()),
                    )))
                } else {
                    let is_selected = i == self.selected;
                    
                    let (fg, bg) = if is_selected && item.enabled {
                        (
                            theme.menu.selected_fg.to_color(),
                            theme.menu.selected_bg.to_color(),
                        )
                    } else if item.enabled {
                        (
                            theme.menu.item_fg.to_color(),
                            theme.menu.item_bg.to_color(),
                        )
                    } else {
                        (
                            theme.menu.disabled_fg.to_color(),
                            theme.menu.item_bg.to_color(),
                        )
                    };

                    let mut spans = vec![Span::styled(
                        format!(" {} ", item.label),
                        Style::default().fg(fg).bg(bg),
                    )];

                    // Add shortcut hint
                    if let Some(ref shortcut) = item.shortcut {
                        // Calculate padding
                        let used_width = item.label.len() + 2;
                        let shortcut_width = shortcut.len() + 1;
                        let available = menu_rect.width.saturating_sub(4) as usize;
                        let padding = available.saturating_sub(used_width + shortcut_width);
                        
                        spans.push(Span::styled(
                            " ".repeat(padding),
                            Style::default().bg(bg),
                        ));
                        spans.push(Span::styled(
                            shortcut.clone(),
                            Style::default()
                                .fg(theme.menu.shortcut_fg.to_color())
                                .bg(bg),
                        ));
                    }

                    ListItem::new(Line::from(spans))
                }
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.focus.focused_border.to_color()))
            .style(Style::default().bg(theme.colors.background.to_color()));

        let list = List::new(items).block(block);

        frame.render_widget(list, menu_rect);
    }
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_menu_new() {
        let menu = ContextMenu::new();
        assert!(!menu.is_visible());
        assert!(matches!(menu.target(), ContextMenuTarget::Generic));
    }

    #[test]
    fn test_context_menu_show_hide() {
        let mut menu = ContextMenu::new();
        let items = vec![
            ContextMenuItem::new("Test", Action::None),
        ];
        
        menu.show(10, 20, ContextMenuTarget::Tab(0), items);
        assert!(menu.is_visible());
        assert_eq!(menu.position(), (10, 20));
        assert!(matches!(menu.target(), ContextMenuTarget::Tab(0)));
        
        menu.hide();
        assert!(!menu.is_visible());
    }

    #[test]
    fn test_context_menu_item_creation() {
        let item = ContextMenuItem::new("Copy", Action::Copy)
            .with_shortcut("Ctrl+C");
        
        assert_eq!(item.label, "Copy");
        assert_eq!(item.shortcut, Some("Ctrl+C".to_string()));
        assert!(item.enabled);
        assert!(!item.is_separator);
    }

    #[test]
    fn test_context_menu_item_disabled() {
        let item = ContextMenuItem::new("Paste", Action::Paste).disabled();
        assert!(!item.enabled);
    }

    #[test]
    fn test_context_menu_separator() {
        let sep = ContextMenuItem::separator();
        assert!(sep.is_separator);
        assert!(!sep.enabled);
    }

    #[test]
    fn test_context_menu_navigation() {
        let mut menu = ContextMenu::new();
        let items = vec![
            ContextMenuItem::new("Item 1", Action::None),
            ContextMenuItem::new("Item 2", Action::None),
            ContextMenuItem::new("Item 3", Action::None),
        ];
        
        menu.show(0, 0, ContextMenuTarget::Generic, items);
        assert_eq!(menu.selected, 0);
        
        menu.select_next();
        assert_eq!(menu.selected, 1);
        
        menu.select_next();
        assert_eq!(menu.selected, 2);
        
        menu.select_next();
        assert_eq!(menu.selected, 0); // Wrap around
        
        menu.select_prev();
        assert_eq!(menu.selected, 2); // Wrap around backwards
    }

    #[test]
    fn test_context_menu_skip_separators() {
        let mut menu = ContextMenu::new();
        let items = vec![
            ContextMenuItem::new("Item 1", Action::None),
            ContextMenuItem::separator(),
            ContextMenuItem::new("Item 2", Action::None),
        ];
        
        menu.show(0, 0, ContextMenuTarget::Generic, items);
        assert_eq!(menu.selected, 0);
        
        menu.select_next();
        assert_eq!(menu.selected, 2); // Skipped separator at index 1
    }

    #[test]
    fn test_context_menu_skip_disabled() {
        let mut menu = ContextMenu::new();
        let items = vec![
            ContextMenuItem::new("Enabled 1", Action::None),
            ContextMenuItem::new("Disabled", Action::None).disabled(),
            ContextMenuItem::new("Enabled 2", Action::None),
        ];
        
        menu.show(0, 0, ContextMenuTarget::Generic, items);
        assert_eq!(menu.selected, 0);
        
        menu.select_next();
        assert_eq!(menu.selected, 2); // Skipped disabled at index 1
    }

    #[test]
    fn test_context_menu_confirm_selection() {
        let mut menu = ContextMenu::new();
        let items = vec![
            ContextMenuItem::new("Copy", Action::Copy),
        ];
        
        menu.show(0, 0, ContextMenuTarget::Generic, items);
        
        let action = menu.confirm_selection();
        assert!(matches!(action, Some(Action::Copy)));
        assert!(!menu.is_visible()); // Menu should close after confirm
    }

    #[test]
    fn test_calculate_menu_rect_no_overflow() {
        let menu = ContextMenu {
            visible: true,
            position: (10, 10),
            items: vec![
                ContextMenuItem::new("Short", Action::None),
            ],
            selected: 0,
            target: ContextMenuTarget::Generic,
            max_width: 30,
        };
        
        let screen = Rect::new(0, 0, 100, 50);
        let rect = menu.calculate_menu_rect(screen);
        
        assert!(rect.x + rect.width <= screen.width);
        assert!(rect.y + rect.height <= screen.height);
    }

    #[test]
    fn test_calculate_menu_rect_right_edge() {
        let menu = ContextMenu {
            visible: true,
            position: (95, 10),
            items: vec![
                ContextMenuItem::new("A longer menu item", Action::None),
            ],
            selected: 0,
            target: ContextMenuTarget::Generic,
            max_width: 30,
        };
        
        let screen = Rect::new(0, 0, 100, 50);
        let rect = menu.calculate_menu_rect(screen);
        
        // Should be pushed left to fit
        assert!(rect.x + rect.width <= screen.width);
    }

    #[test]
    fn test_calculate_menu_rect_bottom_edge() {
        let menu = ContextMenu {
            visible: true,
            position: (10, 48),
            items: vec![
                ContextMenuItem::new("Item 1", Action::None),
                ContextMenuItem::new("Item 2", Action::None),
                ContextMenuItem::new("Item 3", Action::None),
            ],
            selected: 0,
            target: ContextMenuTarget::Generic,
            max_width: 30,
        };
        
        let screen = Rect::new(0, 0, 100, 50);
        let rect = menu.calculate_menu_rect(screen);
        
        // Should be pushed up to fit
        assert!(rect.y + rect.height <= screen.height);
    }
}
