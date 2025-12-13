use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{RidgeError, Result};

#[derive(Debug, Clone)]
pub enum ConfigEvent {
    Changed(PathBuf),
    Error(String),
}

pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<ConfigEvent>,
}

impl ConfigWatcher {
    pub fn new(config_dir: &Path, debounce_ms: u64) -> Result<Self> {
        let (tx, rx) = channel::<ConfigEvent>();
        
        let watcher = Self::setup_watcher(config_dir, tx, debounce_ms)?;
        
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }
    
    fn setup_watcher(
        config_dir: &Path,
        tx: Sender<ConfigEvent>,
        _debounce_ms: u64,
    ) -> Result<RecommendedWatcher> {
        let tx_clone = tx.clone();
        
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            match result {
                Ok(event) => {
                    if event.kind.is_modify() || event.kind.is_create() {
                        for path in event.paths {
                            if Self::is_config_file(&path) {
                                let _ = tx_clone.send(ConfigEvent::Changed(path));
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx_clone.send(ConfigEvent::Error(e.to_string()));
                }
            }
        })
        .map_err(|e| RidgeError::Config(format!("Failed to create watcher: {}", e)))?;
        
        if config_dir.exists() {
            watcher
                .watch(config_dir, RecursiveMode::NonRecursive)
                .map_err(|e| RidgeError::Config(format!("Failed to watch config dir: {}", e)))?;
        }
        
        Ok(watcher)
    }
    
    fn is_config_file(path: &Path) -> bool {
        let extension = path.extension().and_then(|e| e.to_str());
        matches!(extension, Some("toml") | Some("yaml") | Some("yml"))
    }
    
    pub fn try_recv(&self) -> Option<ConfigEvent> {
        self.rx.try_recv().ok()
    }
    
    pub fn poll_events(&self) -> Vec<ConfigEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.try_recv() {
            events.push(event);
        }
        events
    }
}

pub struct TickBasedWatcher {
    config_dir: PathBuf,
    last_check: std::time::Instant,
    check_interval: Duration,
    file_mtimes: std::collections::HashMap<PathBuf, std::time::SystemTime>,
}

impl TickBasedWatcher {
    pub fn new(config_dir: PathBuf, check_interval_ms: u64) -> Self {
        let mut watcher = Self {
            config_dir,
            last_check: std::time::Instant::now(),
            check_interval: Duration::from_millis(check_interval_ms),
            file_mtimes: std::collections::HashMap::new(),
        };
        watcher.scan_files();
        watcher
    }
    
    fn scan_files(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.config_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if ConfigWatcher::is_config_file(&path) {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if let Ok(mtime) = metadata.modified() {
                            self.file_mtimes.insert(path, mtime);
                        }
                    }
                }
            }
        }
    }
    
    pub fn check(&mut self) -> Vec<ConfigEvent> {
        if self.last_check.elapsed() < self.check_interval {
            return Vec::new();
        }
        
        self.last_check = std::time::Instant::now();
        let mut events = Vec::new();
        
        if let Ok(entries) = std::fs::read_dir(&self.config_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if ConfigWatcher::is_config_file(&path) {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if let Ok(mtime) = metadata.modified() {
                            let changed = self.file_mtimes
                                .get(&path)
                                .map(|&old_mtime| mtime != old_mtime)
                                .unwrap_or(true);
                            
                            if changed {
                                self.file_mtimes.insert(path.clone(), mtime);
                                events.push(ConfigEvent::Changed(path));
                            }
                        }
                    }
                }
            }
        }
        
        events
    }
}

pub enum ConfigWatcherMode {
    Notify(ConfigWatcher),
    Tick(TickBasedWatcher),
}

impl ConfigWatcherMode {
    pub fn notify(config_dir: &Path, debounce_ms: u64) -> Result<Self> {
        Ok(Self::Notify(ConfigWatcher::new(config_dir, debounce_ms)?))
    }
    
    pub fn tick(config_dir: PathBuf, check_interval_ms: u64) -> Self {
        Self::Tick(TickBasedWatcher::new(config_dir, check_interval_ms))
    }
    
    pub fn poll_events(&mut self) -> Vec<ConfigEvent> {
        match self {
            Self::Notify(watcher) => watcher.poll_events(),
            Self::Tick(watcher) => watcher.check(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;
    
    #[test]
    fn test_is_config_file() {
        assert!(ConfigWatcher::is_config_file(Path::new("config.toml")));
        assert!(ConfigWatcher::is_config_file(Path::new("theme.yaml")));
        assert!(ConfigWatcher::is_config_file(Path::new("keys.yml")));
        assert!(!ConfigWatcher::is_config_file(Path::new("script.sh")));
        assert!(!ConfigWatcher::is_config_file(Path::new("data.json")));
    }
    
    #[test]
    fn test_tick_based_watcher() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test.toml");
        fs::write(&config_path, "key = \"value\"").unwrap();
        
        let mut watcher = TickBasedWatcher::new(temp_dir.path().to_path_buf(), 0);
        
        let events = watcher.check();
        assert!(events.is_empty());
        
        std::thread::sleep(Duration::from_millis(10));
        fs::write(&config_path, "key = \"new_value\"").unwrap();
        
        let events = watcher.check();
        assert_eq!(events.len(), 1);
        match &events[0] {
            ConfigEvent::Changed(path) => assert_eq!(path, &config_path),
            _ => panic!("Expected Changed event"),
        }
    }
}
