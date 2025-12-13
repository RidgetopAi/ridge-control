// TRC-015: Spinner animations - some methods for future use
#![allow(dead_code)]

use ratatui::style::Color;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpinnerStyle {
    #[default]
    Braille,
    BrailleDots,
    Blocks,
    DigitalDots,
    Line,
    Arrow,
    Bounce,
    Pulse,
    Moon,
    Clock,
}

impl SpinnerStyle {
    pub fn frames(&self) -> &'static [&'static str] {
        match self {
            SpinnerStyle::Braille => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            SpinnerStyle::BrailleDots => &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"],
            SpinnerStyle::Blocks => &["▖", "▘", "▝", "▗"],
            SpinnerStyle::DigitalDots => &["⣀", "⣤", "⣶", "⣿", "⣶", "⣤"],
            SpinnerStyle::Line => &["⎯", "\\", "|", "/"],
            SpinnerStyle::Arrow => &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"],
            SpinnerStyle::Bounce => &["⠁", "⠂", "⠄", "⠂"],
            SpinnerStyle::Pulse => &["█", "▓", "▒", "░", "▒", "▓"],
            SpinnerStyle::Moon => &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"],
            SpinnerStyle::Clock => &["🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛"],
        }
    }

    pub fn frame_duration_ms(&self) -> u64 {
        match self {
            SpinnerStyle::Braille => 80,
            SpinnerStyle::BrailleDots => 100,
            SpinnerStyle::Blocks => 150,
            SpinnerStyle::DigitalDots => 100,
            SpinnerStyle::Line => 100,
            SpinnerStyle::Arrow => 100,
            SpinnerStyle::Bounce => 120,
            SpinnerStyle::Pulse => 120,
            SpinnerStyle::Moon => 150,
            SpinnerStyle::Clock => 100,
        }
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            SpinnerStyle::Braille => "braille",
            SpinnerStyle::BrailleDots => "braille_dots",
            SpinnerStyle::Blocks => "blocks",
            SpinnerStyle::DigitalDots => "digital_dots",
            SpinnerStyle::Line => "line",
            SpinnerStyle::Arrow => "arrow",
            SpinnerStyle::Bounce => "bounce",
            SpinnerStyle::Pulse => "pulse",
            SpinnerStyle::Moon => "moon",
            SpinnerStyle::Clock => "clock",
        }
    }
    
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "braille" => Some(SpinnerStyle::Braille),
            "braille_dots" => Some(SpinnerStyle::BrailleDots),
            "blocks" => Some(SpinnerStyle::Blocks),
            "digital_dots" => Some(SpinnerStyle::DigitalDots),
            "line" => Some(SpinnerStyle::Line),
            "arrow" => Some(SpinnerStyle::Arrow),
            "bounce" => Some(SpinnerStyle::Bounce),
            "pulse" => Some(SpinnerStyle::Pulse),
            "moon" => Some(SpinnerStyle::Moon),
            "clock" => Some(SpinnerStyle::Clock),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spinner {
    style: SpinnerStyle,
    frame_index: usize,
    last_frame_time: Instant,
    label: Option<String>,
    active: bool,
    color: Color,
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new(SpinnerStyle::default())
    }
}

impl Spinner {
    pub fn new(style: SpinnerStyle) -> Self {
        Self {
            style,
            frame_index: 0,
            last_frame_time: Instant::now(),
            label: None,
            active: true,
            color: Color::Cyan,
        }
    }
    
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    
    pub fn set_label(&mut self, label: Option<String>) {
        self.label = label;
    }
    
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }
    
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        if active {
            self.last_frame_time = Instant::now();
        }
    }
    
    pub fn is_active(&self) -> bool {
        self.active
    }
    
    pub fn tick(&mut self) -> bool {
        if !self.active {
            return false;
        }
        
        let frame_duration = Duration::from_millis(self.style.frame_duration_ms());
        if self.last_frame_time.elapsed() >= frame_duration {
            let frames = self.style.frames();
            self.frame_index = (self.frame_index + 1) % frames.len();
            self.last_frame_time = Instant::now();
            true
        } else {
            false
        }
    }
    
    pub fn current_frame(&self) -> &'static str {
        let frames = self.style.frames();
        frames[self.frame_index % frames.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_creation() {
        let spinner = Spinner::new(SpinnerStyle::Braille);
        assert_eq!(spinner.style, SpinnerStyle::Braille);
        assert!(spinner.is_active());
    }

    #[test]
    fn test_spinner_with_label() {
        let spinner = Spinner::new(SpinnerStyle::Braille)
            .with_label("Loading...");
        assert_eq!(spinner.label, Some("Loading...".to_string()));
    }

    #[test]
    fn test_spinner_tick() {
        let mut spinner = Spinner::new(SpinnerStyle::Braille);
        spinner.last_frame_time = Instant::now() - Duration::from_millis(100);
        let ticked = spinner.tick();
        assert!(ticked);
        assert_eq!(spinner.frame_index, 1);
    }

    #[test]
    fn test_spinner_inactive() {
        let mut spinner = Spinner::new(SpinnerStyle::Braille);
        spinner.set_active(false);
        assert!(!spinner.is_active());
        assert!(!spinner.tick());
    }

    #[test]
    fn test_spinner_style_frames() {
        for style in [
            SpinnerStyle::Braille,
            SpinnerStyle::BrailleDots,
            SpinnerStyle::Blocks,
            SpinnerStyle::DigitalDots,
            SpinnerStyle::Line,
            SpinnerStyle::Arrow,
            SpinnerStyle::Bounce,
            SpinnerStyle::Pulse,
        ] {
            let frames = style.frames();
            assert!(!frames.is_empty(), "Style {:?} has no frames", style);
        }
    }

    #[test]
    fn test_spinner_style_from_name() {
        assert_eq!(SpinnerStyle::from_name("braille"), Some(SpinnerStyle::Braille));
        assert_eq!(SpinnerStyle::from_name("blocks"), Some(SpinnerStyle::Blocks));
        assert_eq!(SpinnerStyle::from_name("invalid"), None);
    }
}
