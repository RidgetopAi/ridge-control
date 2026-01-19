//! SIRK Panel component for Forge configuration and control
//!
//! Provides UI for:
//! - Configuring Forge run parameters (run name, seed path, instance count, project)
//! - Displaying run status and progress
//! - Starting, stopping, and resuming Forge runs

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;

use super::types::ForgeConfig;

/// Run status for the SIRK panel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Idle,
    Running,
    Paused,
    Completed,
    Failed,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Idle => "Idle",
            RunStatus::Running => "Running",
            RunStatus::Paused => "Paused",
            RunStatus::Completed => "Completed",
            RunStatus::Failed => "Failed",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            RunStatus::Idle => "⭘",
            RunStatus::Running => "▶",
            RunStatus::Paused => "⏸",
            RunStatus::Completed => "✓",
            RunStatus::Failed => "✕",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            RunStatus::Idle => Color::Gray,
            RunStatus::Running => Color::Green,
            RunStatus::Paused => Color::Yellow,
            RunStatus::Completed => Color::Cyan,
            RunStatus::Failed => Color::Red,
        }
    }
}

/// Input field identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SirkField {
    RunName,
    SeedPath,
    InstanceCount,
    Project,
}

impl SirkField {
    pub const ALL: &'static [SirkField] = &[
        SirkField::RunName,
        SirkField::SeedPath,
        SirkField::InstanceCount,
        SirkField::Project,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            SirkField::RunName => "Run Name",
            SirkField::SeedPath => "Seed Document",
            SirkField::InstanceCount => "Instances",
            SirkField::Project => "Project",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            SirkField::RunName => "e.g., feature-auth-v2",
            SirkField::SeedPath => "Path to seed markdown file",
            SirkField::InstanceCount => "Number of parallel instances (1-100)",
            SirkField::Project => "Mandrel project name",
        }
    }
}

/// Input mode for the panel
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SirkInputMode {
    /// Normal navigation mode
    Normal,
    /// Editing a text field
    Editing { field: SirkField, buffer: String },
}

/// Button identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SirkButton {
    Start,
    Stop,
    Resume,
    Reset,
}

impl SirkButton {
    pub const ALL: &'static [SirkButton] = &[SirkButton::Start, SirkButton::Stop, SirkButton::Resume, SirkButton::Reset];

    pub fn label(&self) -> &'static str {
        match self {
            SirkButton::Start => "[Start]",
            SirkButton::Stop => "[Stop]",
            SirkButton::Resume => "[Resume]",
            SirkButton::Reset => "[Reset]",
        }
    }

    pub fn is_enabled(&self, status: RunStatus) -> bool {
        match (self, status) {
            (SirkButton::Start, RunStatus::Idle) => true,
            (SirkButton::Start, RunStatus::Completed) => true,
            (SirkButton::Start, RunStatus::Failed) => true,
            (SirkButton::Stop, RunStatus::Running) => true,
            (SirkButton::Resume, RunStatus::Paused) => true,
            (SirkButton::Reset, RunStatus::Completed) => true,
            (SirkButton::Reset, RunStatus::Failed) => true,
            _ => false,
        }
    }
}

/// SIRK Panel component
pub struct SirkPanel {
    /// Currently selected field (0-3 for fields, 4+ for buttons)
    selected_index: usize,
    /// Current input mode
    input_mode: SirkInputMode,
    /// Run name input value
    run_name: String,
    /// Seed document path
    seed_path: String,
    /// Number of instances
    instance_count: u32,
    /// Mandrel project name
    project: String,
    /// Current run status
    status: RunStatus,
    /// Current instance number (0 if not running)
    current_instance: u32,
    /// Total instances for current run
    total_instances: u32,
    /// Success count
    success_count: u32,
    /// Failure count
    fail_count: u32,
    /// Run start time (for duration tracking)
    run_start: Option<Instant>,
    /// Instance start time
    instance_start: Option<Instant>,
    /// Last error message
    last_error: Option<String>,
    /// Panel area for mouse handling
    panel_area: Rect,
}

impl SirkPanel {
    pub fn new() -> Self {
        Self {
            selected_index: 0,
            input_mode: SirkInputMode::Normal,
            run_name: String::new(),
            seed_path: String::new(),
            instance_count: 1,
            project: String::new(),
            status: RunStatus::Idle,
            current_instance: 0,
            total_instances: 0,
            success_count: 0,
            fail_count: 0,
            run_start: None,
            instance_start: None,
            last_error: None,
            panel_area: Rect::default(),
        }
    }

    pub fn status(&self) -> RunStatus {
        self.status
    }

    pub fn set_status(&mut self, status: RunStatus) {
        self.status = status;
        match status {
            RunStatus::Running => {
                if self.run_start.is_none() {
                    self.run_start = Some(Instant::now());
                }
            }
            RunStatus::Idle | RunStatus::Completed | RunStatus::Failed => {
                self.run_start = None;
                self.instance_start = None;
            }
            RunStatus::Paused => {}
        }
    }

    pub fn set_run_name(&mut self, name: String) {
        self.run_name = name;
    }

    pub fn set_seed_path(&mut self, path: String) {
        self.seed_path = path;
    }

    pub fn set_instance_count(&mut self, count: u32) {
        self.instance_count = count.clamp(1, 100);
    }

    pub fn set_project(&mut self, project: String) {
        self.project = project;
    }

    pub fn run_started(&mut self, total_instances: u32) {
        self.status = RunStatus::Running;
        self.current_instance = 0;
        self.total_instances = total_instances;
        self.success_count = 0;
        self.fail_count = 0;
        self.run_start = Some(Instant::now());
        self.last_error = None;
    }

    pub fn instance_started(&mut self, instance_number: u32) {
        self.current_instance = instance_number;
        self.instance_start = Some(Instant::now());
    }

    pub fn instance_completed(&mut self, success: bool) {
        if success {
            self.success_count += 1;
        } else {
            self.fail_count += 1;
        }
        self.instance_start = None;
    }

    pub fn run_completed(&mut self) {
        self.status = RunStatus::Completed;
        self.instance_start = None;
    }

    pub fn run_failed(&mut self, error: String) {
        self.status = RunStatus::Failed;
        self.last_error = Some(error);
        self.instance_start = None;
    }

    pub fn run_paused(&mut self) {
        self.status = RunStatus::Paused;
    }

    /// Reset the panel to idle state for a new run
    pub fn reset(&mut self) {
        self.status = RunStatus::Idle;
        self.current_instance = 0;
        self.total_instances = 0;
        self.success_count = 0;
        self.fail_count = 0;
        self.run_start = None;
        self.instance_start = None;
        self.last_error = None;
    }

    pub fn build_config(&self) -> ForgeConfig {
        ForgeConfig {
            run_name: self.run_name.clone(),
            total_instances: self.instance_count,
            project: self.project.clone(),
            seed_path: self.seed_path.clone(),
            ..Default::default()
        }
    }

    pub fn validate_config(&self) -> Result<(), String> {
        if self.run_name.trim().is_empty() {
            return Err("Run name is required".to_string());
        }
        if self.seed_path.trim().is_empty() {
            return Err("Seed document path is required".to_string());
        }
        if self.project.trim().is_empty() {
            return Err("Project name is required".to_string());
        }
        if self.instance_count == 0 || self.instance_count > 100 {
            return Err("Instance count must be between 1 and 100".to_string());
        }
        Ok(())
    }

    fn selected_field(&self) -> Option<SirkField> {
        if self.selected_index < SirkField::ALL.len() {
            Some(SirkField::ALL[self.selected_index])
        } else {
            None
        }
    }

    fn selected_button(&self) -> Option<SirkButton> {
        let button_start = SirkField::ALL.len();
        if self.selected_index >= button_start {
            let idx = self.selected_index - button_start;
            SirkButton::ALL.get(idx).copied()
        } else {
            None
        }
    }

    fn total_items(&self) -> usize {
        SirkField::ALL.len() + SirkButton::ALL.len()
    }

    fn move_selection(&mut self, delta: i32) {
        let total = self.total_items();
        let new_index = (self.selected_index as i32 + delta).rem_euclid(total as i32) as usize;
        self.selected_index = new_index;
    }

    fn get_field_value(&self, field: SirkField) -> String {
        match field {
            SirkField::RunName => self.run_name.clone(),
            SirkField::SeedPath => self.seed_path.clone(),
            SirkField::InstanceCount => self.instance_count.to_string(),
            SirkField::Project => self.project.clone(),
        }
    }

    fn set_field_value(&mut self, field: SirkField, value: String) {
        match field {
            SirkField::RunName => self.run_name = value,
            SirkField::SeedPath => self.seed_path = value,
            SirkField::InstanceCount => {
                if let Ok(n) = value.parse::<u32>() {
                    self.instance_count = n.clamp(1, 100);
                }
            }
            SirkField::Project => self.project = value,
        }
    }

    fn start_editing(&mut self, field: SirkField) {
        let value = self.get_field_value(field);
        self.input_mode = SirkInputMode::Editing {
            field,
            buffer: value,
        };
    }

    fn confirm_edit(&mut self) {
        if let SirkInputMode::Editing { field, buffer } = &self.input_mode {
            self.set_field_value(*field, buffer.clone());
        }
        self.input_mode = SirkInputMode::Normal;
    }

    fn cancel_edit(&mut self) {
        self.input_mode = SirkInputMode::Normal;
    }

    fn handle_key_normal(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                None
            }
            KeyCode::Enter => {
                if let Some(field) = self.selected_field() {
                    self.start_editing(field);
                    None
                } else if let Some(button) = self.selected_button() {
                    if button.is_enabled(self.status) {
                        match button {
                            SirkButton::Start => Some(Action::SirkStart),
                            SirkButton::Stop => Some(Action::SirkStop),
                            SirkButton::Resume => Some(Action::SirkResume),
                            SirkButton::Reset => Some(Action::SirkReset),
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::SirkPanelToggle),
            _ => None,
        }
    }

    fn handle_key_editing(&mut self, key: KeyEvent) -> Option<Action> {
        let SirkInputMode::Editing { field, buffer } = &mut self.input_mode else {
            return None;
        };

        match key.code {
            KeyCode::Enter => {
                self.confirm_edit();
                Some(Action::Noop)
            }
            KeyCode::Esc => {
                self.cancel_edit();
                Some(Action::Noop)
            }
            KeyCode::Backspace => {
                buffer.pop();
                Some(Action::Noop)
            }
            KeyCode::Char(c) => {
                if *field == SirkField::InstanceCount {
                    if c.is_ascii_digit() && buffer.len() < 3 {
                        buffer.push(c);
                    }
                } else {
                    buffer.push(c);
                }
                Some(Action::Noop)
            }
            // Consume all other keys in editing mode to prevent fall-through
            _ => Some(Action::Noop),
        }
    }

    fn format_duration(duration: Duration) -> String {
        let secs = duration.as_secs();
        let mins = secs / 60;
        let secs = secs % 60;
        if mins > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}s", secs)
        }
    }

    fn render_field(&self, field: SirkField, _width: u16, theme: &Theme) -> Line<'_> {
        let is_selected = self.selected_field() == Some(field);
        let is_editing = matches!(&self.input_mode, SirkInputMode::Editing { field: f, .. } if *f == field);

        let label_style = if is_selected {
            Style::default()
                .fg(theme.colors.primary.to_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.colors.foreground.to_color())
        };

        let label = format!("{}: ", field.label());
        let value = if is_editing {
            if let SirkInputMode::Editing { buffer, .. } = &self.input_mode {
                format!("{}▌", buffer)
            } else {
                self.get_field_value(field)
            }
        } else {
            let v = self.get_field_value(field);
            if v.is_empty() {
                field.hint().to_string()
            } else {
                v
            }
        };

        let value_style = if is_editing {
            Style::default()
                .fg(theme.colors.accent.to_color())
                .add_modifier(Modifier::BOLD)
        } else if self.get_field_value(field).is_empty() {
            Style::default().fg(theme.colors.muted.to_color())
        } else {
            Style::default().fg(theme.colors.secondary.to_color())
        };

        let indicator = if is_selected { "▸ " } else { "  " };

        Line::from(vec![
            Span::styled(indicator, label_style),
            Span::styled(label, label_style),
            Span::styled(value, value_style),
        ])
    }

    fn render_button(&self, button: SirkButton, theme: &Theme) -> Span<'_> {
        let is_selected = self.selected_button() == Some(button);
        let is_enabled = button.is_enabled(self.status);

        let style = if !is_enabled {
            Style::default().fg(theme.colors.muted.to_color())
        } else if is_selected {
            Style::default()
                .fg(theme.colors.background.to_color())
                .bg(theme.colors.primary.to_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.colors.foreground.to_color())
        };

        Span::styled(button.label(), style)
    }

    fn render_status_bar(&self, theme: &Theme) -> Line<'_> {
        let status_color = self.status.color();
        let status_icon = self.status.icon();
        let status_text = self.status.as_str();

        let mut spans = vec![
            Span::styled(
                format!("{} {} ", status_icon, status_text),
                Style::default().fg(status_color).add_modifier(Modifier::BOLD),
            ),
        ];

        if self.status == RunStatus::Running || self.status == RunStatus::Paused {
            spans.push(Span::styled(
                format!("[{}/{}] ", self.current_instance, self.total_instances),
                Style::default().fg(theme.colors.secondary.to_color()),
            ));

            if self.success_count > 0 || self.fail_count > 0 {
                spans.push(Span::styled(
                    format!("✓{} ✕{} ", self.success_count, self.fail_count),
                    Style::default().fg(theme.colors.muted.to_color()),
                ));
            }

            if let Some(start) = self.run_start {
                let elapsed = start.elapsed();
                spans.push(Span::styled(
                    format!("⏱ {}", Self::format_duration(elapsed)),
                    Style::default().fg(theme.colors.muted.to_color()),
                ));
            }
        }

        if let Some(ref error) = self.last_error {
            let truncated = if error.len() > 40 {
                format!("{}...", &error[..37])
            } else {
                error.clone()
            };
            spans.push(Span::styled(
                format!(" ⚠ {}", truncated),
                Style::default().fg(theme.colors.error.to_color()),
            ));
        }

        Line::from(spans)
    }
}

impl Default for SirkPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for SirkPanel {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => match &self.input_mode {
                SirkInputMode::Normal => self.handle_key_normal(*key),
                SirkInputMode::Editing { .. } => self.handle_key_editing(*key),
            },
            Event::Mouse(mouse) => self.handle_mouse(*mouse),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        if matches!(action, Action::SirkReset) {
            self.reset();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, _focused: bool, theme: &Theme) {
        // Note: panel_area would need Cell/RefCell if mouse tracking needed during render
        // For now, mouse handling uses positions relative to the original area

        let block = Block::default()
            .title(" SIRK Panel ")
            .borders(Borders::ALL)
            .border_style(theme.border_style(true));

        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // Status bar
                Constraint::Length(1), // Separator
                Constraint::Length(1), // Run Name
                Constraint::Length(1), // Seed Path
                Constraint::Length(1), // Instance Count
                Constraint::Length(1), // Project
                Constraint::Length(1), // Separator
                Constraint::Length(1), // Buttons
                Constraint::Min(0),    // Remaining
            ])
            .split(inner);

        // Status bar
        let status_line = self.render_status_bar(theme);
        f.render_widget(Paragraph::new(status_line), chunks[0]);

        // Separator
        f.render_widget(
            Paragraph::new("─".repeat(inner.width as usize))
                .style(Style::default().fg(theme.colors.muted.to_color())),
            chunks[1],
        );

        // Fields
        for (i, field) in SirkField::ALL.iter().enumerate() {
            let line = self.render_field(*field, inner.width, theme);
            f.render_widget(Paragraph::new(line), chunks[2 + i]);
        }

        // Separator
        f.render_widget(
            Paragraph::new("─".repeat(inner.width as usize))
                .style(Style::default().fg(theme.colors.muted.to_color())),
            chunks[6],
        );

        // Buttons
        let buttons_line = Line::from(vec![
            Span::raw("  "),
            self.render_button(SirkButton::Start, theme),
            Span::raw("  "),
            self.render_button(SirkButton::Stop, theme),
            Span::raw("  "),
            self.render_button(SirkButton::Resume, theme),
            Span::raw("  "),
            self.render_button(SirkButton::Reset, theme),
        ]);
        f.render_widget(Paragraph::new(buttons_line), chunks[7]);

        // Hint line
        if inner.height > 9 {
            let hint = if matches!(self.input_mode, SirkInputMode::Editing { .. }) {
                "↵ confirm  Esc cancel"
            } else {
                "j/k navigate  ↵ edit/activate  q close"
            };
            f.render_widget(
                Paragraph::new(hint).style(Style::default().fg(theme.colors.muted.to_color())),
                chunks[8],
            );
        }
    }
}

impl SirkPanel {
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::Down(_) => {
                let y = mouse.row;
                let inner_y = self.panel_area.y + 2; // Account for border and margin

                // Check if click is in field area (rows 2-5 relative to inner)
                let field_start_y = inner_y + 2; // After status + separator
                let field_end_y = field_start_y + SirkField::ALL.len() as u16;

                if y >= field_start_y && y < field_end_y {
                    let field_idx = (y - field_start_y) as usize;
                    if field_idx < SirkField::ALL.len() {
                        self.selected_index = field_idx;
                        return None;
                    }
                }

                // Check if click is in button area (row 7)
                let button_y = field_end_y + 1; // After separator
                if y == button_y {
                    let x = mouse.column;
                    let inner_x = self.panel_area.x + 2;
                    
                    // Rough button positions: Start at col 2, Stop at col 12, Resume at col 20
                    if x >= inner_x + 2 && x < inner_x + 10 {
                        self.selected_index = SirkField::ALL.len(); // Start button
                    } else if x >= inner_x + 12 && x < inner_x + 18 {
                        self.selected_index = SirkField::ALL.len() + 1; // Stop button
                    } else if x >= inner_x + 20 && x < inner_x + 30 {
                        self.selected_index = SirkField::ALL.len() + 2; // Resume button
                    }
                }
                None
            }
            MouseEventKind::ScrollUp => {
                self.move_selection(-1);
                None
            }
            MouseEventKind::ScrollDown => {
                self.move_selection(1);
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sirk_panel_new() {
        let panel = SirkPanel::new();
        assert_eq!(panel.status, RunStatus::Idle);
        assert_eq!(panel.selected_index, 0);
        assert!(panel.run_name.is_empty());
    }

    #[test]
    fn test_run_status_display() {
        assert_eq!(RunStatus::Idle.as_str(), "Idle");
        assert_eq!(RunStatus::Running.icon(), "▶");
        assert_eq!(RunStatus::Failed.color(), Color::Red);
    }

    #[test]
    fn test_field_navigation() {
        let mut panel = SirkPanel::new();
        assert_eq!(panel.selected_index, 0);
        
        panel.move_selection(1);
        assert_eq!(panel.selected_index, 1);
        
        panel.move_selection(-1);
        assert_eq!(panel.selected_index, 0);
        
        // Wrap around
        panel.move_selection(-1);
        assert_eq!(panel.selected_index, panel.total_items() - 1);
    }

    #[test]
    fn test_build_config() {
        let mut panel = SirkPanel::new();
        panel.set_run_name("test-run".to_string());
        panel.set_seed_path("/path/to/seed.md".to_string());
        panel.set_instance_count(5);
        panel.set_project("myproject".to_string());

        let config = panel.build_config();
        assert_eq!(config.run_name, "test-run");
        assert_eq!(config.seed_path, "/path/to/seed.md");
        assert_eq!(config.total_instances, 5);
        assert_eq!(config.project, "myproject");
    }

    #[test]
    fn test_validate_config() {
        let mut panel = SirkPanel::new();
        
        // All empty - should fail
        assert!(panel.validate_config().is_err());
        
        panel.set_run_name("test".to_string());
        assert!(panel.validate_config().is_err());
        
        panel.set_seed_path("/path".to_string());
        assert!(panel.validate_config().is_err());
        
        panel.set_project("proj".to_string());
        assert!(panel.validate_config().is_ok());
    }

    #[test]
    fn test_button_enabled_states() {
        assert!(SirkButton::Start.is_enabled(RunStatus::Idle));
        assert!(SirkButton::Start.is_enabled(RunStatus::Completed));
        assert!(SirkButton::Start.is_enabled(RunStatus::Failed));
        assert!(!SirkButton::Start.is_enabled(RunStatus::Running));
        
        assert!(SirkButton::Stop.is_enabled(RunStatus::Running));
        assert!(!SirkButton::Stop.is_enabled(RunStatus::Idle));
        
        assert!(SirkButton::Resume.is_enabled(RunStatus::Paused));
        assert!(!SirkButton::Resume.is_enabled(RunStatus::Running));
    }

    #[test]
    fn test_run_lifecycle() {
        let mut panel = SirkPanel::new();
        
        panel.run_started(10);
        assert_eq!(panel.status, RunStatus::Running);
        assert_eq!(panel.total_instances, 10);
        assert!(panel.run_start.is_some());
        
        panel.instance_started(1);
        assert_eq!(panel.current_instance, 1);
        
        panel.instance_completed(true);
        assert_eq!(panel.success_count, 1);
        
        panel.instance_completed(false);
        assert_eq!(panel.fail_count, 1);
        
        panel.run_completed();
        assert_eq!(panel.status, RunStatus::Completed);
    }

    #[test]
    fn test_editing_mode() {
        let mut panel = SirkPanel::new();
        panel.set_run_name("initial".to_string());
        
        panel.start_editing(SirkField::RunName);
        match &panel.input_mode {
            SirkInputMode::Editing { field, buffer } => {
                assert_eq!(*field, SirkField::RunName);
                assert_eq!(buffer, "initial");
            }
            _ => panic!("Should be in editing mode"),
        }
        
        // Type a character
        panel.handle_key_editing(KeyEvent::from(KeyCode::Char('X')));
        match &panel.input_mode {
            SirkInputMode::Editing { buffer, .. } => {
                assert_eq!(buffer, "initialX");
            }
            _ => panic!("Should still be in editing mode"),
        }
        
        panel.confirm_edit();
        assert!(matches!(panel.input_mode, SirkInputMode::Normal));
        assert_eq!(panel.run_name, "initialX");
    }

    #[test]
    fn test_instance_count_validation() {
        let mut panel = SirkPanel::new();
        
        panel.set_instance_count(0);
        assert_eq!(panel.instance_count, 1);
        
        panel.set_instance_count(200);
        assert_eq!(panel.instance_count, 100);
        
        panel.set_instance_count(50);
        assert_eq!(panel.instance_count, 50);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(SirkPanel::format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(SirkPanel::format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(SirkPanel::format_duration(Duration::from_secs(3600)), "60m 0s");
    }
}
