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
use crate::config::{AppConfig, KeybindingsConfig, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSection {
    General,
    Terminal,
    ProcessMonitor,
    Keybindings,
    Theme,
    Providers,
}

impl ConfigSection {
    pub const ALL: &'static [ConfigSection] = &[
        ConfigSection::General,
        ConfigSection::Terminal,
        ConfigSection::ProcessMonitor,
        ConfigSection::Keybindings,
        ConfigSection::Theme,
        ConfigSection::Providers,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ConfigSection::General => "General",
            ConfigSection::Terminal => "Terminal",
            ConfigSection::ProcessMonitor => "Process Monitor",
            ConfigSection::Keybindings => "Keybindings",
            ConfigSection::Theme => "Theme",
            ConfigSection::Providers => "LLM Providers",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ConfigSection::General => "⚙",
            ConfigSection::Terminal => "󰆍",
            ConfigSection::ProcessMonitor => "󰍛",
            ConfigSection::Keybindings => "󰌌",
            ConfigSection::Theme => "󰏘",
            ConfigSection::Providers => "󰧑",
        }
    }
}

pub struct ConfigPanel {
    scroll_offset: u16,
    visible_height: u16,
    inner_area: Rect,
    selected_section: usize,
    expanded_sections: Vec<bool>,
    cached_lines: Vec<ConfigLine>,
}

#[derive(Clone)]
struct ConfigLine {
    is_header: bool,
    section_idx: Option<usize>,
    content: String,
    value: Option<String>,
}

impl ConfigPanel {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            visible_height: 10,
            inner_area: Rect::default(),
            selected_section: 0,
            expanded_sections: vec![true; ConfigSection::ALL.len()],
            cached_lines: Vec::new(),
        }
    }

    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = area;
        self.visible_height = area.height.saturating_sub(2);
    }

    pub fn refresh(&mut self, config: &AppConfig, keybindings: &KeybindingsConfig, theme: &Theme, providers: &[String]) {
        self.cached_lines.clear();

        for (idx, section) in ConfigSection::ALL.iter().enumerate() {
            self.cached_lines.push(ConfigLine {
                is_header: true,
                section_idx: Some(idx),
                content: format!("{} {}", section.icon(), section.as_str()),
                value: None,
            });

            if self.expanded_sections[idx] {
                match section {
                    ConfigSection::General => {
                        self.add_setting("Tick Interval", &format!("{}ms", config.general.tick_interval_ms));
                        self.add_setting("Log Level", &config.general.log_level);
                        self.add_setting("Log File", &config.general.log_file.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "None".to_string()));
                        self.add_setting("Watch Config", &config.general.watch_config.to_string());
                        self.add_setting("Config Watch Debounce", &format!("{}ms", config.general.config_watch_debounce_ms));
                    }
                    ConfigSection::Terminal => {
                        self.add_setting("Scrollback Lines", &config.terminal.scrollback_lines.to_string());
                        self.add_setting("Shell", config.terminal.shell.as_deref().unwrap_or("default"));
                        self.add_setting("Shell Args", &if config.terminal.shell_args.is_empty() { "none".to_string() } else { config.terminal.shell_args.join(" ") });
                        self.add_setting("TERM Env", &config.terminal.term_env);
                    }
                    ConfigSection::ProcessMonitor => {
                        self.add_setting("Refresh Interval", &format!("{}ms", config.process_monitor.refresh_interval_ms));
                        self.add_setting("Max Processes", &config.process_monitor.max_processes.to_string());
                        self.add_setting("Show Threads", &config.process_monitor.show_threads.to_string());
                        self.add_setting("CPU Warn Threshold", &format!("{}%", config.process_monitor.cpu_threshold_warn));
                        self.add_setting("CPU Critical Threshold", &format!("{}%", config.process_monitor.cpu_threshold_critical));
                    }
                    ConfigSection::Keybindings => {
                        self.add_setting("Normal mode bindings", &format!("{}", keybindings.normal.bindings.len()));
                        for (key_str, action) in keybindings.normal.bindings.iter().take(8) {
                            self.add_setting(&format!("  {}", key_str), &action.action);
                        }
                        if keybindings.normal.bindings.len() > 8 {
                            self.add_setting("  ...", &format!("({} more)", keybindings.normal.bindings.len() - 8));
                        }
                        self.add_setting("PTY raw mode bindings", &format!("{}", keybindings.pty_raw.bindings.len()));
                        self.add_setting("Command palette bindings", &format!("{}", keybindings.command_palette.bindings.len()));
                    }
                    ConfigSection::Theme => {
                        self.add_setting("Theme Name", &theme.name);
                        self.add_setting("Background", theme.colors.background.as_str());
                        self.add_setting("Foreground", theme.colors.foreground.as_str());
                        self.add_setting("Primary", theme.colors.primary.as_str());
                        self.add_setting("Secondary", theme.colors.secondary.as_str());
                        self.add_setting("Accent", theme.colors.accent.as_str());
                        self.add_setting("Focus Border", theme.focus.focused_border.as_str());
                        self.add_setting("Focus Indicator", &theme.focus.focus_indicator);
                    }
                    ConfigSection::Providers => {
                        if providers.is_empty() {
                            self.add_setting("Status", "No providers configured");
                        } else {
                            for provider in providers {
                                self.add_setting(provider, "✓ Configured");
                            }
                        }
                    }
                }
            }
        }
    }

    fn add_setting(&mut self, key: &str, value: &str) {
        self.cached_lines.push(ConfigLine {
            is_header: false,
            section_idx: None,
            content: key.to_string(),
            value: Some(value.to_string()),
        });
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: u16) {
        let max_scroll = self.cached_lines.len().saturating_sub(self.visible_height as usize) as u16;
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        let max_scroll = self.cached_lines.len().saturating_sub(self.visible_height as usize) as u16;
        self.scroll_offset = max_scroll;
    }

    pub fn scroll_page_up(&mut self) {
        self.scroll_up(self.visible_height.saturating_sub(2).max(1));
    }

    pub fn scroll_page_down(&mut self) {
        self.scroll_down(self.visible_height.saturating_sub(2).max(1));
    }

    pub fn toggle_section(&mut self) {
        if let Some(line) = self.cached_lines.get(self.line_at_selection()) {
            if let Some(idx) = line.section_idx {
                self.expanded_sections[idx] = !self.expanded_sections[idx];
            }
        }
    }

    pub fn next_section(&mut self) {
        self.selected_section = (self.selected_section + 1) % ConfigSection::ALL.len();
        self.scroll_to_section(self.selected_section);
    }

    pub fn prev_section(&mut self) {
        if self.selected_section == 0 {
            self.selected_section = ConfigSection::ALL.len() - 1;
        } else {
            self.selected_section -= 1;
        }
        self.scroll_to_section(self.selected_section);
    }

    fn line_at_selection(&self) -> usize {
        self.cached_lines
            .iter()
            .position(|l| l.section_idx == Some(self.selected_section))
            .unwrap_or(0)
    }

    fn scroll_to_section(&mut self, section_idx: usize) {
        if let Some(line_idx) = self.cached_lines
            .iter()
            .position(|l| l.section_idx == Some(section_idx))
        {
            if line_idx < self.scroll_offset as usize {
                self.scroll_offset = line_idx as u16;
            } else if line_idx >= (self.scroll_offset + self.visible_height) as usize {
                self.scroll_offset = (line_idx as u16).saturating_sub(self.visible_height / 2);
            }
        }
    }

    fn render_themed(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let expand_icon = if focused { "󰅀" } else { "" };
        let title = format!(
            " Config Settings {} [j/k=nav ↵=toggle] ",
            expand_icon
        );

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        if self.cached_lines.is_empty() {
            let msg = Paragraph::new(Line::from(Span::styled(
                "No configuration loaded",
                Style::default()
                    .fg(theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )))
            .block(block);
            frame.render_widget(msg, area);
            return;
        }

        let lines: Vec<Line> = self
            .cached_lines
            .iter()
            .enumerate()
            .skip(self.scroll_offset as usize)
            .take(self.visible_height as usize + 1)
            .map(|(_idx, line)| {
                if line.is_header {
                    let is_selected = line.section_idx == Some(self.selected_section);
                    let expanded = line.section_idx.map(|i| self.expanded_sections[i]).unwrap_or(false);
                    let arrow = if expanded { "▼" } else { "▶" };

                    let style = if is_selected && focused {
                        Style::default()
                            .fg(theme.colors.primary.to_color())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(theme.colors.secondary.to_color())
                            .add_modifier(Modifier::BOLD)
                    };

                    Line::from(vec![
                        Span::styled(format!("{} ", arrow), style),
                        Span::styled(&line.content, style),
                    ])
                } else {
                    let key_span = Span::styled(
                        format!("  {:24}", &line.content),
                        Style::default().fg(theme.colors.foreground.to_color()),
                    );

                    let value_span = Span::styled(
                        line.value.as_deref().unwrap_or(""),
                        Style::default().fg(theme.colors.accent.to_color()),
                    );

                    Line::from(vec![key_span, value_span])
                }
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.next_section();
                Some(Action::ConfigPanelNextSection)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.prev_section();
                Some(Action::ConfigPanelPrevSection)
            }
            KeyCode::Char('g') => {
                self.scroll_to_top();
                Some(Action::ConfigPanelScrollToTop)
            }
            KeyCode::Char('G') => {
                self.scroll_to_bottom();
                Some(Action::ConfigPanelScrollToBottom)
            }
            KeyCode::PageUp => {
                self.scroll_page_up();
                Some(Action::ConfigPanelScrollPageUp)
            }
            KeyCode::PageDown => {
                self.scroll_page_down();
                Some(Action::ConfigPanelScrollPageDown)
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.toggle_section();
                Some(Action::ConfigPanelToggleSection)
            }
            KeyCode::Char('r') => Some(Action::ConfigReload),
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::ConfigPanelHide),
            _ => None,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(3);
                Some(Action::ConfigPanelScrollUp(3))
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(3);
                Some(Action::ConfigPanelScrollDown(3))
            }
            _ => None,
        }
    }
}

impl Default for ConfigPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ConfigPanel {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.handle_key(*key),
            Event::Mouse(mouse) => self.handle_mouse(*mouse),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::ConfigPanelScrollUp(n) => self.scroll_up(*n),
            Action::ConfigPanelScrollDown(n) => self.scroll_down(*n),
            Action::ConfigPanelScrollToTop => self.scroll_to_top(),
            Action::ConfigPanelScrollToBottom => self.scroll_to_bottom(),
            Action::ConfigPanelScrollPageUp => self.scroll_page_up(),
            Action::ConfigPanelScrollPageDown => self.scroll_page_down(),
            Action::ConfigPanelNextSection => self.next_section(),
            Action::ConfigPanelPrevSection => self.prev_section(),
            Action::ConfigPanelToggleSection => self.toggle_section(),
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        self.render_themed(frame, area, focused, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_panel_new() {
        let panel = ConfigPanel::new();
        assert_eq!(panel.selected_section, 0);
        assert!(panel.expanded_sections.iter().all(|&e| e));
    }

    #[test]
    fn test_section_navigation() {
        let mut panel = ConfigPanel::new();
        assert_eq!(panel.selected_section, 0);
        
        panel.next_section();
        assert_eq!(panel.selected_section, 1);
        
        panel.prev_section();
        assert_eq!(panel.selected_section, 0);
        
        panel.prev_section();
        assert_eq!(panel.selected_section, ConfigSection::ALL.len() - 1);
    }

    #[test]
    fn test_section_toggle() {
        let mut panel = ConfigPanel::new();
        
        let config = AppConfig::default();
        let keybindings = KeybindingsConfig::default();
        let theme = Theme::default();
        panel.refresh(&config, &keybindings, &theme, &[]);
        
        assert!(panel.expanded_sections[0]);
        panel.toggle_section();
        assert!(!panel.expanded_sections[0]);
        panel.toggle_section();
        assert!(panel.expanded_sections[0]);
    }

    #[test]
    fn test_scroll_operations() {
        let mut panel = ConfigPanel::new();
        panel.visible_height = 10;
        
        let config = AppConfig::default();
        let keybindings = KeybindingsConfig::default();
        let theme = Theme::default();
        panel.refresh(&config, &keybindings, &theme, &["anthropic".to_string()]);
        
        assert_eq!(panel.scroll_offset, 0);
        panel.scroll_down(5);
        assert!(panel.scroll_offset > 0);
        
        let offset = panel.scroll_offset;
        panel.scroll_up(2);
        assert_eq!(panel.scroll_offset, offset - 2);
        
        panel.scroll_to_top();
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_config_section_display() {
        assert_eq!(ConfigSection::General.as_str(), "General");
        assert_eq!(ConfigSection::Terminal.as_str(), "Terminal");
        assert_eq!(ConfigSection::Theme.as_str(), "Theme");
    }

    #[test]
    fn test_refresh_populates_lines() {
        let mut panel = ConfigPanel::new();
        assert!(panel.cached_lines.is_empty());
        
        let config = AppConfig::default();
        let keybindings = KeybindingsConfig::default();
        let theme = Theme::default();
        panel.refresh(&config, &keybindings, &theme, &[]);
        
        assert!(!panel.cached_lines.is_empty());
        assert!(panel.cached_lines.iter().any(|l| l.is_header));
    }
}
