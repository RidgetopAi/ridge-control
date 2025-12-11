mod keybindings;
mod theme;
mod watcher;

pub use keybindings::{KeyBinding, KeybindingsConfig, Modifier};
pub use theme::{Theme, ThemeColors, FocusStyle};
pub use watcher::{ConfigWatcher, ConfigWatcherMode, ConfigEvent};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use directories::BaseDirs;

use crate::error::{RidgeError, Result};

const CONFIG_DIR: &str = "ridge-control";
const MAIN_CONFIG_FILE: &str = "config.toml";
const KEYBINDINGS_FILE: &str = "keybindings.toml";
const THEME_FILE: &str = "theme.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub terminal: TerminalConfig,
    pub process_monitor: ProcessMonitorConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            terminal: TerminalConfig::default(),
            process_monitor: ProcessMonitorConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub tick_interval_ms: u64,
    pub log_level: String,
    pub log_file: Option<PathBuf>,
    pub watch_config: bool,
    pub config_watch_debounce_ms: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 500,
            log_level: "info".to_string(),
            log_file: None,
            watch_config: true,
            config_watch_debounce_ms: 2000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub scrollback_lines: usize,
    pub shell: Option<String>,
    pub shell_args: Vec<String>,
    pub term_env: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 10000,
            shell: None,
            shell_args: vec![],
            term_env: "xterm-256color".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessMonitorConfig {
    pub refresh_interval_ms: u64,
    pub max_processes: usize,
    pub show_threads: bool,
    pub cpu_threshold_warn: f32,
    pub cpu_threshold_critical: f32,
}

impl Default for ProcessMonitorConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: 2000,
            max_processes: 50,
            show_threads: false,
            cpu_threshold_warn: 70.0,
            cpu_threshold_critical: 90.0,
        }
    }
}

pub struct ConfigManager {
    config_dir: PathBuf,
    app_config: AppConfig,
    keybindings: KeybindingsConfig,
    theme: Theme,
}

impl ConfigManager {
    pub fn new() -> Result<Self> {
        let config_dir = Self::get_config_dir()?;
        
        let app_config = Self::load_app_config(&config_dir);
        let keybindings = Self::load_keybindings(&config_dir);
        let theme = Self::load_theme(&config_dir);
        
        Ok(Self {
            config_dir,
            app_config,
            keybindings,
            theme,
        })
    }
    
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }
    
    pub fn app_config(&self) -> &AppConfig {
        &self.app_config
    }
    
    pub fn keybindings(&self) -> &KeybindingsConfig {
        &self.keybindings
    }
    
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
    
    pub fn reload_all(&mut self) {
        self.app_config = Self::load_app_config(&self.config_dir);
        self.keybindings = Self::load_keybindings(&self.config_dir);
        self.theme = Self::load_theme(&self.config_dir);
    }
    
    pub fn reload_file(&mut self, path: &Path) {
        let file_name = path.file_name().and_then(|n| n.to_str());
        
        match file_name {
            Some(MAIN_CONFIG_FILE) => {
                self.app_config = Self::load_app_config(&self.config_dir);
            }
            Some(KEYBINDINGS_FILE) => {
                self.keybindings = Self::load_keybindings(&self.config_dir);
            }
            Some(THEME_FILE) => {
                self.theme = Self::load_theme(&self.config_dir);
            }
            _ => {
                self.reload_all();
            }
        }
    }
    
    fn get_config_dir() -> Result<PathBuf> {
        BaseDirs::new()
            .map(|dirs| dirs.config_dir().join(CONFIG_DIR))
            .ok_or_else(|| RidgeError::Config("Could not determine config directory".to_string()))
    }
    
    fn load_app_config(config_dir: &Path) -> AppConfig {
        let path = config_dir.join(MAIN_CONFIG_FILE);
        Self::load_toml_file(&path).unwrap_or_default()
    }
    
    fn load_keybindings(config_dir: &Path) -> KeybindingsConfig {
        let path = config_dir.join(KEYBINDINGS_FILE);
        Self::load_toml_file(&path).unwrap_or_default()
    }
    
    fn load_theme(config_dir: &Path) -> Theme {
        let path = config_dir.join(THEME_FILE);
        Self::load_toml_file(&path).unwrap_or_default()
    }
    
    fn load_toml_file<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Option<T> {
        if !path.exists() {
            return None;
        }
        
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => Some(config),
                Err(e) => {
                    tracing::warn!("Failed to parse {}: {}", path.display(), e);
                    None
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read {}: {}", path.display(), e);
                None
            }
        }
    }
    
    pub fn ensure_config_dir(&self) -> Result<()> {
        if !self.config_dir.exists() {
            std::fs::create_dir_all(&self.config_dir)
                .map_err(|e| RidgeError::Config(format!("Failed to create config dir: {}", e)))?;
        }
        Ok(())
    }
    
    pub fn write_default_configs(&self) -> Result<()> {
        self.ensure_config_dir()?;
        
        let main_path = self.config_dir.join(MAIN_CONFIG_FILE);
        if !main_path.exists() {
            let content = toml::to_string_pretty(&AppConfig::default())
                .map_err(|e| RidgeError::Config(format!("Failed to serialize config: {}", e)))?;
            std::fs::write(&main_path, content)
                .map_err(|e| RidgeError::Config(format!("Failed to write config: {}", e)))?;
        }
        
        let keybindings_path = self.config_dir.join(KEYBINDINGS_FILE);
        if !keybindings_path.exists() {
            let content = toml::to_string_pretty(&KeybindingsConfig::default())
                .map_err(|e| RidgeError::Config(format!("Failed to serialize keybindings: {}", e)))?;
            std::fs::write(&keybindings_path, content)
                .map_err(|e| RidgeError::Config(format!("Failed to write keybindings: {}", e)))?;
        }
        
        let theme_path = self.config_dir.join(THEME_FILE);
        if !theme_path.exists() {
            let content = toml::to_string_pretty(&Theme::default())
                .map_err(|e| RidgeError::Config(format!("Failed to serialize theme: {}", e)))?;
            std::fs::write(&theme_path, content)
                .map_err(|e| RidgeError::Config(format!("Failed to write theme: {}", e)))?;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_app_config() {
        let config = AppConfig::default();
        assert_eq!(config.general.tick_interval_ms, 500);
        assert!(config.general.watch_config);
    }
    
    #[test]
    fn test_app_config_serialization() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.general.tick_interval_ms, config.general.tick_interval_ms);
    }
}
