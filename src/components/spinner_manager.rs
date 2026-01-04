// TRC-015: Spinner manager - some key variants for future use

use std::collections::HashMap;

use ratatui::style::Color;

use super::spinner::{Spinner, SpinnerStyle};
use crate::config::Theme;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum SpinnerKey {
    LlmLoading,
    StreamConnecting(String),
    ToolExecuting(String),
    Custom(String),
}

#[allow(dead_code)]
impl SpinnerKey {
    pub fn stream_connecting(name: impl Into<String>) -> Self {
        Self::StreamConnecting(name.into())
    }
    
    pub fn tool_executing(name: impl Into<String>) -> Self {
        Self::ToolExecuting(name.into())
    }
    
    pub fn custom(name: impl Into<String>) -> Self {
        Self::Custom(name.into())
    }
}

pub struct SpinnerManager {
    spinners: HashMap<SpinnerKey, Spinner>,
    default_style: SpinnerStyle,
}

impl Default for SpinnerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl SpinnerManager {
    pub fn new() -> Self {
        Self {
            spinners: HashMap::new(),
            default_style: SpinnerStyle::Braille,
        }
    }
    
    pub fn with_default_style(mut self, style: SpinnerStyle) -> Self {
        self.default_style = style;
        self
    }
    
    pub fn set_default_style(&mut self, style: SpinnerStyle) {
        self.default_style = style;
    }
    
    pub fn start(&mut self, key: SpinnerKey, label: Option<String>) {
        let mut spinner = Spinner::new(self.default_style);
        if let Some(l) = label {
            spinner = spinner.with_label(l);
        }
        spinner.set_active(true);
        self.spinners.insert(key, spinner);
    }
    
    pub fn start_with_style(&mut self, key: SpinnerKey, style: SpinnerStyle, label: Option<String>) {
        let mut spinner = Spinner::new(style);
        if let Some(l) = label {
            spinner = spinner.with_label(l);
        }
        spinner.set_active(true);
        self.spinners.insert(key, spinner);
    }
    
    pub fn stop(&mut self, key: &SpinnerKey) {
        self.spinners.remove(key);
    }
    
    pub fn set_label(&mut self, key: &SpinnerKey, label: Option<String>) {
        if let Some(spinner) = self.spinners.get_mut(key) {
            spinner.set_label(label);
        }
    }
    
    pub fn set_color(&mut self, key: &SpinnerKey, color: Color) {
        if let Some(spinner) = self.spinners.get_mut(key) {
            spinner.set_color(color);
        }
    }
    
    pub fn get(&self, key: &SpinnerKey) -> Option<&Spinner> {
        self.spinners.get(key)
    }
    
    pub fn get_mut(&mut self, key: &SpinnerKey) -> Option<&mut Spinner> {
        self.spinners.get_mut(key)
    }
    
    pub fn is_active(&self, key: &SpinnerKey) -> bool {
        self.spinners.contains_key(key)
    }
    
    pub fn tick(&mut self) {
        for spinner in self.spinners.values_mut() {
            spinner.tick();
        }
    }
    
    pub fn active_count(&self) -> usize {
        self.spinners.len()
    }
    
    pub fn clear(&mut self) {
        self.spinners.clear();
    }
    
    pub fn apply_theme(&mut self, theme: &Theme) {
        if let Some(style) = SpinnerStyle::from_name(&theme.spinner.default_style) {
            self.default_style = style;
        }
        
        let color = theme.spinner.loading_color.to_color();
        for spinner in self.spinners.values_mut() {
            spinner.set_color(color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_manager_new() {
        let manager = SpinnerManager::new();
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_spinner_manager_start_stop() {
        let mut manager = SpinnerManager::new();
        
        manager.start(SpinnerKey::LlmLoading, Some("Loading...".to_string()));
        assert!(manager.is_active(&SpinnerKey::LlmLoading));
        assert_eq!(manager.active_count(), 1);
        
        manager.stop(&SpinnerKey::LlmLoading);
        assert!(!manager.is_active(&SpinnerKey::LlmLoading));
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_spinner_manager_multiple() {
        let mut manager = SpinnerManager::new();
        
        manager.start(SpinnerKey::LlmLoading, None);
        manager.start(SpinnerKey::stream_connecting("logs"), Some("Connecting...".to_string()));
        manager.start(SpinnerKey::tool_executing("bash"), None);
        
        assert_eq!(manager.active_count(), 3);
        
        manager.stop(&SpinnerKey::stream_connecting("logs"));
        assert_eq!(manager.active_count(), 2);
    }

    #[test]
    fn test_spinner_manager_set_label() {
        let mut manager = SpinnerManager::new();
        
        manager.start(SpinnerKey::LlmLoading, Some("Loading...".to_string()));
        manager.set_label(&SpinnerKey::LlmLoading, Some("Almost done...".to_string()));
        
        let spinner = manager.get(&SpinnerKey::LlmLoading).unwrap();
        assert!(spinner.is_active());
    }

    #[test]
    fn test_spinner_manager_tick() {
        let mut manager = SpinnerManager::new();
        manager.start(SpinnerKey::LlmLoading, None);
        
        manager.tick();
    }

    #[test]
    fn test_spinner_manager_clear() {
        let mut manager = SpinnerManager::new();
        
        manager.start(SpinnerKey::LlmLoading, None);
        manager.start(SpinnerKey::stream_connecting("logs"), None);
        
        manager.clear();
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_spinner_key_variants() {
        let key1 = SpinnerKey::LlmLoading;
        let key2 = SpinnerKey::stream_connecting("test");
        let key3 = SpinnerKey::tool_executing("bash");
        let key4 = SpinnerKey::custom("my-spinner");
        
        assert_ne!(key1, key2);
        assert_ne!(key2, key3);
        assert_ne!(key3, key4);
    }
}
