//! Settings Editor component for LLM configuration
//!
//! Provides UI for editing:
//! - API Keys (per provider, masked input)
//! - Provider selection
//! - Model selection  
//! - Parameters (temperature, max_tokens)

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::{LLMConfig, Theme};

/// Section within the settings editor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    ApiKeys,
    Provider,
    Model,
    Parameters,
}

impl SettingsSection {
    pub const ALL: &'static [SettingsSection] = &[
        SettingsSection::ApiKeys,
        SettingsSection::Provider,
        SettingsSection::Model,
        SettingsSection::Parameters,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            SettingsSection::ApiKeys => "API Keys",
            SettingsSection::Provider => "Provider",
            SettingsSection::Model => "Model",
            SettingsSection::Parameters => "Parameters",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SettingsSection::ApiKeys => "󰌋",
            SettingsSection::Provider => "󰒍",
            SettingsSection::Model => "󰘦",
            SettingsSection::Parameters => "󰒓",
        }
    }
}

/// Input mode for the settings editor
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsInputMode {
    /// Normal navigation mode
    Normal,
    /// Editing a text field (e.g., API key input)
    Editing { field: String, buffer: String, masked: bool },
}

/// Provider with key status
#[derive(Debug, Clone)]
pub struct ProviderKeyStatus {
    pub name: String,
    pub has_key: bool,
}

/// Settings Editor component
pub struct SettingsEditor {
    /// Currently selected section
    selected_section: usize,
    /// Selected item within current section
    selected_item: usize,
    /// Current input mode
    input_mode: SettingsInputMode,
    /// Scroll offset for content
    scroll_offset: u16,
    /// Visible height (set during render)
    visible_height: u16,
    /// Inner area for mouse handling
    inner_area: Rect,
    /// Provider key statuses (populated externally)
    provider_keys: Vec<ProviderKeyStatus>,
    /// Available providers
    available_providers: Vec<String>,
    /// Available models for current provider
    available_models: Vec<String>,
    /// Current LLM config snapshot
    config: LLMConfig,
}

impl SettingsEditor {
    pub fn new() -> Self {
        Self {
            selected_section: 0,
            selected_item: 0,
            input_mode: SettingsInputMode::Normal,
            scroll_offset: 0,
            visible_height: 10,
            inner_area: Rect::default(),
            provider_keys: Vec::new(),
            available_providers: vec![
                "anthropic".to_string(),
                "openai".to_string(),
                "gemini".to_string(),
                "grok".to_string(),
                "groq".to_string(),
            ],
            available_models: Vec::new(),
            config: LLMConfig::default(),
        }
    }

    /// Set the LLM config to display/edit
    pub fn set_config(&mut self, config: LLMConfig) {
        self.config = config;
        self.refresh_models_for_provider();
    }

    /// Get current config
    pub fn config(&self) -> &LLMConfig {
        &self.config
    }

    /// Set provider key statuses
    pub fn set_provider_keys(&mut self, keys: Vec<ProviderKeyStatus>) {
        self.provider_keys = keys;
    }

    /// Set available models for current provider
    pub fn set_available_models(&mut self, models: Vec<String>) {
        self.available_models = models;
    }

    /// Get current section
    pub fn current_section(&self) -> SettingsSection {
        SettingsSection::ALL[self.selected_section]
    }

    /// Get current input mode
    pub fn input_mode(&self) -> &SettingsInputMode {
        &self.input_mode
    }

    /// Check if in editing mode
    pub fn is_editing(&self) -> bool {
        matches!(self.input_mode, SettingsInputMode::Editing { .. })
    }

    /// Get the current provider
    pub fn current_provider(&self) -> &str {
        &self.config.defaults.provider
    }

    /// Get the current model
    pub fn current_model(&self) -> &str {
        &self.config.defaults.model
    }

    fn refresh_models_for_provider(&mut self) {
        // Placeholder - will be populated by TS-009
        self.available_models = match self.config.defaults.provider.as_str() {
            "anthropic" => vec![
                "claude-sonnet-4-20250514".to_string(),
                "claude-3-5-sonnet-20241022".to_string(),
                "claude-3-opus-20240229".to_string(),
                "claude-3-haiku-20240307".to_string(),
            ],
            "openai" => vec![
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
                "gpt-4-turbo".to_string(),
            ],
            "gemini" => vec![
                "gemini-2.0-flash".to_string(),
                "gemini-1.5-pro".to_string(),
                "gemini-1.5-flash".to_string(),
            ],
            "grok" => vec!["grok-3".to_string(), "grok-2".to_string()],
            "groq" => vec![
                "llama-3.3-70b-versatile".to_string(),
                "llama-3.1-8b-instant".to_string(),
                "mixtral-8x7b-32768".to_string(),
            ],
            _ => Vec::new(),
        };
    }

    /// Navigate to next section
    pub fn next_section(&mut self) {
        self.selected_section = (self.selected_section + 1) % SettingsSection::ALL.len();
        self.selected_item = 0;
    }

    /// Navigate to previous section
    pub fn prev_section(&mut self) {
        if self.selected_section == 0 {
            self.selected_section = SettingsSection::ALL.len() - 1;
        } else {
            self.selected_section -= 1;
        }
        self.selected_item = 0;
    }

    /// Navigate to next item within section
    pub fn next_item(&mut self) {
        let max_items = self.items_in_current_section();
        if max_items > 0 {
            self.selected_item = (self.selected_item + 1) % max_items;
        }
    }

    /// Navigate to previous item within section
    pub fn prev_item(&mut self) {
        let max_items = self.items_in_current_section();
        if max_items > 0 {
            if self.selected_item == 0 {
                self.selected_item = max_items - 1;
            } else {
                self.selected_item -= 1;
            }
        }
    }

    fn items_in_current_section(&self) -> usize {
        match self.current_section() {
            SettingsSection::ApiKeys => self.available_providers.len(),
            SettingsSection::Provider => self.available_providers.len(),
            SettingsSection::Model => self.available_models.len(),
            SettingsSection::Parameters => 2, // temperature, max_tokens
        }
    }

    /// Start editing current field (for API key entry)
    pub fn start_editing(&mut self) {
        if self.current_section() == SettingsSection::ApiKeys {
            if let Some(provider) = self.available_providers.get(self.selected_item) {
                self.input_mode = SettingsInputMode::Editing {
                    field: provider.clone(),
                    buffer: String::new(),
                    masked: true,
                };
            }
        }
    }

    /// Cancel editing and return to normal mode
    pub fn cancel_editing(&mut self) {
        self.input_mode = SettingsInputMode::Normal;
    }

    /// Confirm current edit
    pub fn confirm_edit(&mut self) -> Option<Action> {
        if let SettingsInputMode::Editing { field, buffer, .. } = &self.input_mode {
            let action = if !buffer.is_empty() {
                // Return action to store key - TS-006 will wire this
                Some(Action::SettingsKeyEntered {
                    provider: field.clone(),
                    key: buffer.clone(),
                })
            } else {
                None
            };
            self.input_mode = SettingsInputMode::Normal;
            return action;
        }
        None
    }

    /// Handle character input during editing
    pub fn handle_edit_char(&mut self, c: char) {
        if let SettingsInputMode::Editing { buffer, .. } = &mut self.input_mode {
            buffer.push(c);
        }
    }

    /// Handle backspace during editing
    pub fn handle_edit_backspace(&mut self) {
        if let SettingsInputMode::Editing { buffer, .. } = &mut self.input_mode {
            buffer.pop();
        }
    }

    /// Select current provider
    pub fn select_provider(&mut self) -> Option<Action> {
        if self.current_section() == SettingsSection::Provider {
            if let Some(provider) = self.available_providers.get(self.selected_item).cloned() {
                if let Some(model) = self.config.default_model_for_provider(&provider) {
                    self.config.defaults.model = model.to_string();
                }
                self.config.defaults.provider = provider.clone();
                self.refresh_models_for_provider();
                return Some(Action::SettingsProviderChanged(provider));
            }
        }
        None
    }

    /// Select current model
    pub fn select_model(&mut self) -> Option<Action> {
        if self.current_section() == SettingsSection::Model {
            if let Some(model) = self.available_models.get(self.selected_item) {
                self.config.defaults.model = model.clone();
                return Some(Action::SettingsModelChanged(model.clone()));
            }
        }
        None
    }

    /// Scroll operations
    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    fn handle_key_normal(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Tab => {
                self.next_section();
                Some(Action::SettingsNextSection)
            }
            KeyCode::BackTab => {
                self.prev_section();
                Some(Action::SettingsPrevSection)
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.next_item();
                Some(Action::SettingsNextItem)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.prev_item();
                Some(Action::SettingsPrevItem)
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                match self.current_section() {
                    SettingsSection::ApiKeys => {
                        self.start_editing();
                        Some(Action::SettingsStartEdit)
                    }
                    SettingsSection::Provider => self.select_provider(),
                    SettingsSection::Model => self.select_model(),
                    SettingsSection::Parameters => {
                        // Parameters editing - TS-010 will implement
                        None
                    }
                }
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Test key - TS-007 will implement
                Some(Action::SettingsTestKey)
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::SettingsSave)
            }
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::SettingsClose),
            _ => None,
        }
    }

    fn handle_key_editing(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.cancel_editing();
                Some(Action::SettingsCancelEdit)
            }
            KeyCode::Enter => self.confirm_edit(),
            KeyCode::Backspace => {
                self.handle_edit_backspace();
                None
            }
            KeyCode::Char(c) => {
                self.handle_edit_char(c);
                None
            }
            _ => None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match &self.input_mode {
            SettingsInputMode::Normal => self.handle_key_normal(key),
            SettingsInputMode::Editing { .. } => self.handle_key_editing(key),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        if !self.inner_area.contains((mouse.column, mouse.row).into()) {
            return None;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(3);
                Some(Action::SettingsScrollUp(3))
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(3);
                Some(Action::SettingsScrollDown(3))
            }
            _ => None,
        }
    }

    fn render_section_header(&self, section: SettingsSection, selected: bool, theme: &Theme) -> Line<'static> {
        let style = if selected {
            Style::default()
                .fg(theme.colors.primary.to_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.colors.secondary.to_color())
        };

        Line::from(vec![
            Span::styled(format!("{} ", section.icon()), style),
            Span::styled(section.as_str().to_string(), style),
        ])
    }

    fn render_api_keys_section(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        
        for (idx, provider) in self.available_providers.iter().enumerate() {
            let is_selected = self.current_section() == SettingsSection::ApiKeys 
                && self.selected_item == idx;
            
            let has_key = self.provider_keys
                .iter()
                .find(|p| &p.name == provider)
                .map(|p| p.has_key)
                .unwrap_or(false);

            let status_icon = if has_key { "✓" } else { "✗" };
            let status_color = if has_key {
                theme.colors.success.to_color()
            } else {
                theme.colors.error.to_color()
            };

            let name_style = if is_selected {
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.colors.foreground.to_color())
            };

            // Show input field if editing this provider
            if let SettingsInputMode::Editing { field, buffer, masked } = &self.input_mode {
                if field == provider {
                    let display = if *masked {
                        "•".repeat(buffer.len())
                    } else {
                        buffer.clone()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:12} ", provider), name_style),
                        Span::styled("[", Style::default().fg(theme.colors.muted.to_color())),
                        Span::styled(display, Style::default().fg(theme.colors.accent.to_color())),
                        Span::styled("█", Style::default().fg(theme.colors.accent.to_color())),
                        Span::styled("]", Style::default().fg(theme.colors.muted.to_color())),
                    ]));
                    continue;
                }
            }

            lines.push(Line::from(vec![
                Span::styled(format!("  {:12} ", provider), name_style),
                Span::styled(status_icon.to_string(), Style::default().fg(status_color)),
                Span::styled(
                    if has_key { " configured" } else { " not set" }.to_string(),
                    Style::default().fg(theme.colors.muted.to_color()),
                ),
            ]));
        }

        lines
    }

    fn render_provider_section(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for (idx, provider) in self.available_providers.iter().enumerate() {
            let is_selected = self.current_section() == SettingsSection::Provider 
                && self.selected_item == idx;
            let is_current = provider == &self.config.defaults.provider;

            let marker = if is_current { "●" } else { "○" };
            let style = if is_selected {
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme.colors.primary.to_color())
            } else {
                Style::default().fg(theme.colors.foreground.to_color())
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", marker), style),
                Span::styled(provider.clone(), style),
            ]));
        }

        lines
    }

    fn render_model_section(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for (idx, model) in self.available_models.iter().enumerate() {
            let is_selected = self.current_section() == SettingsSection::Model 
                && self.selected_item == idx;
            let is_current = model == &self.config.defaults.model;

            let marker = if is_current { "●" } else { "○" };
            let style = if is_selected {
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme.colors.primary.to_color())
            } else {
                Style::default().fg(theme.colors.foreground.to_color())
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", marker), style),
                Span::styled(model.clone(), style),
            ]));
        }

        lines
    }

    fn render_parameters_section(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        let items = [
            ("Temperature", format!("{:.1}", self.config.parameters.temperature)),
            ("Max Tokens", format!("{}", self.config.parameters.max_tokens)),
        ];

        for (idx, (label, value)) in items.iter().enumerate() {
            let is_selected = self.current_section() == SettingsSection::Parameters 
                && self.selected_item == idx;

            let label_style = if is_selected {
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.colors.foreground.to_color())
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {:14} ", label), label_style),
                Span::styled(value.clone(), Style::default().fg(theme.colors.primary.to_color())),
            ]));
        }

        lines
    }

    fn render_themed(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let mode_indicator = match &self.input_mode {
            SettingsInputMode::Normal => "",
            SettingsInputMode::Editing { .. } => " [EDITING]",
        };

        let title = format!(" LLM Settings{} [Tab=section j/k=nav ↵=select] ", mode_indicator);

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);

        // Build all content lines
        let mut lines: Vec<Line> = Vec::new();

        for (idx, section) in SettingsSection::ALL.iter().enumerate() {
            let is_selected_section = idx == self.selected_section;

            // Section header
            lines.push(self.render_section_header(*section, is_selected_section, theme));

            // Section content (only if this section is selected for now - can expand all later)
            if is_selected_section {
                let section_lines = match section {
                    SettingsSection::ApiKeys => self.render_api_keys_section(theme),
                    SettingsSection::Provider => self.render_provider_section(theme),
                    SettingsSection::Model => self.render_model_section(theme),
                    SettingsSection::Parameters => self.render_parameters_section(theme),
                };
                lines.extend(section_lines);
            }

            // Add spacing between sections
            lines.push(Line::default());
        }

        // Handle scrolling
        let content_height = lines.len() as u16;
        let visible_height = inner.height;
        let max_scroll = content_height.saturating_sub(visible_height);
        let scroll_offset = self.scroll_offset.min(max_scroll);

        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(scroll_offset as usize)
            .take(visible_height as usize)
            .collect();

        let paragraph = Paragraph::new(visible_lines).block(block);
        frame.render_widget(paragraph, area);

        // Render scrollbar if needed
        if content_height > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"));

            let mut scrollbar_state = ScrollbarState::new(max_scroll as usize)
                .position(scroll_offset as usize);

            frame.render_stateful_widget(
                scrollbar,
                area.inner(ratatui::layout::Margin { horizontal: 0, vertical: 1 }),
                &mut scrollbar_state,
            );
        }
    }
}

impl Default for SettingsEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for SettingsEditor {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.handle_key(*key),
            Event::Mouse(mouse) => self.handle_mouse(*mouse),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::SettingsNextSection => self.next_section(),
            Action::SettingsPrevSection => self.prev_section(),
            Action::SettingsNextItem => self.next_item(),
            Action::SettingsPrevItem => self.prev_item(),
            Action::SettingsScrollUp(n) => self.scroll_up(*n),
            Action::SettingsScrollDown(n) => self.scroll_down(*n),
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
    fn test_settings_editor_new() {
        let editor = SettingsEditor::new();
        assert_eq!(editor.selected_section, 0);
        assert_eq!(editor.selected_item, 0);
        assert!(matches!(editor.input_mode, SettingsInputMode::Normal));
    }

    #[test]
    fn test_section_navigation() {
        let mut editor = SettingsEditor::new();
        assert_eq!(editor.current_section(), SettingsSection::ApiKeys);

        editor.next_section();
        assert_eq!(editor.current_section(), SettingsSection::Provider);

        editor.next_section();
        assert_eq!(editor.current_section(), SettingsSection::Model);

        editor.prev_section();
        assert_eq!(editor.current_section(), SettingsSection::Provider);

        // Wrap around
        editor.selected_section = 0;
        editor.prev_section();
        assert_eq!(editor.current_section(), SettingsSection::Parameters);
    }

    #[test]
    fn test_item_navigation() {
        let mut editor = SettingsEditor::new();
        assert_eq!(editor.selected_item, 0);

        editor.next_item();
        assert_eq!(editor.selected_item, 1);

        editor.prev_item();
        assert_eq!(editor.selected_item, 0);

        // Wrap around
        editor.prev_item();
        assert_eq!(editor.selected_item, editor.available_providers.len() - 1);
    }

    #[test]
    fn test_editing_mode() {
        let mut editor = SettingsEditor::new();
        assert!(!editor.is_editing());

        editor.start_editing();
        assert!(editor.is_editing());

        if let SettingsInputMode::Editing { field, buffer, masked } = &editor.input_mode {
            assert_eq!(field, "anthropic");
            assert!(buffer.is_empty());
            assert!(*masked);
        } else {
            panic!("Expected Editing mode");
        }

        editor.handle_edit_char('a');
        editor.handle_edit_char('b');
        editor.handle_edit_char('c');

        if let SettingsInputMode::Editing { buffer, .. } = &editor.input_mode {
            assert_eq!(buffer, "abc");
        }

        editor.handle_edit_backspace();
        if let SettingsInputMode::Editing { buffer, .. } = &editor.input_mode {
            assert_eq!(buffer, "ab");
        }

        editor.cancel_editing();
        assert!(!editor.is_editing());
    }

    #[test]
    fn test_provider_selection() {
        let mut editor = SettingsEditor::new();
        editor.selected_section = 1; // Provider section
        editor.selected_item = 1; // openai

        let action = editor.select_provider();
        assert!(action.is_some());
        assert_eq!(editor.config.defaults.provider, "openai");
    }

    #[test]
    fn test_model_selection() {
        let mut editor = SettingsEditor::new();
        editor.selected_section = 2; // Model section
        editor.refresh_models_for_provider();
        editor.selected_item = 1; // Second model

        let action = editor.select_model();
        assert!(action.is_some());
        assert_eq!(editor.config.defaults.model, editor.available_models[1]);
    }

    #[test]
    fn test_section_display() {
        assert_eq!(SettingsSection::ApiKeys.as_str(), "API Keys");
        assert_eq!(SettingsSection::Provider.as_str(), "Provider");
        assert_eq!(SettingsSection::Model.as_str(), "Model");
        assert_eq!(SettingsSection::Parameters.as_str(), "Parameters");
    }

    #[test]
    fn test_set_config() {
        let mut editor = SettingsEditor::new();
        let mut config = LLMConfig::default();
        config.defaults.provider = "openai".to_string();
        config.defaults.model = "gpt-4o".to_string();

        editor.set_config(config);

        assert_eq!(editor.current_provider(), "openai");
        assert_eq!(editor.current_model(), "gpt-4o");
        assert!(!editor.available_models.is_empty());
    }

    #[test]
    fn test_provider_key_status() {
        let mut editor = SettingsEditor::new();
        editor.set_provider_keys(vec![
            ProviderKeyStatus { name: "anthropic".to_string(), has_key: true },
            ProviderKeyStatus { name: "openai".to_string(), has_key: false },
        ]);

        assert_eq!(editor.provider_keys.len(), 2);
        assert!(editor.provider_keys[0].has_key);
        assert!(!editor.provider_keys[1].has_key);
    }
}
