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
use crate::agent::ModelCatalog;
use crate::components::Component;
use crate::config::{KeyId, KeyStore, LLMConfig, Theme};

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
    Editing { 
        field: String, 
        buffer: String, 
        /// Whether input is masked (shows dots instead of chars)
        masked: bool,
        /// Whether mask is currently hidden (user toggled visibility)
        show_plain: bool,
    },
}

/// Provider with key status
#[derive(Debug, Clone)]
pub struct ProviderKeyStatus {
    pub name: String,
    pub has_key: bool,
}

/// Status of a key test operation (TS-007)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyTestStatus {
    /// No test in progress
    Idle,
    /// Testing in progress for this provider
    Testing(String),
    /// Test passed
    Success(String),
    /// Test failed with error message
    Failed(String, String),
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
    /// Key test status (TS-007)
    key_test_status: KeyTestStatus,
    /// Model catalog for rich model info (TS-009)
    model_catalog: ModelCatalog,
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
            key_test_status: KeyTestStatus::Idle,
            model_catalog: ModelCatalog::new(),
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

    /// Load key statuses from the keystore (TS-006)
    /// Queries the keystore to determine which providers have keys configured
    pub fn load_key_statuses_from_keystore(&mut self, keystore: &KeyStore) {
        let mut statuses = Vec::new();
        
        for provider in &self.available_providers {
            let key_id = KeyId::from_provider_str(provider);
            let has_key = keystore.exists(&key_id).unwrap_or(false);
            statuses.push(ProviderKeyStatus {
                name: provider.clone(),
                has_key,
            });
        }
        
        self.provider_keys = statuses;
    }

    /// Mark a single provider's key status as configured (TS-006)
    /// Called after successfully storing a key
    pub fn mark_key_configured(&mut self, provider: &str) {
        if let Some(status) = self.provider_keys.iter_mut().find(|s| s.name == provider) {
            status.has_key = true;
        } else {
            self.provider_keys.push(ProviderKeyStatus {
                name: provider.to_string(),
                has_key: true,
            });
        }
    }

    /// Get the provider name for the current editing session (TS-006)
    /// Returns None if not currently editing
    pub fn editing_provider(&self) -> Option<&str> {
        if let SettingsInputMode::Editing { field, .. } = &self.input_mode {
            Some(field.as_str())
        } else {
            None
        }
    }

    /// Get the currently selected provider in API Keys section (TS-007)
    pub fn selected_provider(&self) -> Option<&str> {
        if self.current_section() == SettingsSection::ApiKeys {
            self.available_providers.get(self.selected_item).map(|s| s.as_str())
        } else {
            None
        }
    }

    /// Start testing a key (TS-007)
    pub fn start_key_test(&mut self, provider: &str) {
        self.key_test_status = KeyTestStatus::Testing(provider.to_string());
    }

    /// Update key test result (TS-007)
    pub fn set_key_test_result(&mut self, provider: &str, success: bool, error: Option<String>) {
        if success {
            self.key_test_status = KeyTestStatus::Success(provider.to_string());
        } else {
            self.key_test_status = KeyTestStatus::Failed(
                provider.to_string(),
                error.unwrap_or_else(|| "Unknown error".to_string()),
            );
        }
    }

    /// Clear key test status (TS-007)
    pub fn clear_key_test_status(&mut self) {
        self.key_test_status = KeyTestStatus::Idle;
    }

    /// Get current key test status (TS-007)
    pub fn key_test_status(&self) -> &KeyTestStatus {
        &self.key_test_status
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
                "gpt-5.2-2025-12-11".to_string(),
                "gpt-5.2-pro-2025-12-11".to_string(),
                "gpt-5-mini-2025-08-07".to_string(),
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
                "gpt-4-turbo".to_string(),
                "o1".to_string(),
                "o1-mini".to_string(),
                "o3-mini".to_string(),
            ],
            "gemini" => vec![
                "gemini-2.5-flash".to_string(),
                "gemini-2.5-pro".to_string(),
                "gemini-2.0-flash".to_string(),
                "gemini-1.5-pro".to_string(),
                "gemini-1.5-flash".to_string(),
            ],
            "grok" => vec![
                    "grok-4".to_string(),
                    "grok-4-fast-reasoning".to_string(),
                    "grok-4-fast-non-reasoning".to_string(),
                    "grok-4-1-fast-reasoning".to_string(),
                    "grok-4-1-fast-non-reasoning".to_string(),
                    "grok-code-fast-1".to_string(),
                    "grok-3".to_string(),
                    "grok-3-mini".to_string(),
                    "grok-2-1212".to_string(),
                    "grok-2-vision-1212".to_string(),
                ],
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
                    show_plain: false,
                };
            }
        }
    }

    /// Toggle visibility of masked input (Ctrl+U)
    pub fn toggle_mask_visibility(&mut self) {
        if let SettingsInputMode::Editing { show_plain, .. } = &mut self.input_mode {
            *show_plain = !*show_plain;
        }
    }

    /// Clear the current input buffer (Ctrl+K)
    pub fn clear_input(&mut self) {
        if let SettingsInputMode::Editing { buffer, .. } = &mut self.input_mode {
            buffer.clear();
        }
    }

    /// Paste text into input buffer
    pub fn paste_text(&mut self, text: &str) {
        if let SettingsInputMode::Editing { buffer, .. } = &mut self.input_mode {
            // Filter to only printable ASCII chars (API keys are typically alphanumeric + symbols)
            let filtered: String = text.chars()
                .filter(|c| c.is_ascii_graphic())
                .collect();
            buffer.push_str(&filtered);
        }
    }

    /// Get expected key prefix hint for a provider
    fn key_prefix_hint(provider: &str) -> &'static str {
        match provider {
            "anthropic" => "sk-ant-...",
            "openai" => "sk-...",
            "gemini" => "AI...",
            "grok" => "xai-...",
            "groq" => "gsk_...",
            _ => "",
        }
    }

    /// Validate key format for a provider (basic checks)
    fn validate_key_format(provider: &str, key: &str) -> Option<&'static str> {
        if key.is_empty() {
            return Some("Key cannot be empty");
        }
        if key.len() < 10 {
            return Some("Key too short");
        }
        // Provider-specific prefix checks
        match provider {
            "anthropic" => {
                if !key.starts_with("sk-ant-") {
                    return Some("Expected prefix: sk-ant-");
                }
            }
            "openai" => {
                if !key.starts_with("sk-") {
                    return Some("Expected prefix: sk-");
                }
            }
            "groq" => {
                if !key.starts_with("gsk_") {
                    return Some("Expected prefix: gsk_");
                }
            }
            "grok" => {
                if !key.starts_with("xai-") {
                    return Some("Expected prefix: xai-");
                }
            }
            _ => {}
        }
        None
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

    // ==================== TS-010: Parameters section methods ====================

    /// Temperature range constants
    const TEMPERATURE_MIN: f32 = 0.0;
    const TEMPERATURE_MAX: f32 = 2.0;
    const TEMPERATURE_STEP: f32 = 0.1;

    /// Max tokens range constants
    const MAX_TOKENS_MIN: u32 = 1;
    const MAX_TOKENS_MAX: u32 = 128_000;
    const MAX_TOKENS_STEP: u32 = 256;
    const MAX_TOKENS_LARGE_STEP: u32 = 1024;

    /// Increase temperature by step
    pub fn increase_temperature(&mut self) -> Option<Action> {
        if self.current_section() == SettingsSection::Parameters && self.selected_item == 0 {
            let new_temp = (self.config.parameters.temperature + Self::TEMPERATURE_STEP)
                .min(Self::TEMPERATURE_MAX);
            // Round to 1 decimal place to avoid floating point drift
            self.config.parameters.temperature = (new_temp * 10.0).round() / 10.0;
            return Some(Action::SettingsTemperatureChanged(self.config.parameters.temperature));
        }
        None
    }

    /// Decrease temperature by step
    pub fn decrease_temperature(&mut self) -> Option<Action> {
        if self.current_section() == SettingsSection::Parameters && self.selected_item == 0 {
            let new_temp = (self.config.parameters.temperature - Self::TEMPERATURE_STEP)
                .max(Self::TEMPERATURE_MIN);
            self.config.parameters.temperature = (new_temp * 10.0).round() / 10.0;
            return Some(Action::SettingsTemperatureChanged(self.config.parameters.temperature));
        }
        None
    }

    /// Increase max tokens by step
    pub fn increase_max_tokens(&mut self) -> Option<Action> {
        if self.current_section() == SettingsSection::Parameters && self.selected_item == 1 {
            let step = if self.config.parameters.max_tokens >= 8192 {
                Self::MAX_TOKENS_LARGE_STEP
            } else {
                Self::MAX_TOKENS_STEP
            };
            self.config.parameters.max_tokens = self.config.parameters.max_tokens
                .saturating_add(step)
                .min(Self::MAX_TOKENS_MAX);
            return Some(Action::SettingsMaxTokensChanged(self.config.parameters.max_tokens));
        }
        None
    }

    /// Decrease max tokens by step
    pub fn decrease_max_tokens(&mut self) -> Option<Action> {
        if self.current_section() == SettingsSection::Parameters && self.selected_item == 1 {
            let step = if self.config.parameters.max_tokens > 8192 {
                Self::MAX_TOKENS_LARGE_STEP
            } else {
                Self::MAX_TOKENS_STEP
            };
            self.config.parameters.max_tokens = self.config.parameters.max_tokens
                .saturating_sub(step)
                .max(Self::MAX_TOKENS_MIN);
            return Some(Action::SettingsMaxTokensChanged(self.config.parameters.max_tokens));
        }
        None
    }

    /// Adjust current parameter (left = decrease, right = increase)
    pub fn adjust_parameter(&mut self, increase: bool) -> Option<Action> {
        if self.current_section() != SettingsSection::Parameters {
            return None;
        }
        match self.selected_item {
            0 => {
                if increase {
                    self.increase_temperature()
                } else {
                    self.decrease_temperature()
                }
            }
            1 => {
                if increase {
                    self.increase_max_tokens()
                } else {
                    self.decrease_max_tokens()
                }
            }
            _ => None,
        }
    }

    /// Get temperature as a percentage (0-100) for slider rendering
    fn temperature_percentage(&self) -> u8 {
        let range = Self::TEMPERATURE_MAX - Self::TEMPERATURE_MIN;
        let normalized = (self.config.parameters.temperature - Self::TEMPERATURE_MIN) / range;
        (normalized * 100.0).round() as u8
    }

    /// Get max tokens as a percentage (0-100) for slider rendering
    fn max_tokens_percentage(&self) -> u8 {
        // Use logarithmic scale for better UX with large token ranges
        let log_min = (Self::MAX_TOKENS_MIN as f64).ln();
        let log_max = (Self::MAX_TOKENS_MAX as f64).ln();
        let log_val = (self.config.parameters.max_tokens as f64).ln();
        let normalized = (log_val - log_min) / (log_max - log_min);
        (normalized * 100.0).round() as u8
    }

    /// Render a slider bar (width chars, filled to percentage)
    fn render_slider_bar(percentage: u8, width: usize) -> String {
        let filled = (percentage as usize * width) / 100;
        let empty = width.saturating_sub(filled);
        format!("{}{}",
            "█".repeat(filled),
            "░".repeat(empty)
        )
    }

    /// Get description for temperature value
    fn temperature_description(temp: f32) -> &'static str {
        if temp <= 0.2 {
            "Very deterministic - same output each time"
        } else if temp <= 0.5 {
            "Focused - predictable with minor variation"
        } else if temp <= 0.8 {
            "Balanced - good mix of creativity and focus"
        } else if temp <= 1.2 {
            "Creative - more varied responses"
        } else if temp <= 1.6 {
            "Highly creative - quite random"
        } else {
            "Maximum randomness - unpredictable"
        }
    }

    /// Get description for max tokens value
    fn max_tokens_description(tokens: u32) -> &'static str {
        if tokens <= 256 {
            "Very short - quick responses"
        } else if tokens <= 1024 {
            "Short - concise answers"
        } else if tokens <= 4096 {
            "Medium - detailed responses"
        } else if tokens <= 8192 {
            "Long - comprehensive output"
        } else if tokens <= 32768 {
            "Very long - extensive generation"
        } else {
            "Maximum - full context capacity"
        }
    }

    /// Format max tokens for display (e.g., 8192 -> "8K", 128000 -> "128K")
    fn format_tokens_display(tokens: u32) -> String {
        if tokens >= 1000 {
            format!("{}K", tokens / 1000)
        } else {
            format!("{}", tokens)
        }
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
                        // Enter/Space on parameters does nothing (use left/right to adjust)
                        None
                    }
                }
            }
            // TS-010: Parameter adjustment with left/right arrows and +/-
            KeyCode::Left | KeyCode::Char('h') => {
                if self.current_section() == SettingsSection::Parameters {
                    self.adjust_parameter(false)
                } else {
                    None
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.current_section() == SettingsSection::Parameters {
                    self.adjust_parameter(true)
                } else {
                    None
                }
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                if self.current_section() == SettingsSection::Parameters {
                    self.adjust_parameter(false)
                } else {
                    None
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if self.current_section() == SettingsSection::Parameters {
                    self.adjust_parameter(true)
                } else {
                    None
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
            // Ctrl+U: Toggle mask visibility
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_mask_visibility();
                None
            }
            // Ctrl+K: Clear input
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.clear_input();
                None
            }
            // Ctrl+V: Paste (clipboard handled externally, but we accept pasted text)
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Note: Terminal paste comes as rapid char events, not Ctrl+V
                // This is a placeholder for potential clipboard integration
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

            // Selection indicator (arrow for selected, space for others)
            let selector = if is_selected { "▸" } else { " " };
            let selector_style = Style::default().fg(theme.colors.accent.to_color());

            let name_style = if is_selected {
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.colors.foreground.to_color())
            };

            // Show input field if editing this provider
            if let SettingsInputMode::Editing { field, buffer, masked, show_plain } = &self.input_mode {
                if field == provider {
                    // Show masked or plain based on toggle
                    let display = if *masked && !*show_plain {
                        "•".repeat(buffer.len())
                    } else {
                        buffer.clone()
                    };
                    let char_count = format!(" ({} chars)", buffer.len());
                    
                    // Visibility indicator
                    let vis_icon = if *show_plain { "👁" } else { "🔒" };
                    
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {} ", selector), selector_style),
                        Span::styled(format!("{:12} ", provider), name_style),
                        Span::styled("[", Style::default().fg(theme.colors.muted.to_color())),
                        Span::styled(display, Style::default().fg(theme.colors.accent.to_color())),
                        Span::styled("█", Style::default().fg(theme.colors.accent.to_color())),
                        Span::styled("]", Style::default().fg(theme.colors.muted.to_color())),
                        Span::styled(format!(" {} ", vis_icon), Style::default().fg(theme.colors.muted.to_color())),
                        Span::styled(char_count, Style::default().fg(theme.colors.muted.to_color())),
                    ]));
                    
                    // Add format hint if buffer is empty
                    if buffer.is_empty() {
                        let prefix_hint = Self::key_prefix_hint(provider);
                        if !prefix_hint.is_empty() {
                            lines.push(Line::from(vec![
                                Span::styled("                    ", Style::default()),
                                Span::styled(format!("Format: {}", prefix_hint), Style::default().fg(theme.colors.muted.to_color())),
                            ]));
                        }
                    }
                    
                    // Add hint line with keybindings
                    lines.push(Line::from(vec![
                        Span::styled("                    ", Style::default()),
                        Span::styled("↵ save  ", Style::default().fg(theme.colors.success.to_color())),
                        Span::styled("Esc cancel  ", Style::default().fg(theme.colors.muted.to_color())),
                        Span::styled("^U show/hide  ", Style::default().fg(theme.colors.secondary.to_color())),
                        Span::styled("^K clear", Style::default().fg(theme.colors.warning.to_color())),
                    ]));
                    continue;
                }
            }

            // Check test status for this provider (TS-007)
            let test_status_for_provider = match &self.key_test_status {
                KeyTestStatus::Testing(p) if p == provider => Some("⏳ testing..."),
                KeyTestStatus::Success(p) if p == provider => Some("✓ valid"),
                KeyTestStatus::Failed(p, _) if p == provider => Some("✗ invalid"),
                _ => None,
            };

            // Build the line with optional action hints for selected item
            let mut spans = vec![
                Span::styled(format!(" {} ", selector), selector_style),
                Span::styled(format!("{:12} ", provider), name_style),
                Span::styled(status_icon.to_string(), Style::default().fg(status_color)),
                Span::styled(
                    if has_key { " configured" } else { " not set" }.to_string(),
                    Style::default().fg(theme.colors.muted.to_color()),
                ),
            ];

            // Show test status if applicable (TS-007)
            if let Some(status) = test_status_for_provider {
                let color = match &self.key_test_status {
                    KeyTestStatus::Testing(_) => theme.colors.warning.to_color(),
                    KeyTestStatus::Success(_) => theme.colors.success.to_color(),
                    KeyTestStatus::Failed(_, _) => theme.colors.error.to_color(),
                    _ => theme.colors.muted.to_color(),
                };
                spans.push(Span::styled(format!("  {}", status), Style::default().fg(color)));
            }

            // Add action hints for selected item
            if is_selected && test_status_for_provider.is_none() {
                spans.push(Span::styled("  ", Style::default()));
                spans.push(Span::styled("↵ edit", Style::default().fg(theme.colors.primary.to_color())));
                if has_key {
                    spans.push(Span::styled("  ^T test", Style::default().fg(theme.colors.secondary.to_color())));
                }
            }

            lines.push(Line::from(spans));

            // Show error message if test failed (TS-007)
            if let KeyTestStatus::Failed(p, err) = &self.key_test_status {
                if p == provider {
                    lines.push(Line::from(vec![
                        Span::styled("                    ", Style::default()),
                        Span::styled(format!("Error: {}", err), Style::default().fg(theme.colors.error.to_color())),
                    ]));
                }
            }
        }

        lines
    }

    /// Get a description for a provider
    fn provider_description(provider: &str) -> &'static str {
        match provider {
            "anthropic" => "Claude models - Advanced reasoning & coding",
            "openai" => "GPT models - General purpose AI",
            "gemini" => "Google's Gemini - Multimodal AI",
            "grok" => "xAI Grok - Real-time knowledge",
            "groq" => "Groq - Ultra-fast inference",
            _ => "Custom provider",
        }
    }

    /// Get the number of available models for a provider
    fn model_count_for_provider(&self, provider: &str) -> usize {
        match provider {
            "anthropic" => 4,
            "openai" => 3,
            "gemini" => 3,
            "grok" => 2,
            "groq" => 3,
            _ => 0,
        }
    }

    /// Get a short description for a model (TS-009)
    fn model_description(model: &str) -> &'static str {
        match model {
            // Anthropic Claude 4.5 models (latest)
            "claude-opus-4-5-20251101" => "Opus 4.5 - Most capable, deep reasoning with thinking",
            "claude-sonnet-4-5-20250929" => "Sonnet 4.5 - Best balance of speed & intelligence",
            "claude-haiku-4-5-20251001" => "Haiku 4.5 - Fast & capable with thinking",
            // Anthropic Claude 4 models
            "claude-sonnet-4-20250514" => "Sonnet 4 - Previous generation, extended thinking",
            "claude-opus-4-20250514" => "Opus 4 - Previous generation, deep reasoning",
            // Anthropic Claude 3.5 models
            "claude-3-5-sonnet-20241022" => "3.5 Sonnet - Fast, capable coding model",
            "claude-3-5-haiku-20241022" => "3.5 Haiku - Fast, lightweight tasks",
            // Anthropic Claude 3 models (legacy)
            "claude-3-opus-20240229" => "3 Opus - Legacy, deep reasoning",
            "claude-3-haiku-20240307" => "3 Haiku - Legacy, fastest responses",
            // OpenAI GPT-5 models (latest)
            "gpt-5.2-2025-12-11" => "GPT-5.2 - Latest flagship model",
            "gpt-5.2-pro-2025-12-11" => "GPT-5.2 Pro - Deep reasoning",
            "gpt-5-mini-2025-08-07" => "GPT-5 Mini - Fast & efficient",
            // OpenAI GPT-4 models
            "gpt-4o" => "GPT-4o - Multimodal flagship",
            "gpt-4o-mini" => "GPT-4o Mini - Fast, affordable",
            "gpt-4-turbo" => "GPT-4 Turbo - 128K context",
            // OpenAI o-series (reasoning)
            "o1" => "o1 - Advanced reasoning",
            "o1-mini" => "o1 Mini - Fast reasoning",
            "o3-mini" => "o3 Mini - Latest reasoning",
            // Gemini 2.5 models (latest)
            "gemini-2.5-flash" => "2.5 Flash - Latest fast multimodal",
            "gemini-2.5-pro" => "2.5 Pro - Thinking, deep reasoning",
            // Gemini 2.0 models
            "gemini-2.0-flash" => "2.0 Flash - Fast multimodal",
            // Gemini 1.5 models
            "gemini-1.5-pro" => "1.5 Pro - 2M context, document understanding",
            "gemini-1.5-flash" => "1.5 Flash - Fast, efficient",
            // Grok 4 models (latest)
            "grok-4" => "Grok 4 - Flagship with thinking",
            "grok-4-fast-reasoning" => "Grok 4 Fast - 2M context, reasoning",
            "grok-4-fast-non-reasoning" => "Grok 4 Fast - 2M context, speed",
            "grok-4-1-fast-reasoning" => "Grok 4.1 Fast - Latest reasoning",
            "grok-4-1-fast-non-reasoning" => "Grok 4.1 Fast - Latest speed",
            "grok-code-fast-1" => "Grok Code - Optimized for coding",
            // Grok 3 models
            "grok-3" => "Grok 3 - Previous generation",
            "grok-3-mini" => "Grok 3 Mini - Lightweight",
            // Grok 2 models (legacy)
            "grok-2-1212" => "Grok 2 - Legacy",
            "grok-2-vision-1212" => "Grok 2 Vision - Image understanding",
            // Groq models
            "llama-3.3-70b-versatile" => "Latest Llama - Ultra-fast inference",
            "llama-3.1-8b-instant" => "Small Llama - Instant responses",
            "mixtral-8x7b-32768" => "MoE model - 32K context",
            _ => "",
        }
    }

    /// Format token count in a human-friendly way (e.g., 200K, 1M) (TS-009)
    fn format_context_window(tokens: u32) -> String {
        if tokens >= 1_000_000 {
            format!("{}M", tokens / 1_000_000)
        } else if tokens >= 1_000 {
            format!("{}K", tokens / 1_000)
        } else {
            format!("{}", tokens)
        }
    }

    fn render_provider_section(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for (idx, provider) in self.available_providers.iter().enumerate() {
            let is_selected = self.current_section() == SettingsSection::Provider 
                && self.selected_item == idx;
            let is_current = provider == &self.config.defaults.provider;

            // Check if key is configured for this provider
            let has_key = self.provider_keys
                .iter()
                .find(|p| &p.name == provider)
                .map(|p| p.has_key)
                .unwrap_or(false);

            // Selection arrow (like API Keys section)
            let selector = if is_selected { "▸" } else { " " };
            let selector_style = Style::default().fg(theme.colors.accent.to_color());

            // Radio button marker for current selection
            let marker = if is_current { "●" } else { "○" };
            let marker_style = if is_current {
                Style::default().fg(theme.colors.success.to_color())
            } else {
                Style::default().fg(theme.colors.muted.to_color())
            };

            // Provider name styling
            let name_style = if is_selected {
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme.colors.primary.to_color())
            } else {
                Style::default().fg(theme.colors.foreground.to_color())
            };

            // Key status indicator
            let key_icon = if has_key { "🔑" } else { "  " };
            let key_style = if has_key {
                Style::default().fg(theme.colors.success.to_color())
            } else {
                Style::default()
            };

            // Model count
            let model_count = self.model_count_for_provider(provider);
            let model_info = format!("{} models", model_count);

            // Build the main line
            let mut spans = vec![
                Span::styled(format!(" {} ", selector), selector_style),
                Span::styled(format!("{} ", marker), marker_style),
                Span::styled(format!("{:10}", provider), name_style),
                Span::styled(format!(" {} ", key_icon), key_style),
                Span::styled(
                    format!("({}) ", model_info),
                    Style::default().fg(theme.colors.muted.to_color()),
                ),
            ];

            // Add selection hint for selected item
            if is_selected {
                spans.push(Span::styled(
                    "↵ select",
                    Style::default().fg(theme.colors.primary.to_color()),
                ));
            }

            lines.push(Line::from(spans));

            // Show description for selected provider
            if is_selected {
                let desc = Self::provider_description(provider);
                lines.push(Line::from(vec![
                    Span::styled("        ", Style::default()),
                    Span::styled(
                        desc.to_string(),
                        Style::default().fg(theme.colors.secondary.to_color()),
                    ),
                ]));

                // Show warning if key not configured
                if !has_key {
                    lines.push(Line::from(vec![
                        Span::styled("        ", Style::default()),
                        Span::styled(
                            "⚠ API key not configured",
                            Style::default().fg(theme.colors.warning.to_color()),
                        ),
                    ]));
                }
            }
        }

        lines
    }

    fn render_model_section(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for (idx, model) in self.available_models.iter().enumerate() {
            let is_selected = self.current_section() == SettingsSection::Model 
                && self.selected_item == idx;
            let is_current = model == &self.config.defaults.model;

            // Get model info from catalog
            let model_info = self.model_catalog.info_for(model);

            // Selection arrow (like Provider section)
            let selector = if is_selected { "▸" } else { " " };
            let selector_style = Style::default().fg(theme.colors.accent.to_color());

            // Radio button marker for current selection
            let marker = if is_current { "●" } else { "○" };
            let marker_style = if is_current {
                Style::default().fg(theme.colors.success.to_color())
            } else {
                Style::default().fg(theme.colors.muted.to_color())
            };

            // Model name styling
            let name_style = if is_selected {
                Style::default()
                    .fg(theme.colors.accent.to_color())
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme.colors.primary.to_color())
            } else {
                Style::default().fg(theme.colors.foreground.to_color())
            };

            // Context window formatted (e.g., 200K, 1M)
            let context_size = Self::format_context_window(model_info.max_context_tokens);

            // Feature icons
            let thinking_icon = if model_info.supports_thinking { "🧠" } else { "  " };
            let tools_icon = if model_info.supports_tools { "🔧" } else { "  " };

            // Build the main line
            let mut spans = vec![
                Span::styled(format!(" {} ", selector), selector_style),
                Span::styled(format!("{} ", marker), marker_style),
                Span::styled(format!("{:<32}", model), name_style),
                Span::styled(
                    format!(" {} ", context_size),
                    Style::default().fg(theme.colors.secondary.to_color()),
                ),
                Span::styled(format!("{} ", thinking_icon), Style::default()),
                Span::styled(format!("{} ", tools_icon), Style::default()),
            ];

            // Add selection hint for selected item
            if is_selected {
                spans.push(Span::styled(
                    "↵ select",
                    Style::default().fg(theme.colors.primary.to_color()),
                ));
            }

            lines.push(Line::from(spans));

            // Show description for selected model
            if is_selected {
                let desc = Self::model_description(model);
                if !desc.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("        ", Style::default()),
                        Span::styled(
                            desc.to_string(),
                            Style::default().fg(theme.colors.secondary.to_color()),
                        ),
                    ]));
                }

                // Show feature legend on first selected model
                lines.push(Line::from(vec![
                    Span::styled("        ", Style::default()),
                    Span::styled(
                        format!("Context: {} tokens", model_info.max_context_tokens),
                        Style::default().fg(theme.colors.muted.to_color()),
                    ),
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        if model_info.supports_thinking { "🧠 thinking" } else { "" },
                        Style::default().fg(theme.colors.muted.to_color()),
                    ),
                    Span::styled(
                        if model_info.supports_tools { " 🔧 tools" } else { "" },
                        Style::default().fg(theme.colors.muted.to_color()),
                    ),
                ]));
            }
        }

        // Show empty state if no models available
        if lines.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "  No models available for this provider",
                    Style::default().fg(theme.colors.muted.to_color()),
                ),
            ]));
        }

        lines
    }

    fn render_parameters_section(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        const SLIDER_WIDTH: usize = 20;

        // ===== Temperature (item 0) =====
        let temp_selected = self.current_section() == SettingsSection::Parameters 
            && self.selected_item == 0;
        
        let temp_selector = if temp_selected { "▸" } else { " " };
        let temp_selector_style = Style::default().fg(theme.colors.accent.to_color());
        
        let temp_label_style = if temp_selected {
            Style::default()
                .fg(theme.colors.accent.to_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.colors.foreground.to_color())
        };

        let temp_value = format!("{:.1}", self.config.parameters.temperature);
        let temp_slider = Self::render_slider_bar(self.temperature_percentage(), SLIDER_WIDTH);

        // Main temperature line: selector, label, slider, value
        let mut temp_spans = vec![
            Span::styled(format!(" {} ", temp_selector), temp_selector_style),
            Span::styled("Temperature   ", temp_label_style),
            Span::styled(
                format!("[{}] ", temp_slider),
                Style::default().fg(theme.colors.secondary.to_color()),
            ),
            Span::styled(
                temp_value,
                Style::default().fg(theme.colors.primary.to_color()),
            ),
        ];

        // Add adjustment hint for selected
        if temp_selected {
            temp_spans.push(Span::styled(
                "  ←/→ adjust",
                Style::default().fg(theme.colors.muted.to_color()),
            ));
        }

        lines.push(Line::from(temp_spans));

        // Temperature description line (only when selected)
        if temp_selected {
            let desc = Self::temperature_description(self.config.parameters.temperature);
            lines.push(Line::from(vec![
                Span::styled("        ", Style::default()),
                Span::styled(
                    desc.to_string(),
                    Style::default().fg(theme.colors.secondary.to_color()),
                ),
            ]));
            // Range hint
            lines.push(Line::from(vec![
                Span::styled("        ", Style::default()),
                Span::styled(
                    format!("Range: {:.1} - {:.1} (step {:.1})", 
                        Self::TEMPERATURE_MIN, Self::TEMPERATURE_MAX, Self::TEMPERATURE_STEP),
                    Style::default().fg(theme.colors.muted.to_color()),
                ),
            ]));
        }

        // Spacing
        lines.push(Line::default());

        // ===== Max Tokens (item 1) =====
        let tokens_selected = self.current_section() == SettingsSection::Parameters 
            && self.selected_item == 1;
        
        let tokens_selector = if tokens_selected { "▸" } else { " " };
        let tokens_selector_style = Style::default().fg(theme.colors.accent.to_color());
        
        let tokens_label_style = if tokens_selected {
            Style::default()
                .fg(theme.colors.accent.to_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.colors.foreground.to_color())
        };

        let tokens_value = Self::format_tokens_display(self.config.parameters.max_tokens);
        let tokens_slider = Self::render_slider_bar(self.max_tokens_percentage(), SLIDER_WIDTH);

        // Main tokens line: selector, label, slider, value
        let mut tokens_spans = vec![
            Span::styled(format!(" {} ", tokens_selector), tokens_selector_style),
            Span::styled("Max Tokens    ", tokens_label_style),
            Span::styled(
                format!("[{}] ", tokens_slider),
                Style::default().fg(theme.colors.secondary.to_color()),
            ),
            Span::styled(
                tokens_value,
                Style::default().fg(theme.colors.primary.to_color()),
            ),
        ];

        // Add adjustment hint for selected
        if tokens_selected {
            tokens_spans.push(Span::styled(
                "  ←/→ adjust",
                Style::default().fg(theme.colors.muted.to_color()),
            ));
        }

        lines.push(Line::from(tokens_spans));

        // Tokens description line (only when selected)
        if tokens_selected {
            let desc = Self::max_tokens_description(self.config.parameters.max_tokens);
            lines.push(Line::from(vec![
                Span::styled("        ", Style::default()),
                Span::styled(
                    desc.to_string(),
                    Style::default().fg(theme.colors.secondary.to_color()),
                ),
            ]));
            // Range hint with exact value
            lines.push(Line::from(vec![
                Span::styled("        ", Style::default()),
                Span::styled(
                    format!("Value: {} tokens (range: {} - {})", 
                        self.config.parameters.max_tokens,
                        Self::MAX_TOKENS_MIN, 
                        Self::format_tokens_display(Self::MAX_TOKENS_MAX)),
                    Style::default().fg(theme.colors.muted.to_color()),
                ),
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

        if let SettingsInputMode::Editing { field, buffer, masked, show_plain } = &editor.input_mode {
            assert_eq!(field, "anthropic");
            assert!(buffer.is_empty());
            assert!(*masked);
            assert!(!*show_plain);
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

    #[test]
    fn test_api_keys_section_render_content() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.set_provider_keys(vec![
            ProviderKeyStatus { name: "anthropic".to_string(), has_key: true },
            ProviderKeyStatus { name: "openai".to_string(), has_key: false },
        ]);
        
        let theme = Theme::default();
        let lines = editor.render_api_keys_section(&theme);
        
        // Should have one line per provider
        assert_eq!(lines.len(), editor.available_providers.len());
        
        // First line should have selection indicator (selected by default)
        let first_line = &lines[0];
        assert!(!first_line.spans.is_empty());
        // Selected item should have "▸" selector and action hints
        let line_text: String = first_line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(line_text.contains("▸"), "Selected item should have arrow selector");
        assert!(line_text.contains("↵ edit"), "Selected item should show edit hint");
    }

    #[test]
    fn test_api_keys_editing_shows_char_count() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.start_editing();
        
        // Type some characters
        editor.handle_edit_char('s');
        editor.handle_edit_char('k');
        editor.handle_edit_char('-');
        
        let theme = Theme::default();
        let lines = editor.render_api_keys_section(&theme);
        
        // Find the line with the input field (should be first, for anthropic)
        let input_line = &lines[0];
        let line_text: String = input_line.spans.iter().map(|s| s.content.as_ref()).collect();
        
        // Should show character count
        assert!(line_text.contains("(3 chars)"), "Should show character count during editing");
        // Should show masked dots
        assert!(line_text.contains("•••"), "Should show masked input");
    }

    #[test]
    fn test_toggle_mask_visibility() {
        let mut editor = SettingsEditor::new();
        editor.start_editing();
        
        // Initially masked (show_plain = false)
        if let SettingsInputMode::Editing { show_plain, .. } = &editor.input_mode {
            assert!(!*show_plain);
        }
        
        // Toggle visibility
        editor.toggle_mask_visibility();
        if let SettingsInputMode::Editing { show_plain, .. } = &editor.input_mode {
            assert!(*show_plain);
        }
        
        // Toggle back
        editor.toggle_mask_visibility();
        if let SettingsInputMode::Editing { show_plain, .. } = &editor.input_mode {
            assert!(!*show_plain);
        }
    }

    #[test]
    fn test_clear_input() {
        let mut editor = SettingsEditor::new();
        editor.start_editing();
        
        editor.handle_edit_char('a');
        editor.handle_edit_char('b');
        editor.handle_edit_char('c');
        
        if let SettingsInputMode::Editing { buffer, .. } = &editor.input_mode {
            assert_eq!(buffer, "abc");
        }
        
        editor.clear_input();
        
        if let SettingsInputMode::Editing { buffer, .. } = &editor.input_mode {
            assert!(buffer.is_empty());
        }
    }

    #[test]
    fn test_paste_text() {
        let mut editor = SettingsEditor::new();
        editor.start_editing();
        
        // Paste valid text
        editor.paste_text("sk-ant-abc123");
        
        if let SettingsInputMode::Editing { buffer, .. } = &editor.input_mode {
            assert_eq!(buffer, "sk-ant-abc123");
        }
        
        // Clear and paste text with invalid chars (should filter)
        editor.clear_input();
        editor.paste_text("sk-ant-\n\t abc");  // newline, tab, space filtered
        
        if let SettingsInputMode::Editing { buffer, .. } = &editor.input_mode {
            assert_eq!(buffer, "sk-ant-abc");
        }
    }

    #[test]
    fn test_key_validation() {
        // Empty key
        assert!(SettingsEditor::validate_key_format("anthropic", "").is_some());
        
        // Too short
        assert!(SettingsEditor::validate_key_format("anthropic", "sk-ant").is_some());
        
        // Wrong prefix for anthropic
        assert!(SettingsEditor::validate_key_format("anthropic", "sk-wrong-prefix-key").is_some());
        
        // Valid anthropic key
        assert!(SettingsEditor::validate_key_format("anthropic", "sk-ant-valid-key-12345").is_none());
        
        // Valid openai key
        assert!(SettingsEditor::validate_key_format("openai", "sk-valid-openai-key").is_none());
        
        // Gemini has no prefix check, just length
        assert!(SettingsEditor::validate_key_format("gemini", "AIza-some-key").is_none());
    }

    #[test]
    fn test_key_prefix_hints() {
        assert_eq!(SettingsEditor::key_prefix_hint("anthropic"), "sk-ant-...");
        assert_eq!(SettingsEditor::key_prefix_hint("openai"), "sk-...");
        assert_eq!(SettingsEditor::key_prefix_hint("groq"), "gsk_...");
        assert_eq!(SettingsEditor::key_prefix_hint("grok"), "xai-...");
        assert_eq!(SettingsEditor::key_prefix_hint("unknown"), "");
    }

    #[test]
    fn test_editing_shows_visibility_toggle_hint() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.start_editing();
        
        let theme = Theme::default();
        let lines = editor.render_api_keys_section(&theme);
        
        // Find the hints line (contains keybindings)
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        assert!(all_text.contains("^U show/hide"), "Should show visibility toggle hint");
        assert!(all_text.contains("^K clear"), "Should show clear hint");
    }

    #[test]
    fn test_editing_shows_format_hint_when_empty() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.start_editing();  // Empty buffer
        
        let theme = Theme::default();
        let lines = editor.render_api_keys_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        assert!(all_text.contains("Format: sk-ant-..."), "Should show format hint when buffer is empty");
    }

    #[test]
    fn test_mark_key_configured() {
        let mut editor = SettingsEditor::new();
        
        // Initially no keys configured
        assert!(editor.provider_keys.is_empty());
        
        // Mark anthropic as configured
        editor.mark_key_configured("anthropic");
        assert_eq!(editor.provider_keys.len(), 1);
        assert!(editor.provider_keys[0].has_key);
        assert_eq!(editor.provider_keys[0].name, "anthropic");
        
        // Mark again should update, not duplicate
        editor.mark_key_configured("anthropic");
        assert_eq!(editor.provider_keys.len(), 1);
        
        // Mark another provider
        editor.mark_key_configured("openai");
        assert_eq!(editor.provider_keys.len(), 2);
    }

    #[test]
    fn test_editing_provider() {
        let mut editor = SettingsEditor::new();
        
        // Not editing - should be None
        assert!(editor.editing_provider().is_none());
        
        // Start editing anthropic (first provider, selected by default)
        editor.start_editing();
        assert_eq!(editor.editing_provider(), Some("anthropic"));
        
        // Cancel editing
        editor.cancel_editing();
        assert!(editor.editing_provider().is_none());
        
        // Select a different provider and edit
        editor.selected_item = 1; // openai
        editor.start_editing();
        assert_eq!(editor.editing_provider(), Some("openai"));
    }

    #[test]
    fn test_set_provider_keys_updates_status() {
        let mut editor = SettingsEditor::new();
        
        // Set mixed key statuses
        editor.set_provider_keys(vec![
            ProviderKeyStatus { name: "anthropic".to_string(), has_key: true },
            ProviderKeyStatus { name: "openai".to_string(), has_key: false },
            ProviderKeyStatus { name: "gemini".to_string(), has_key: true },
        ]);
        
        assert_eq!(editor.provider_keys.len(), 3);
        assert!(editor.provider_keys[0].has_key);
        assert!(!editor.provider_keys[1].has_key);
        assert!(editor.provider_keys[2].has_key);
    }

    // TS-007 Tests

    #[test]
    fn test_key_test_status_lifecycle() {
        let mut editor = SettingsEditor::new();
        
        // Initially idle
        assert_eq!(editor.key_test_status(), &KeyTestStatus::Idle);
        
        // Start test
        editor.start_key_test("anthropic");
        assert_eq!(editor.key_test_status(), &KeyTestStatus::Testing("anthropic".to_string()));
        
        // Mark success
        editor.set_key_test_result("anthropic", true, None);
        assert_eq!(editor.key_test_status(), &KeyTestStatus::Success("anthropic".to_string()));
        
        // Clear
        editor.clear_key_test_status();
        assert_eq!(editor.key_test_status(), &KeyTestStatus::Idle);
    }

    #[test]
    fn test_key_test_failure() {
        let mut editor = SettingsEditor::new();
        
        editor.start_key_test("openai");
        editor.set_key_test_result("openai", false, Some("Invalid API key".to_string()));
        
        assert_eq!(
            editor.key_test_status(),
            &KeyTestStatus::Failed("openai".to_string(), "Invalid API key".to_string())
        );
    }

    #[test]
    fn test_selected_provider() {
        let mut editor = SettingsEditor::new();
        
        // In API Keys section by default
        assert_eq!(editor.selected_provider(), Some("anthropic"));
        
        // Select openai
        editor.selected_item = 1;
        assert_eq!(editor.selected_provider(), Some("openai"));
        
        // Switch to different section
        editor.next_section();
        assert!(editor.selected_provider().is_none());
    }

    #[test]
    fn test_key_test_ui_display() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.set_provider_keys(vec![
            ProviderKeyStatus { name: "anthropic".to_string(), has_key: true },
        ]);
        
        let theme = Theme::default();
        
        // Start testing
        editor.start_key_test("anthropic");
        let lines = editor.render_api_keys_section(&theme);
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_text.contains("testing"), "Should show testing status");
        
        // Mark success
        editor.set_key_test_result("anthropic", true, None);
        let lines = editor.render_api_keys_section(&theme);
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_text.contains("valid"), "Should show valid status");
        
        // Mark failure
        editor.set_key_test_result("anthropic", false, Some("Auth error".to_string()));
        let lines = editor.render_api_keys_section(&theme);
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_text.contains("invalid"), "Should show invalid status");
        assert!(all_text.contains("Auth error"), "Should show error message");
    }

    // TS-008 Tests: Provider Section Enhanced UI

    #[test]
    fn test_provider_description() {
        assert_eq!(SettingsEditor::provider_description("anthropic"), "Claude models - Advanced reasoning & coding");
        assert_eq!(SettingsEditor::provider_description("openai"), "GPT models - General purpose AI");
        assert_eq!(SettingsEditor::provider_description("gemini"), "Google's Gemini - Multimodal AI");
        assert_eq!(SettingsEditor::provider_description("grok"), "xAI Grok - Real-time knowledge");
        assert_eq!(SettingsEditor::provider_description("groq"), "Groq - Ultra-fast inference");
        assert_eq!(SettingsEditor::provider_description("unknown"), "Custom provider");
    }

    #[test]
    fn test_model_count_for_provider() {
        let editor = SettingsEditor::new();
        
        assert_eq!(editor.model_count_for_provider("anthropic"), 4);
        assert_eq!(editor.model_count_for_provider("openai"), 3);
        assert_eq!(editor.model_count_for_provider("gemini"), 3);
        assert_eq!(editor.model_count_for_provider("grok"), 2);
        assert_eq!(editor.model_count_for_provider("groq"), 3);
        assert_eq!(editor.model_count_for_provider("unknown"), 0);
    }

    #[test]
    fn test_provider_section_shows_selector_and_hint() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.next_section(); // Move to Provider section
        
        let theme = Theme::default();
        let lines = editor.render_provider_section(&theme);
        
        // Should have multiple lines (at least 1 per provider + description lines)
        assert!(lines.len() >= 5, "Provider section should have provider lines plus descriptions");
        
        // First provider should be selected (selected_item = 0)
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show selector arrow
        assert!(all_text.contains("▸"), "Selected provider should have arrow selector");
        // Should show select hint
        assert!(all_text.contains("↵ select"), "Selected provider should show select hint");
        // Should show model count
        assert!(all_text.contains("models"), "Should show model count");
        // Should show description for selected provider
        assert!(all_text.contains("Claude models"), "Should show description for anthropic");
    }

    #[test]
    fn test_provider_section_shows_key_status() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.next_section(); // Move to Provider section
        
        // Set some providers with keys
        editor.set_provider_keys(vec![
            ProviderKeyStatus { name: "anthropic".to_string(), has_key: true },
            ProviderKeyStatus { name: "openai".to_string(), has_key: false },
        ]);
        
        let theme = Theme::default();
        let lines = editor.render_provider_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show key icon for configured provider
        assert!(all_text.contains("🔑"), "Should show key icon for configured provider");
    }

    #[test]
    fn test_provider_section_shows_warning_for_missing_key() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.next_section(); // Move to Provider section
        
        // No keys configured
        editor.set_provider_keys(vec![
            ProviderKeyStatus { name: "anthropic".to_string(), has_key: false },
        ]);
        
        let theme = Theme::default();
        let lines = editor.render_provider_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show warning for selected provider without key
        assert!(all_text.contains("⚠ API key not configured"), "Should show warning for missing key");
    }

    #[test]
    fn test_provider_section_shows_current_marker() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        editor.next_section(); // Move to Provider section
        
        // Default provider is "anthropic"
        let theme = Theme::default();
        let lines = editor.render_provider_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should have filled circle for current provider
        assert!(all_text.contains("●"), "Should show filled circle for current provider");
        // Should have empty circles for other providers
        assert!(all_text.contains("○"), "Should show empty circles for non-current providers");
    }

    // TS-009 Tests: Model Section Enhanced UI

    #[test]
    fn test_model_description() {
        // Updated for new model catalog (TS-010 fix)
        assert_eq!(SettingsEditor::model_description("claude-sonnet-4-20250514"), "Sonnet 4 - Previous generation, extended thinking");
        assert_eq!(SettingsEditor::model_description("claude-sonnet-4-5-20250929"), "Sonnet 4.5 - Best balance of speed & intelligence");
        assert_eq!(SettingsEditor::model_description("gpt-4o"), "GPT-4o - Multimodal flagship");
        assert_eq!(SettingsEditor::model_description("gemini-2.0-flash"), "2.0 Flash - Fast multimodal");
        assert_eq!(SettingsEditor::model_description("grok-3"), "Grok 3 - Previous generation");
        assert_eq!(SettingsEditor::model_description("llama-3.3-70b-versatile"), "Latest Llama - Ultra-fast inference");
        assert_eq!(SettingsEditor::model_description("unknown-model"), "");
    }

    #[test]
    fn test_format_context_window() {
        assert_eq!(SettingsEditor::format_context_window(200_000), "200K");
        assert_eq!(SettingsEditor::format_context_window(1_000_000), "1M");
        assert_eq!(SettingsEditor::format_context_window(128_000), "128K");
        assert_eq!(SettingsEditor::format_context_window(500), "500");
        assert_eq!(SettingsEditor::format_context_window(8_192), "8K");
    }

    #[test]
    fn test_model_section_shows_selector_and_hint() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        // Set up models by setting config
        let mut config = crate::config::LLMConfig::default();
        config.defaults.provider = "anthropic".to_string();
        config.defaults.model = "claude-sonnet-4-20250514".to_string();
        editor.set_config(config);
        
        // Move to Model section (Tab twice: ApiKeys -> Provider -> Model)
        editor.next_section();
        editor.next_section();
        
        let theme = Theme::default();
        let lines = editor.render_model_section(&theme);
        
        // Should have multiple lines (models + description lines)
        assert!(lines.len() >= 4, "Model section should have model lines plus descriptions");
        
        // First model should be selected (selected_item = 0)
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show selector arrow
        assert!(all_text.contains("▸"), "Selected model should have arrow selector");
        // Should show select hint
        assert!(all_text.contains("↵ select"), "Selected model should show select hint");
        // Should show context window
        assert!(all_text.contains("200K") || all_text.contains("200000"), "Should show context window size");
    }

    #[test]
    fn test_model_section_shows_feature_icons() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        // Set up models with anthropic (which has thinking-capable models)
        let mut config = crate::config::LLMConfig::default();
        config.defaults.provider = "anthropic".to_string();
        config.defaults.model = "claude-sonnet-4-20250514".to_string();
        editor.set_config(config);
        
        // Move to Model section
        editor.next_section();
        editor.next_section();
        
        let theme = Theme::default();
        let lines = editor.render_model_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show tools icon (all models support tools by default)
        assert!(all_text.contains("🔧") || all_text.contains("tools"), "Should show tools icon");
        // claude-sonnet-4 supports thinking
        assert!(all_text.contains("🧠") || all_text.contains("thinking"), "Should show thinking icon for claude-sonnet-4");
    }

    #[test]
    fn test_model_section_shows_description() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        let mut config = crate::config::LLMConfig::default();
        config.defaults.provider = "anthropic".to_string();
        config.defaults.model = "claude-sonnet-4-20250514".to_string();
        editor.set_config(config);
        
        // Move to Model section
        editor.next_section();
        editor.next_section();
        
        let theme = Theme::default();
        let lines = editor.render_model_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show description for selected model (updated for new model names - TS-010 fix)
        assert!(all_text.contains("Sonnet 4") || all_text.contains("extended thinking"), 
            "Should show model description, got: {}", all_text);
    }

    #[test]
    fn test_model_section_shows_current_marker() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        let mut config = crate::config::LLMConfig::default();
        config.defaults.provider = "anthropic".to_string();
        config.defaults.model = "claude-3-5-sonnet-20241022".to_string(); // Set a specific current model
        editor.set_config(config);
        
        // Move to Model section
        editor.next_section();
        editor.next_section();
        
        let theme = Theme::default();
        let lines = editor.render_model_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should have filled circle for current model
        assert!(all_text.contains("●"), "Should show filled circle for current model");
        // Should have empty circles for other models
        assert!(all_text.contains("○"), "Should show empty circles for non-current models");
    }

    #[test]
    fn test_model_section_empty_state() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        let mut config = crate::config::LLMConfig::default();
        config.defaults.provider = "unknown".to_string(); // Provider with no models
        editor.set_config(config);
        
        // Move to Model section
        editor.next_section();
        editor.next_section();
        
        let theme = Theme::default();
        let lines = editor.render_model_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show empty state message
        assert!(all_text.contains("No models available"), "Should show empty state message");
    }

    #[test]
    fn test_model_catalog_integration() {
        let editor = SettingsEditor::new();
        
        // Model catalog should be populated
        let info = editor.model_catalog.info_for("gpt-4o");
        assert_eq!(info.max_context_tokens, 128_000);
        assert_eq!(info.provider, "openai");
        
        // Check another provider (updated for 2M context - TS-010 fix)
        let info = editor.model_catalog.info_for("gemini-1.5-pro");
        assert_eq!(info.max_context_tokens, 2_000_000);
    }

    // TS-010 Tests: Parameters Section Enhanced UI

    #[test]
    fn test_temperature_adjustment() {
        let mut editor = SettingsEditor::new();
        // Navigate to Parameters section (Tab 3 times: ApiKeys -> Provider -> Model -> Parameters)
        editor.next_section();
        editor.next_section();
        editor.next_section();
        
        // Initial temperature is 0.7
        assert_eq!(editor.config().parameters.temperature, 0.7);
        
        // Increase temperature
        let action = editor.increase_temperature();
        assert!(action.is_some());
        assert_eq!(editor.config().parameters.temperature, 0.8);
        
        // Decrease temperature
        let action = editor.decrease_temperature();
        assert!(action.is_some());
        assert_eq!(editor.config().parameters.temperature, 0.7);
        
        // Decrease below minimum should clamp
        for _ in 0..20 {
            editor.decrease_temperature();
        }
        assert_eq!(editor.config().parameters.temperature, 0.0);
        
        // Increase above maximum should clamp
        for _ in 0..30 {
            editor.increase_temperature();
        }
        assert_eq!(editor.config().parameters.temperature, 2.0);
    }

    #[test]
    fn test_max_tokens_adjustment() {
        let mut editor = SettingsEditor::new();
        // Navigate to Parameters section
        editor.next_section();
        editor.next_section();
        editor.next_section();
        // Select max_tokens (item 1)
        editor.next_item();
        
        // Initial max_tokens is 8192
        assert_eq!(editor.config().parameters.max_tokens, 8192);
        
        // Increase tokens (should use LARGE_STEP since >= 8192)
        let action = editor.increase_max_tokens();
        assert!(action.is_some());
        assert_eq!(editor.config().parameters.max_tokens, 9216); // 8192 + 1024
        
        // Decrease tokens
        let action = editor.decrease_max_tokens();
        assert!(action.is_some());
        assert_eq!(editor.config().parameters.max_tokens, 8192);
        
        // Decrease to small value should use small step
        editor.config.parameters.max_tokens = 512;
        let action = editor.decrease_max_tokens();
        assert!(action.is_some());
        assert_eq!(editor.config().parameters.max_tokens, 256); // 512 - 256
    }

    #[test]
    fn test_adjust_parameter_delegates() {
        let mut editor = SettingsEditor::new();
        // Navigate to Parameters section
        editor.next_section();
        editor.next_section();
        editor.next_section();
        
        // Initially on temperature (item 0)
        assert_eq!(editor.config().parameters.temperature, 0.7);
        
        // Increase via adjust_parameter
        let action = editor.adjust_parameter(true);
        assert!(action.is_some());
        assert_eq!(editor.config().parameters.temperature, 0.8);
        
        // Decrease via adjust_parameter
        let action = editor.adjust_parameter(false);
        assert!(action.is_some());
        assert_eq!(editor.config().parameters.temperature, 0.7);
        
        // Switch to max_tokens
        editor.next_item();
        let initial_tokens = editor.config().parameters.max_tokens;
        
        let action = editor.adjust_parameter(true);
        assert!(action.is_some());
        assert!(editor.config().parameters.max_tokens > initial_tokens);
    }

    #[test]
    fn test_temperature_percentage() {
        let mut editor = SettingsEditor::new();
        
        // 0.0 -> 0%
        editor.config.parameters.temperature = 0.0;
        assert_eq!(editor.temperature_percentage(), 0);
        
        // 1.0 -> 50%
        editor.config.parameters.temperature = 1.0;
        assert_eq!(editor.temperature_percentage(), 50);
        
        // 2.0 -> 100%
        editor.config.parameters.temperature = 2.0;
        assert_eq!(editor.temperature_percentage(), 100);
    }

    #[test]
    fn test_max_tokens_percentage_logarithmic() {
        let mut editor = SettingsEditor::new();
        
        // Very small value -> near 0%
        editor.config.parameters.max_tokens = 1;
        assert!(editor.max_tokens_percentage() < 5);
        
        // Max value -> 100%
        editor.config.parameters.max_tokens = 128_000;
        assert_eq!(editor.max_tokens_percentage(), 100);
        
        // Middle-ish value should be in middle range
        editor.config.parameters.max_tokens = 8192;
        let pct = editor.max_tokens_percentage();
        assert!(pct > 30 && pct < 80, "8192 tokens should be in middle range, got {}", pct);
    }

    #[test]
    fn test_render_slider_bar() {
        // 0% -> all empty
        let bar = SettingsEditor::render_slider_bar(0, 10);
        assert_eq!(bar, "░░░░░░░░░░");
        
        // 100% -> all filled
        let bar = SettingsEditor::render_slider_bar(100, 10);
        assert_eq!(bar, "██████████");
        
        // 50% -> half and half
        let bar = SettingsEditor::render_slider_bar(50, 10);
        assert_eq!(bar, "█████░░░░░");
        
        // 30% with width 20
        let bar = SettingsEditor::render_slider_bar(30, 20);
        assert_eq!(bar, "██████░░░░░░░░░░░░░░");
    }

    #[test]
    fn test_temperature_description() {
        assert_eq!(SettingsEditor::temperature_description(0.0), "Very deterministic - same output each time");
        assert_eq!(SettingsEditor::temperature_description(0.7), "Balanced - good mix of creativity and focus");
        assert_eq!(SettingsEditor::temperature_description(1.5), "Highly creative - quite random");
        assert_eq!(SettingsEditor::temperature_description(2.0), "Maximum randomness - unpredictable");
    }

    #[test]
    fn test_max_tokens_description() {
        assert_eq!(SettingsEditor::max_tokens_description(100), "Very short - quick responses");
        assert_eq!(SettingsEditor::max_tokens_description(512), "Short - concise answers");
        assert_eq!(SettingsEditor::max_tokens_description(2048), "Medium - detailed responses");
        assert_eq!(SettingsEditor::max_tokens_description(8192), "Long - comprehensive output");
        assert_eq!(SettingsEditor::max_tokens_description(16384), "Very long - extensive generation");
        assert_eq!(SettingsEditor::max_tokens_description(64000), "Maximum - full context capacity");
    }

    #[test]
    fn test_format_tokens_display() {
        assert_eq!(SettingsEditor::format_tokens_display(500), "500");
        assert_eq!(SettingsEditor::format_tokens_display(1000), "1K");
        assert_eq!(SettingsEditor::format_tokens_display(8192), "8K");
        assert_eq!(SettingsEditor::format_tokens_display(128000), "128K");
    }

    #[test]
    fn test_parameters_section_shows_slider_and_hint() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        // Navigate to Parameters section
        editor.next_section();
        editor.next_section();
        editor.next_section();
        
        let theme = Theme::default();
        let lines = editor.render_parameters_section(&theme);
        
        // Should have multiple lines (2 params + descriptions + spacing)
        assert!(lines.len() >= 4, "Parameters section should have multiple lines");
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show selector arrow
        assert!(all_text.contains("▸"), "Selected parameter should have arrow selector");
        // Should show slider
        assert!(all_text.contains("█") || all_text.contains("░"), "Should show slider bar");
        // Should show adjustment hint
        assert!(all_text.contains("←/→ adjust"), "Should show adjustment hint");
        // Should show temperature label
        assert!(all_text.contains("Temperature"), "Should show Temperature label");
        // Should show description
        assert!(all_text.contains("Balanced") || all_text.contains("creativity"), "Should show temperature description");
    }

    #[test]
    fn test_parameters_section_max_tokens_selected() {
        use crate::config::Theme;
        
        let mut editor = SettingsEditor::new();
        // Navigate to Parameters section
        editor.next_section();
        editor.next_section();
        editor.next_section();
        // Select max_tokens
        editor.next_item();
        
        let theme = Theme::default();
        let lines = editor.render_parameters_section(&theme);
        
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        
        // Should show Max Tokens label
        assert!(all_text.contains("Max Tokens"), "Should show Max Tokens label");
        // Should show description for tokens
        assert!(all_text.contains("comprehensive") || all_text.contains("Long"), "Should show max tokens description");
        // Should show token value
        assert!(all_text.contains("8K") || all_text.contains("8192"), "Should show token value");
    }

    #[test]
    fn test_parameters_not_adjustable_in_other_sections() {
        let mut editor = SettingsEditor::new();
        // Stay in API Keys section (default)
        
        // Try to adjust temperature - should return None
        let action = editor.adjust_parameter(true);
        assert!(action.is_none(), "Should not adjust when not in Parameters section");
        
        // Temperature should be unchanged
        assert_eq!(editor.config().parameters.temperature, 0.7);
    }
}
