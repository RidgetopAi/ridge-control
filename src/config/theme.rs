use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    pub colors: ThemeColors,
    pub focus: FocusStyle,
    pub borders: BorderStyle,
    pub process_monitor: ProcessMonitorStyle,
    pub terminal: TerminalStyle,
    pub menu: MenuStyle,
    pub command_palette: CommandPaletteStyle,
    pub notifications: NotificationStyle,
    pub spinner: SpinnerThemeStyle,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            colors: ThemeColors::default(),
            focus: FocusStyle::default(),
            borders: BorderStyle::default(),
            process_monitor: ProcessMonitorStyle::default(),
            terminal: TerminalStyle::default(),
            menu: MenuStyle::default(),
            command_palette: CommandPaletteStyle::default(),
            notifications: NotificationStyle::default(),
            spinner: SpinnerThemeStyle::default(),
        }
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self::default()
    }
    
    pub fn vibrant() -> Self {
        Self {
            name: "vibrant".to_string(),
            colors: ThemeColors {
                background: HexColor::new("#0a0a0f"),
                foreground: HexColor::new("#e0e0e0"),
                primary: HexColor::new("#ff6b6b"),
                secondary: HexColor::new("#4ecdc4"),
                accent: HexColor::new("#ffe66d"),
                success: HexColor::new("#95e1a3"),
                warning: HexColor::new("#ffd93d"),
                error: HexColor::new("#ff6b6b"),
                muted: HexColor::new("#6c757d"),
            },
            focus: FocusStyle {
                focused_border: HexColor::new("#ff6b6b"),
                unfocused_border: HexColor::new("#3d3d4d"),
                focused_title: HexColor::new("#ffe66d"),
                unfocused_title: HexColor::new("#6c757d"),
                use_bold_focused: true,
                focus_indicator: "▶".to_string(),
            },
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeColors {
    pub background: HexColor,
    pub foreground: HexColor,
    pub primary: HexColor,
    pub secondary: HexColor,
    pub accent: HexColor,
    pub success: HexColor,
    pub warning: HexColor,
    pub error: HexColor,
    pub muted: HexColor,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            background: HexColor::new("#1a1b26"),
            foreground: HexColor::new("#c0caf5"),
            primary: HexColor::new("#7aa2f7"),
            secondary: HexColor::new("#9ece6a"),
            accent: HexColor::new("#bb9af7"),
            success: HexColor::new("#9ece6a"),
            warning: HexColor::new("#e0af68"),
            error: HexColor::new("#f7768e"),
            muted: HexColor::new("#565f89"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusStyle {
    pub focused_border: HexColor,
    pub unfocused_border: HexColor,
    pub focused_title: HexColor,
    pub unfocused_title: HexColor,
    pub use_bold_focused: bool,
    pub focus_indicator: String,
}

impl Default for FocusStyle {
    fn default() -> Self {
        Self {
            focused_border: HexColor::new("#7aa2f7"),
            unfocused_border: HexColor::new("#3b4261"),
            focused_title: HexColor::new("#bb9af7"),
            unfocused_title: HexColor::new("#565f89"),
            use_bold_focused: true,
            focus_indicator: "●".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BorderStyle {
    pub border_type: String,
    pub corner_chars: Option<CornerChars>,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self {
            border_type: "rounded".to_string(),
            corner_chars: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CornerChars {
    pub top_left: char,
    pub top_right: char,
    pub bottom_left: char,
    pub bottom_right: char,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessMonitorStyle {
    pub header_bg: HexColor,
    pub header_fg: HexColor,
    pub selected_bg: HexColor,
    pub selected_fg: HexColor,
    pub cpu_low: HexColor,
    pub cpu_medium: HexColor,
    pub cpu_high: HexColor,
    pub cpu_critical: HexColor,
    pub memory_color: HexColor,
    pub kill_button: HexColor,
    pub gpu_low: HexColor,
    pub gpu_medium: HexColor,
    pub gpu_high: HexColor,
    pub gpu_critical: HexColor,
    pub gpu_unavailable: HexColor,
}

impl Default for ProcessMonitorStyle {
    fn default() -> Self {
        Self {
            header_bg: HexColor::new("#24283b"),
            header_fg: HexColor::new("#7aa2f7"),
            selected_bg: HexColor::new("#364a82"),
            selected_fg: HexColor::new("#c0caf5"),
            cpu_low: HexColor::new("#9ece6a"),
            cpu_medium: HexColor::new("#e0af68"),
            cpu_high: HexColor::new("#ff9e64"),
            cpu_critical: HexColor::new("#f7768e"),
            memory_color: HexColor::new("#7dcfff"),
            kill_button: HexColor::new("#f7768e"),
            gpu_low: HexColor::new("#76b947"),
            gpu_medium: HexColor::new("#e0af68"),
            gpu_high: HexColor::new("#ff9e64"),
            gpu_critical: HexColor::new("#f7768e"),
            gpu_unavailable: HexColor::new("#565f89"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalStyle {
    pub cursor_color: HexColor,
    pub cursor_blink: bool,
    pub selection_bg: HexColor,
    pub selection_fg: HexColor,
    pub scrollbar_fg: HexColor,
    pub scrollbar_bg: HexColor,
}

impl Default for TerminalStyle {
    fn default() -> Self {
        Self {
            cursor_color: HexColor::new("#c0caf5"),
            cursor_blink: true,
            selection_bg: HexColor::new("#364a82"),
            selection_fg: HexColor::new("#c0caf5"),
            scrollbar_fg: HexColor::new("#565f89"),
            scrollbar_bg: HexColor::new("#1a1b26"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuStyle {
    pub item_fg: HexColor,
    pub item_bg: HexColor,
    pub selected_fg: HexColor,
    pub selected_bg: HexColor,
    pub disabled_fg: HexColor,
    pub shortcut_fg: HexColor,
    pub stream_connected: HexColor,
    pub stream_disconnected: HexColor,
    pub stream_connecting: HexColor,
    pub stream_error: HexColor,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            item_fg: HexColor::new("#c0caf5"),
            item_bg: HexColor::new("#1a1b26"),
            selected_fg: HexColor::new("#1a1b26"),
            selected_bg: HexColor::new("#7aa2f7"),
            disabled_fg: HexColor::new("#565f89"),
            shortcut_fg: HexColor::new("#9ece6a"),
            stream_connected: HexColor::new("#9ece6a"),
            stream_disconnected: HexColor::new("#565f89"),
            stream_connecting: HexColor::new("#e0af68"),
            stream_error: HexColor::new("#f7768e"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandPaletteStyle {
    pub background: HexColor,
    pub border: HexColor,
    pub input_fg: HexColor,
    pub input_bg: HexColor,
    pub item_fg: HexColor,
    pub item_bg: HexColor,
    pub selected_fg: HexColor,
    pub selected_bg: HexColor,
    pub match_highlight: HexColor,
    pub description_fg: HexColor,
}

impl Default for CommandPaletteStyle {
    fn default() -> Self {
        Self {
            background: HexColor::new("#1a1b26"),
            border: HexColor::new("#7aa2f7"),
            input_fg: HexColor::new("#c0caf5"),
            input_bg: HexColor::new("#24283b"),
            item_fg: HexColor::new("#c0caf5"),
            item_bg: HexColor::new("#1a1b26"),
            selected_fg: HexColor::new("#1a1b26"),
            selected_bg: HexColor::new("#7aa2f7"),
            match_highlight: HexColor::new("#bb9af7"),
            description_fg: HexColor::new("#565f89"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationStyle {
    pub info_fg: HexColor,
    pub info_bg: HexColor,
    pub success_fg: HexColor,
    pub success_bg: HexColor,
    pub warning_fg: HexColor,
    pub warning_bg: HexColor,
    pub error_fg: HexColor,
    pub error_bg: HexColor,
}

impl Default for NotificationStyle {
    fn default() -> Self {
        Self {
            info_fg: HexColor::new("#c0caf5"),
            info_bg: HexColor::new("#24283b"),
            success_fg: HexColor::new("#1a1b26"),
            success_bg: HexColor::new("#9ece6a"),
            warning_fg: HexColor::new("#1a1b26"),
            warning_bg: HexColor::new("#e0af68"),
            error_fg: HexColor::new("#c0caf5"),
            error_bg: HexColor::new("#f7768e"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpinnerThemeStyle {
    pub default_style: String,
    pub color: HexColor,
    pub loading_color: HexColor,
    pub success_color: HexColor,
    pub error_color: HexColor,
    pub progress_filled_color: HexColor,
    pub progress_empty_color: HexColor,
    pub progress_filled_char: char,
    pub progress_empty_char: char,
}

impl Default for SpinnerThemeStyle {
    fn default() -> Self {
        Self {
            default_style: "braille".to_string(),
            color: HexColor::new("#7dcfff"),
            loading_color: HexColor::new("#7aa2f7"),
            success_color: HexColor::new("#9ece6a"),
            error_color: HexColor::new("#f7768e"),
            progress_filled_color: HexColor::new("#7aa2f7"),
            progress_empty_color: HexColor::new("#3b4261"),
            progress_filled_char: '█',
            progress_empty_char: '░',
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HexColor(String);

impl HexColor {
    pub fn new(hex: &str) -> Self {
        Self(hex.to_string())
    }
    
    pub fn to_color(&self) -> Color {
        self.parse_hex().unwrap_or(Color::Reset)
    }
    
    fn parse_hex(&self) -> Option<Color> {
        let hex = self.0.trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        
        Some(Color::Rgb(r, g, b))
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for HexColor {
    fn default() -> Self {
        Self("#ffffff".to_string())
    }
}

impl Theme {
    pub fn border_style(&self, focused: bool) -> Style {
        let color = if focused {
            self.focus.focused_border.to_color()
        } else {
            self.focus.unfocused_border.to_color()
        };
        
        let mut style = Style::default().fg(color);
        if focused && self.focus.use_bold_focused {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }
    
    pub fn title_style(&self, focused: bool) -> Style {
        let color = if focused {
            self.focus.focused_title.to_color()
        } else {
            self.focus.unfocused_title.to_color()
        };
        
        let mut style = Style::default().fg(color);
        if focused && self.focus.use_bold_focused {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }
    
    pub fn selection_style(&self) -> Style {
        Style::default()
            .fg(self.terminal.selection_fg.to_color())
            .bg(self.terminal.selection_bg.to_color())
    }
    
    pub fn cpu_color(&self, percentage: f32, warn_threshold: f32, critical_threshold: f32) -> Color {
        if percentage >= critical_threshold {
            self.process_monitor.cpu_critical.to_color()
        } else if percentage >= warn_threshold {
            self.process_monitor.cpu_high.to_color()
        } else if percentage >= warn_threshold * 0.5 {
            self.process_monitor.cpu_medium.to_color()
        } else {
            self.process_monitor.cpu_low.to_color()
        }
    }

    pub fn gpu_color(&self, percentage: f32) -> Color {
        if percentage >= 90.0 {
            self.process_monitor.gpu_critical.to_color()
        } else if percentage >= 70.0 {
            self.process_monitor.gpu_high.to_color()
        } else if percentage >= 40.0 {
            self.process_monitor.gpu_medium.to_color()
        } else {
            self.process_monitor.gpu_low.to_color()
        }
    }

    pub fn gpu_unavailable_color(&self) -> Color {
        self.process_monitor.gpu_unavailable.to_color()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hex_color_parsing() {
        let color = HexColor::new("#ff0000");
        assert_eq!(color.to_color(), Color::Rgb(255, 0, 0));
        
        let color = HexColor::new("#00ff00");
        assert_eq!(color.to_color(), Color::Rgb(0, 255, 0));
        
        let color = HexColor::new("#0000ff");
        assert_eq!(color.to_color(), Color::Rgb(0, 0, 255));
    }
    
    #[test]
    fn test_theme_default() {
        let theme = Theme::default();
        assert_eq!(theme.name, "default");
        assert!(theme.focus.use_bold_focused);
    }
    
    #[test]
    fn test_theme_serialization() {
        let theme = Theme::default();
        let toml_str = toml::to_string_pretty(&theme).unwrap();
        let parsed: Theme = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.name, theme.name);
    }
    
    #[test]
    fn test_vibrant_theme() {
        let theme = Theme::vibrant();
        assert_eq!(theme.name, "vibrant");
    }
}
