use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::time::{Duration, Instant};

use crate::config::Theme;

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
    
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    
    pub fn set_style(&mut self, style: SpinnerStyle) {
        self.style = style;
        self.frame_index = 0;
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
    
    pub fn render_inline(&self, theme: &Theme) -> Span<'static> {
        let color = if self.active {
            self.color
        } else {
            theme.colors.muted.to_color()
        };
        
        Span::styled(
            self.current_frame().to_string(),
            Style::default().fg(color),
        )
    }
    
    pub fn render_with_label(&self, theme: &Theme) -> Line<'static> {
        let spinner_span = self.render_inline(theme);
        
        if let Some(ref label) = self.label {
            Line::from(vec![
                spinner_span,
                Span::raw(" "),
                Span::styled(
                    label.clone(),
                    Style::default().fg(theme.colors.foreground.to_color()),
                ),
            ])
        } else {
            Line::from(vec![spinner_span])
        }
    }
    
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let line = self.render_with_label(theme);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
    }
    
    pub fn reset(&mut self) {
        self.frame_index = 0;
        self.last_frame_time = Instant::now();
    }
}

#[derive(Debug, Clone)]
pub struct ProgressBar {
    progress: f32,
    width: u16,
    label: Option<String>,
    show_percentage: bool,
    filled_char: char,
    empty_char: char,
    filled_color: Color,
    empty_color: Color,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            progress: 0.0,
            width: 20,
            label: None,
            show_percentage: true,
            filled_char: '█',
            empty_char: '░',
            filled_color: Color::Cyan,
            empty_color: Color::DarkGray,
        }
    }
}

impl ProgressBar {
    pub fn new(progress: f32) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            ..Default::default()
        }
    }
    
    pub fn with_width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }
    
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    
    pub fn with_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }
    
    pub fn with_chars(mut self, filled: char, empty: char) -> Self {
        self.filled_char = filled;
        self.empty_char = empty;
        self
    }
    
    pub fn with_colors(mut self, filled: Color, empty: Color) -> Self {
        self.filled_color = filled;
        self.empty_color = empty;
        self
    }
    
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }
    
    pub fn progress(&self) -> f32 {
        self.progress
    }
    
    pub fn increment(&mut self, amount: f32) {
        self.progress = (self.progress + amount).clamp(0.0, 1.0);
    }
    
    pub fn render_line(&self, theme: &Theme) -> Line<'static> {
        let filled_count = (self.progress * self.width as f32).round() as usize;
        let empty_count = self.width as usize - filled_count;
        
        let filled: String = std::iter::repeat(self.filled_char).take(filled_count).collect();
        let empty: String = std::iter::repeat(self.empty_char).take(empty_count).collect();
        
        let mut spans = vec![
            Span::styled(filled, Style::default().fg(self.filled_color)),
            Span::styled(empty, Style::default().fg(self.empty_color)),
        ];
        
        if self.show_percentage {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{:>3}%", (self.progress * 100.0).round() as u8),
                Style::default().fg(theme.colors.foreground.to_color()),
            ));
        }
        
        if let Some(ref label) = self.label {
            spans.insert(0, Span::styled(
                format!("{} ", label),
                Style::default().fg(theme.colors.foreground.to_color()),
            ));
        }
        
        Line::from(spans)
    }
    
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let line = self.render_line(theme);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
    }
}

#[derive(Debug, Clone)]
pub struct IndeterminateProgress {
    position: usize,
    width: u16,
    bar_width: u16,
    direction: i8,
    speed_ms: u64,
    last_update: Instant,
    color: Color,
    background_char: char,
    bar_char: char,
}

impl Default for IndeterminateProgress {
    fn default() -> Self {
        Self {
            position: 0,
            width: 20,
            bar_width: 4,
            direction: 1,
            speed_ms: 50,
            last_update: Instant::now(),
            color: Color::Cyan,
            background_char: '░',
            bar_char: '█',
        }
    }
}

impl IndeterminateProgress {
    pub fn new(width: u16) -> Self {
        Self {
            width,
            ..Default::default()
        }
    }
    
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    
    pub fn with_bar_width(mut self, width: u16) -> Self {
        self.bar_width = width;
        self
    }
    
    pub fn with_speed(mut self, speed_ms: u64) -> Self {
        self.speed_ms = speed_ms;
        self
    }
    
    pub fn tick(&mut self) -> bool {
        let frame_duration = Duration::from_millis(self.speed_ms);
        if self.last_update.elapsed() >= frame_duration {
            let max_pos = (self.width - self.bar_width) as usize;
            
            if self.direction > 0 {
                if self.position >= max_pos {
                    self.direction = -1;
                    self.position = max_pos.saturating_sub(1);
                } else {
                    self.position += 1;
                }
            } else {
                if self.position == 0 {
                    self.direction = 1;
                    self.position = 1;
                } else {
                    self.position -= 1;
                }
            }
            
            self.last_update = Instant::now();
            true
        } else {
            false
        }
    }
    
    pub fn render_line(&self, theme: &Theme) -> Line<'static> {
        let before: String = std::iter::repeat(self.background_char)
            .take(self.position)
            .collect();
        let bar: String = std::iter::repeat(self.bar_char)
            .take(self.bar_width as usize)
            .collect();
        let after_len = (self.width as usize).saturating_sub(self.position + self.bar_width as usize);
        let after: String = std::iter::repeat(self.background_char)
            .take(after_len)
            .collect();
        
        Line::from(vec![
            Span::styled(before, Style::default().fg(theme.colors.muted.to_color())),
            Span::styled(bar, Style::default().fg(self.color)),
            Span::styled(after, Style::default().fg(theme.colors.muted.to_color())),
        ])
    }
    
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let line = self.render_line(theme);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
    }
    
    pub fn reset(&mut self) {
        self.position = 0;
        self.direction = 1;
        self.last_update = Instant::now();
    }
}

#[derive(Debug, Clone)]
pub struct LoadingDots {
    dots: usize,
    max_dots: usize,
    speed_ms: u64,
    last_update: Instant,
    label: Option<String>,
    color: Color,
}

impl Default for LoadingDots {
    fn default() -> Self {
        Self {
            dots: 0,
            max_dots: 3,
            speed_ms: 400,
            last_update: Instant::now(),
            label: None,
            color: Color::Cyan,
        }
    }
}

impl LoadingDots {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    
    pub fn with_max_dots(mut self, max: usize) -> Self {
        self.max_dots = max;
        self
    }
    
    pub fn tick(&mut self) -> bool {
        let frame_duration = Duration::from_millis(self.speed_ms);
        if self.last_update.elapsed() >= frame_duration {
            self.dots = (self.dots + 1) % (self.max_dots + 1);
            self.last_update = Instant::now();
            true
        } else {
            false
        }
    }
    
    pub fn render_line(&self, theme: &Theme) -> Line<'static> {
        let dots_str: String = std::iter::repeat('.').take(self.dots).collect();
        let padding: String = std::iter::repeat(' ').take(self.max_dots - self.dots).collect();
        
        let mut spans = Vec::new();
        
        if let Some(ref label) = self.label {
            spans.push(Span::styled(
                label.clone(),
                Style::default().fg(theme.colors.foreground.to_color()),
            ));
        }
        
        spans.push(Span::styled(
            dots_str,
            Style::default().fg(self.color),
        ));
        spans.push(Span::raw(padding));
        
        Line::from(spans)
    }
    
    pub fn reset(&mut self) {
        self.dots = 0;
        self.last_update = Instant::now();
    }
}

#[derive(Debug, Clone)]
pub struct StatusIndicator {
    status: IndicatorStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorStatus {
    Success,
    Warning,
    Error,
    Info,
    Loading,
    Inactive,
}

impl StatusIndicator {
    pub fn new(status: IndicatorStatus) -> Self {
        Self { status }
    }
    
    pub fn success() -> Self {
        Self::new(IndicatorStatus::Success)
    }
    
    pub fn warning() -> Self {
        Self::new(IndicatorStatus::Warning)
    }
    
    pub fn error() -> Self {
        Self::new(IndicatorStatus::Error)
    }
    
    pub fn info() -> Self {
        Self::new(IndicatorStatus::Info)
    }
    
    pub fn loading() -> Self {
        Self::new(IndicatorStatus::Loading)
    }
    
    pub fn inactive() -> Self {
        Self::new(IndicatorStatus::Inactive)
    }
    
    pub fn set_status(&mut self, status: IndicatorStatus) {
        self.status = status;
    }
    
    pub fn icon(&self) -> &'static str {
        match self.status {
            IndicatorStatus::Success => "✓",
            IndicatorStatus::Warning => "⚠",
            IndicatorStatus::Error => "✗",
            IndicatorStatus::Info => "ℹ",
            IndicatorStatus::Loading => "◌",
            IndicatorStatus::Inactive => "○",
        }
    }
    
    pub fn color(&self, theme: &Theme) -> Color {
        match self.status {
            IndicatorStatus::Success => theme.colors.success.to_color(),
            IndicatorStatus::Warning => theme.colors.warning.to_color(),
            IndicatorStatus::Error => theme.colors.error.to_color(),
            IndicatorStatus::Info => theme.colors.primary.to_color(),
            IndicatorStatus::Loading => theme.colors.secondary.to_color(),
            IndicatorStatus::Inactive => theme.colors.muted.to_color(),
        }
    }
    
    pub fn render_span(&self, theme: &Theme) -> Span<'static> {
        Span::styled(
            self.icon().to_string(),
            Style::default().fg(self.color(theme)),
        )
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

    #[test]
    fn test_progress_bar_creation() {
        let bar = ProgressBar::new(0.5);
        assert_eq!(bar.progress(), 0.5);
    }

    #[test]
    fn test_progress_bar_clamp() {
        let bar = ProgressBar::new(1.5);
        assert_eq!(bar.progress(), 1.0);
        
        let bar2 = ProgressBar::new(-0.5);
        assert_eq!(bar2.progress(), 0.0);
    }

    #[test]
    fn test_progress_bar_increment() {
        let mut bar = ProgressBar::new(0.5);
        bar.increment(0.1);
        assert!((bar.progress() - 0.6).abs() < 0.001);
        
        bar.increment(0.5);
        assert_eq!(bar.progress(), 1.0);
    }

    #[test]
    fn test_indeterminate_progress_tick() {
        let mut progress = IndeterminateProgress::new(20);
        progress.last_update = Instant::now() - Duration::from_millis(100);
        let ticked = progress.tick();
        assert!(ticked);
        assert_eq!(progress.position, 1);
    }

    #[test]
    fn test_indeterminate_progress_bounce() {
        let mut progress = IndeterminateProgress::new(10)
            .with_bar_width(4);
        
        progress.position = 5;
        progress.direction = 1;
        progress.last_update = Instant::now() - Duration::from_millis(100);
        progress.tick();
        assert_eq!(progress.position, 6);
        
        progress.last_update = Instant::now() - Duration::from_millis(100);
        progress.tick();
        progress.direction = -1;
    }

    #[test]
    fn test_loading_dots() {
        let mut dots = LoadingDots::new()
            .with_max_dots(3)
            .with_label("Loading");
        
        dots.last_update = Instant::now() - Duration::from_millis(500);
        dots.tick();
        assert_eq!(dots.dots, 1);
    }

    #[test]
    fn test_status_indicator() {
        let theme = Theme::default();
        
        let success = StatusIndicator::success();
        assert_eq!(success.icon(), "✓");
        assert_eq!(success.color(&theme), theme.colors.success.to_color());
        
        let error = StatusIndicator::error();
        assert_eq!(error.icon(), "✗");
        assert_eq!(error.color(&theme), theme.colors.error.to_color());
    }

    #[test]
    fn test_spinner_reset() {
        let mut spinner = Spinner::new(SpinnerStyle::Braille);
        spinner.frame_index = 5;
        spinner.reset();
        assert_eq!(spinner.frame_index, 0);
    }
}
