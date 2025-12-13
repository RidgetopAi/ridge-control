// TRC-012: Session persistence - fully implemented but not yet triggered
#![allow(dead_code)]

//! Session Persistence for Ridge-Control
//!
//! Implements TRC-012: Save/restore tabs on startup
//!
//! Per CONTRACT.md Section 5:
//! "Session persistence (restore tabs/layout on restart)"
//!
//! Saves:
//! - Tab names and their order
//! - Active tab index
//! - Optional: Working directories per tab (future)
//!
//! Location: ~/.config/ridge-control/session.toml

use std::path::{Path, PathBuf};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::error::{RidgeError, Result};

const SESSION_FILE: &str = "session.toml";

/// Persistent session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// Version for forward compatibility
    #[serde(default = "default_version")]
    pub version: u32,
    /// Tabs in order (main tab is always first)
    pub tabs: Vec<TabData>,
    /// Index of the active tab (0-based)
    pub active_tab_index: usize,
    /// Timestamp when session was saved (Unix epoch seconds)
    #[serde(default)]
    pub saved_at: u64,
}

fn default_version() -> u32 {
    1
}

/// Data for a single tab
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabData {
    /// Tab display name
    pub name: String,
    /// Whether this is the main "Ridge-Control" tab
    #[serde(default)]
    pub is_main: bool,
    /// Optional working directory for this tab's shell
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

impl Default for SessionData {
    fn default() -> Self {
        Self {
            version: 1,
            tabs: vec![TabData {
                name: "Ridge-Control".to_string(),
                is_main: true,
                working_dir: None,
            }],
            active_tab_index: 0,
            saved_at: 0,
        }
    }
}

impl SessionData {
    /// Create session data from current tabs
    pub fn from_tabs<'a>(
        tabs: impl Iterator<Item = (&'a str, bool)>,
        active_index: usize,
    ) -> Self {
        let tabs: Vec<TabData> = tabs
            .map(|(name, is_main)| TabData {
                name: name.to_string(),
                is_main,
                working_dir: None,
            })
            .collect();

        Self {
            version: 1,
            tabs,
            active_tab_index: active_index,
            saved_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    /// Check if this is a valid session (has at least the main tab)
    pub fn is_valid(&self) -> bool {
        !self.tabs.is_empty() && self.tabs.iter().any(|t| t.is_main)
    }
}

/// Manages session persistence
pub struct SessionManager {
    session_path: PathBuf,
}

impl SessionManager {
    /// Create a new SessionManager using the default config directory
    pub fn new() -> Result<Self> {
        let config_dir = BaseDirs::new()
            .map(|dirs| dirs.config_dir().join("ridge-control"))
            .ok_or_else(|| RidgeError::Config("Could not determine config directory".to_string()))?;

        Ok(Self {
            session_path: config_dir.join(SESSION_FILE),
        })
    }

    /// Create a SessionManager with a custom path (for testing)
    pub fn with_path(path: PathBuf) -> Self {
        Self { session_path: path }
    }

    /// Get the session file path
    pub fn session_path(&self) -> &Path {
        &self.session_path
    }

    /// Load session data from disk
    /// Returns default session if file doesn't exist or is invalid
    pub fn load(&self) -> SessionData {
        if !self.session_path.exists() {
            tracing::debug!("No session file found, using default");
            return SessionData::default();
        }

        match std::fs::read_to_string(&self.session_path) {
            Ok(content) => match toml::from_str::<SessionData>(&content) {
                Ok(session) => {
                    if session.is_valid() {
                        tracing::info!(
                            "Loaded session with {} tabs, active: {}",
                            session.tabs.len(),
                            session.active_tab_index
                        );
                        session
                    } else {
                        tracing::warn!("Session file invalid, using default");
                        SessionData::default()
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse session file: {}", e);
                    SessionData::default()
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read session file: {}", e);
                SessionData::default()
            }
        }
    }

    /// Save session data to disk
    pub fn save(&self, session: &SessionData) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.session_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    RidgeError::Config(format!("Failed to create config dir: {}", e))
                })?;
            }
        }

        let content = toml::to_string_pretty(session)
            .map_err(|e| RidgeError::Config(format!("Failed to serialize session: {}", e)))?;

        std::fs::write(&self.session_path, content)
            .map_err(|e| RidgeError::Config(format!("Failed to write session file: {}", e)))?;

        tracing::info!(
            "Saved session with {} tabs to {}",
            session.tabs.len(),
            self.session_path.display()
        );

        Ok(())
    }

    /// Delete the session file (for clean starts)
    pub fn clear(&self) -> Result<()> {
        if self.session_path.exists() {
            std::fs::remove_file(&self.session_path)
                .map_err(|e| RidgeError::Config(format!("Failed to remove session file: {}", e)))?;
            tracing::info!("Cleared session file");
        }
        Ok(())
    }

    /// Check if a session file exists
    pub fn exists(&self) -> bool {
        self.session_path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_session_manager() -> (SessionManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join(SESSION_FILE);
        let manager = SessionManager::with_path(session_path);
        (manager, temp_dir)
    }

    #[test]
    fn test_default_session() {
        let session = SessionData::default();
        assert_eq!(session.tabs.len(), 1);
        assert!(session.tabs[0].is_main);
        assert_eq!(session.tabs[0].name, "Ridge-Control");
        assert_eq!(session.active_tab_index, 0);
        assert!(session.is_valid());
    }

    #[test]
    fn test_session_from_tabs() {
        let tabs = vec![
            ("Ridge-Control", true),
            ("Dev", false),
            ("Build", false),
        ];
        let session = SessionData::from_tabs(tabs.iter().map(|(n, m)| (*n, *m)), 1);

        assert_eq!(session.tabs.len(), 3);
        assert_eq!(session.tabs[0].name, "Ridge-Control");
        assert!(session.tabs[0].is_main);
        assert_eq!(session.tabs[1].name, "Dev");
        assert!(!session.tabs[1].is_main);
        assert_eq!(session.active_tab_index, 1);
        assert!(session.is_valid());
    }

    #[test]
    fn test_session_save_and_load() {
        let (manager, _temp_dir) = temp_session_manager();

        let tabs = vec![
            ("Ridge-Control", true),
            ("Test Tab", false),
        ];
        let session = SessionData::from_tabs(tabs.iter().map(|(n, m)| (*n, *m)), 1);

        // Save
        manager.save(&session).unwrap();
        assert!(manager.exists());

        // Load
        let loaded = manager.load();
        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.tabs[0].name, "Ridge-Control");
        assert_eq!(loaded.tabs[1].name, "Test Tab");
        assert_eq!(loaded.active_tab_index, 1);
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let (manager, _temp_dir) = temp_session_manager();

        let session = manager.load();
        assert_eq!(session.tabs.len(), 1);
        assert!(session.tabs[0].is_main);
    }

    #[test]
    fn test_clear_session() {
        let (manager, _temp_dir) = temp_session_manager();

        // Create a session file
        let session = SessionData::default();
        manager.save(&session).unwrap();
        assert!(manager.exists());

        // Clear it
        manager.clear().unwrap();
        assert!(!manager.exists());
    }

    #[test]
    fn test_invalid_session_returns_default() {
        let (manager, _temp_dir) = temp_session_manager();

        // Write invalid session (empty tabs)
        let invalid = SessionData {
            version: 1,
            tabs: vec![],
            active_tab_index: 0,
            saved_at: 0,
        };
        
        let content = toml::to_string_pretty(&invalid).unwrap();
        std::fs::write(manager.session_path(), content).unwrap();

        // Load should return default
        let loaded = manager.load();
        assert!(loaded.is_valid());
        assert!(loaded.tabs[0].is_main);
    }

    #[test]
    fn test_session_serialization_format() {
        let tabs = vec![
            ("Ridge-Control", true),
            ("Dev", false),
        ];
        let session = SessionData::from_tabs(tabs.iter().map(|(n, m)| (*n, *m)), 0);

        let toml_str = toml::to_string_pretty(&session).unwrap();
        
        // Verify key fields are present
        assert!(toml_str.contains("version = 1"));
        assert!(toml_str.contains("active_tab_index = 0"));
        assert!(toml_str.contains("[[tabs]]"));
        assert!(toml_str.contains("name = \"Ridge-Control\""));
        assert!(toml_str.contains("is_main = true"));
    }

    #[test]
    fn test_corrupted_session_file() {
        let (manager, _temp_dir) = temp_session_manager();

        // Write garbage
        std::fs::write(manager.session_path(), "not valid toml {{{").unwrap();

        // Should return default
        let loaded = manager.load();
        assert!(loaded.is_valid());
    }
}
