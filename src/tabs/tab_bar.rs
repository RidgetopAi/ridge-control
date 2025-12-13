// Tab bar - some types for future theming options
#![allow(dead_code)]

//! Tab Bar Widget
//!
//! Renders the tab bar at the top of the screen with:
//! - Tab names with index indicators
//! - Active tab highlighting
//! - Activity indicators for background tabs
//! - Nerd Font icons per CONTRACT.md Section 4.8

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};

use crate::config::Theme;
use super::{Tab, TabManager};

/// Visual style configuration for the tab bar
#[derive(Debug, Clone)]
pub struct TabBarStyle {
    /// Background color of the entire tab bar
    pub background: Color,
    /// Style for inactive tabs
    pub inactive: Style,
    /// Style for the active tab
    pub active: Style,
    /// Style for tabs with activity
    pub activity: Style,
    /// Separator between tabs
    pub separator: &'static str,
    /// Icon for main tab (Nerd Font)
    pub main_icon: &'static str,
    /// Icon for regular tabs (Nerd Font)
    pub tab_icon: &'static str,
    /// Activity indicator (Nerd Font)
    pub activity_icon: &'static str,
    /// Close button icon (Nerd Font)
    pub close_icon: &'static str,
}

impl Default for TabBarStyle {
    fn default() -> Self {
        Self {
            background: Color::Rgb(30, 30, 46), // Dark background
            inactive: Style::default()
                .fg(Color::Rgb(147, 153, 178)) // Muted text
                .bg(Color::Rgb(30, 30, 46)),
            active: Style::default()
                .fg(Color::Rgb(205, 214, 244)) // Bright text
                .bg(Color::Rgb(69, 71, 90)) // Slightly lighter bg
                .add_modifier(Modifier::BOLD),
            activity: Style::default()
                .fg(Color::Rgb(249, 226, 175)) // Yellow/gold for activity
                .add_modifier(Modifier::BOLD),
            separator: "│",
            main_icon: "󰍜 ", // Nerd Font: nf-md-view_dashboard
            tab_icon: "󰓩 ",  // Nerd Font: nf-md-tab
            activity_icon: "●",
            close_icon: "󰅖", // Nerd Font: nf-md-close
        }
    }
}

impl TabBarStyle {
    /// Vibrant color scheme alternative
    pub fn vibrant() -> Self {
        Self {
            background: Color::Rgb(20, 20, 30),
            inactive: Style::default()
                .fg(Color::Rgb(100, 100, 120))
                .bg(Color::Rgb(20, 20, 30)),
            active: Style::default()
                .fg(Color::Rgb(130, 170, 255)) // Bright blue
                .bg(Color::Rgb(40, 40, 60))
                .add_modifier(Modifier::BOLD),
            activity: Style::default()
                .fg(Color::Rgb(255, 180, 100)) // Orange
                .add_modifier(Modifier::BOLD),
            separator: "▏",
            main_icon: "󰍜 ",
            tab_icon: "󰓩 ",
            activity_icon: "◉",
            close_icon: "󰅖",
        }
    }

    /// Create TabBarStyle from Theme configuration
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            background: theme.colors.background.to_color(),
            inactive: Style::default()
                .fg(theme.colors.muted.to_color())
                .bg(theme.colors.background.to_color()),
            active: Style::default()
                .fg(theme.colors.foreground.to_color())
                .bg(theme.focus.focused_border.to_color())
                .add_modifier(Modifier::BOLD),
            activity: Style::default()
                .fg(theme.colors.warning.to_color())
                .add_modifier(Modifier::BOLD),
            separator: "│",
            main_icon: "󰍜 ",
            tab_icon: "󰓩 ",
            activity_icon: "●",
            close_icon: "󰅖",
        }
    }
}

/// Tab bar widget that renders all tabs
pub struct TabBar<'a> {
    tabs: &'a [Tab],
    active_index: usize,
    style: TabBarStyle,
    show_indices: bool,
    show_close_buttons: bool,
    /// TRC-018: Show dangerous mode warning indicator
    dangerous_mode: bool,
    /// TRC-029: Inline rename buffer (if renaming active tab)
    rename_buffer: Option<&'a str>,
}

impl<'a> TabBar<'a> {
    /// Create a new tab bar from a TabManager
    pub fn from_manager(manager: &'a TabManager) -> Self {
        Self {
            tabs: manager.tabs(),
            active_index: manager.active_index(),
            style: TabBarStyle::default(),
            show_indices: true,
            show_close_buttons: true,
            dangerous_mode: false,
            rename_buffer: manager.rename_buffer(),
        }
    }

    /// Create a new tab bar from a TabManager with theme-based styling
    pub fn from_manager_themed(manager: &'a TabManager, theme: &Theme) -> Self {
        Self {
            tabs: manager.tabs(),
            active_index: manager.active_index(),
            style: TabBarStyle::from_theme(theme),
            show_indices: true,
            show_close_buttons: true,
            dangerous_mode: false,
            rename_buffer: manager.rename_buffer(),
        }
    }

    /// Create from raw tab slice (for testing)
    pub fn new(tabs: &'a [Tab], active_index: usize) -> Self {
        Self {
            tabs,
            active_index,
            style: TabBarStyle::default(),
            show_indices: true,
            show_close_buttons: true,
            dangerous_mode: false,
            rename_buffer: None,
        }
    }
    
    /// Set dangerous mode indicator (TRC-018)
    pub fn dangerous_mode(mut self, enabled: bool) -> Self {
        self.dangerous_mode = enabled;
        self
    }

    /// Set custom style
    pub fn style(mut self, style: TabBarStyle) -> Self {
        self.style = style;
        self
    }

    /// Show/hide tab indices (Alt+N shortcuts)
    pub fn show_indices(mut self, show: bool) -> Self {
        self.show_indices = show;
        self
    }

    /// Show/hide close buttons
    pub fn show_close_buttons(mut self, show: bool) -> Self {
        self.show_close_buttons = show;
        self
    }

    /// Build spans for a single tab
    fn build_tab_spans(&self, tab: &Tab, index: usize, is_active: bool) -> Vec<Span<'a>> {
        let mut spans = Vec::new();

        // Determine base style
        let base_style = if is_active {
            self.style.active
        } else {
            self.style.inactive
        };

        // Opening padding
        spans.push(Span::styled(" ", base_style));

        // Icon
        let icon = if tab.is_main() {
            self.style.main_icon
        } else {
            self.style.tab_icon
        };
        spans.push(Span::styled(icon.to_string(), base_style));

        // Index indicator (for keyboard shortcuts Alt+1 through Alt+9)
        if self.show_indices && index < 9 {
            let idx_style = base_style.add_modifier(Modifier::DIM);
            spans.push(Span::styled(format!("{}:", index + 1), idx_style));
        }

        // TRC-029: Show rename input for active tab when renaming
        if is_active && self.rename_buffer.is_some() {
            let rename_text = self.rename_buffer.unwrap_or("");
            // Use a distinct style for the input field
            let input_style = Style::default()
                .fg(Color::Rgb(205, 214, 244))  // Bright text
                .bg(Color::Rgb(49, 50, 68))     // Slightly different bg
                .add_modifier(Modifier::UNDERLINED);
            
            // Show the rename buffer with cursor indicator
            spans.push(Span::styled(format!("{}_", rename_text), input_style));
        } else {
            // Tab name (normal display)
            spans.push(Span::styled(tab.name().to_string(), base_style));
        }

        // Activity indicator for inactive tabs
        if !is_active && tab.has_activity() {
            spans.push(Span::styled(
                format!(" {}", self.style.activity_icon),
                self.style.activity,
            ));
        }

        // Close button (not for main tab) - hide during rename
        if self.show_close_buttons && !tab.is_main() && !(is_active && self.rename_buffer.is_some()) {
            let close_style = if is_active {
                base_style.add_modifier(Modifier::DIM)
            } else {
                self.style.inactive.add_modifier(Modifier::DIM)
            };
            spans.push(Span::styled(format!(" {}", self.style.close_icon), close_style));
        }

        // Closing padding
        spans.push(Span::styled(" ", base_style));

        spans
    }

    /// Calculate the click position to tab index mapping
    /// Returns Vec of (start_x, end_x, tab_index) for hit testing
    pub fn calculate_hit_areas(&self, area: Rect) -> Vec<(u16, u16, usize)> {
        let mut hit_areas = Vec::new();
        let mut x = area.x + 1; // +1 for left border

        for (index, tab) in self.tabs.iter().enumerate() {
            // Calculate width of this tab
            let icon_width = 2;
            let index_width = if self.show_indices && index < 9 { 2 } else { 0 };
            let name_width = tab.name().chars().count();
            let activity_width = if tab.has_activity() && index != self.active_index { 2 } else { 0 };
            let close_width = if self.show_close_buttons && !tab.is_main() { 2 } else { 0 };
            let padding = 2; // 1 on each side

            let tab_width = (icon_width + index_width + name_width + activity_width + close_width + padding) as u16;
            let separator_width = 1;

            hit_areas.push((x, x + tab_width, index));

            x += tab_width + separator_width;
        }

        hit_areas
    }
}

impl Widget for TabBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Need at least 1 row for the tab bar
        if area.height < 1 {
            return;
        }

        // Fill background
        for x in area.x..area.x + area.width {
            buf[(x, area.y)]
                .set_char(' ')
                .set_bg(self.style.background);
        }

        // Build the line of spans for all tabs
        let mut spans: Vec<Span> = Vec::new();

        for (index, tab) in self.tabs.iter().enumerate() {
            // Add separator before tab (except first)
            if index > 0 {
                spans.push(Span::styled(
                    self.style.separator.to_string(),
                    Style::default()
                        .fg(Color::Rgb(69, 71, 90))
                        .bg(self.style.background),
                ));
            }

            // Add tab spans
            let is_active = index == self.active_index;
            spans.extend(self.build_tab_spans(tab, index, is_active));
        }

        // TRC-018: Add dangerous mode warning indicator on the right side
        if self.dangerous_mode {
            // Calculate used width for tabs
            let tabs_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            
            // Add spacing to push warning to the right
            let warning_text = " ⚠ DANGEROUS MODE ";
            let warning_width = warning_text.chars().count();
            let available = area.width as usize;
            
            if tabs_width + warning_width + 2 < available {
                let padding = available.saturating_sub(tabs_width + warning_width + 1);
                spans.push(Span::styled(
                    " ".repeat(padding),
                    Style::default().bg(self.style.background),
                ));
                spans.push(Span::styled(
                    warning_text.to_string(),
                    Style::default()
                        .fg(Color::Rgb(0, 0, 0)) // Black text
                        .bg(Color::Rgb(255, 100, 100)) // Red background
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }

        // Render the line
        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

/// Tab bar with border (as a Block wrapper)
pub struct TabBarBlock<'a> {
    tab_bar: TabBar<'a>,
    title: Option<&'a str>,
}

impl<'a> TabBarBlock<'a> {
    pub fn new(tab_bar: TabBar<'a>) -> Self {
        Self {
            tab_bar,
            title: None,
        }
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }
}

impl Widget for TabBarBlock<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::Rgb(69, 71, 90)));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height > 0 {
            self.tab_bar.render(inner, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_bar_style_default() {
        let style = TabBarStyle::default();
        assert_eq!(style.separator, "│");
        assert_eq!(style.main_icon, "󰍜 ");
    }

    #[test]
    fn test_tab_bar_from_manager() {
        let manager = TabManager::new();
        let tab_bar = TabBar::from_manager(&manager);

        assert_eq!(tab_bar.tabs.len(), 1);
        assert_eq!(tab_bar.active_index, 0);
    }

    #[test]
    fn test_hit_areas_calculation() {
        let mut manager = TabManager::new();
        manager.create_tab("Test");

        let tab_bar = TabBar::from_manager(&manager);
        let area = Rect::new(0, 0, 80, 1);

        let hit_areas = tab_bar.calculate_hit_areas(area);
        assert_eq!(hit_areas.len(), 2); // Main tab + Test tab
    }
}
