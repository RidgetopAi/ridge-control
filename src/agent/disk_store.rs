//! Disk-based thread storage using JSON files
//!
//! Stores threads as JSON files in ~/.config/ridge-control/threads/{thread_id}.json

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::RwLock;

use super::thread::{AgentThread, ThreadStore, ThreadSummary};

/// File-based thread persistence using JSON
pub struct DiskThreadStore {
    /// Base directory for thread files
    base_path: PathBuf,
    /// Cache of loaded threads for performance
    cache: RwLock<HashMap<String, AgentThread>>,
}

impl DiskThreadStore {
    /// Create a new DiskThreadStore with the default path
    ///
    /// Default: ~/.config/ridge-control/threads/
    pub fn new() -> Result<Self, String> {
        let base_path = Self::default_path()?;
        Self::with_path(base_path)
    }

    /// Create a DiskThreadStore with a custom path
    pub fn with_path(base_path: PathBuf) -> Result<Self, String> {
        // Create directory if it doesn't exist
        if !base_path.exists() {
            fs::create_dir_all(&base_path)
                .map_err(|e| format!("Failed to create threads directory: {}", e))?;
        }

        Ok(Self {
            base_path,
            cache: RwLock::new(HashMap::new()),
        })
    }

    /// Get the default storage path (~/.config/ridge-control/threads/)
    pub fn default_path() -> Result<PathBuf, String> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| "Could not determine config directory".to_string())?;
        Ok(config_dir.join("ridge-control").join("threads"))
    }

    /// Get the file path for a thread ID
    pub fn thread_path(&self, id: &str) -> PathBuf {
        // Sanitize thread ID to prevent directory traversal
        let safe_id = id
            .replace("..", "_")
            .replace(['/', '\\', '\0'], "_");
        self.base_path.join(format!("{}.json", safe_id))
    }

    /// Get the base path
    #[allow(dead_code)]
    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
    }

    /// Load a thread from disk into cache
    fn load_from_disk(&self, id: &str) -> Result<AgentThread, String> {
        let path = self.thread_path(id);
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read thread file: {}", e))?;
        let thread: AgentThread = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse thread JSON: {}", e))?;
        Ok(thread)
    }

    /// Atomic write: write to temp file then rename
    fn atomic_write(&self, id: &str, thread: &AgentThread) -> Result<(), String> {
        let target_path = self.thread_path(id);
        let temp_path = self.base_path.join(format!(".{}.tmp", id.replace(['/', '\\', '\0'], "_")));

        // Serialize to JSON with pretty formatting
        let json = serde_json::to_string_pretty(thread)
            .map_err(|e| format!("Failed to serialize thread: {}", e))?;

        // Write to temp file
        {
            let mut file = fs::File::create(&temp_path)
                .map_err(|e| format!("Failed to create temp file: {}", e))?;
            file.write_all(json.as_bytes())
                .map_err(|e| format!("Failed to write temp file: {}", e))?;
            file.sync_all()
                .map_err(|e| format!("Failed to sync temp file: {}", e))?;
        }

        // Atomic rename
        fs::rename(&temp_path, &target_path)
            .map_err(|e| format!("Failed to rename temp file: {}", e))?;

        Ok(())
    }
}

impl Default for DiskThreadStore {
    fn default() -> Self {
        Self::new().expect("Failed to create default DiskThreadStore")
    }
}

impl ThreadStore for DiskThreadStore {
    fn get(&self, id: &str) -> Option<AgentThread> {
        // Check cache first
        if let Ok(cache) = self.cache.read() {
            if let Some(thread) = cache.get(id) {
                return Some(thread.clone());
            }
        }

        // Try to load from disk
        match self.load_from_disk(id) {
            Ok(thread) => {
                // Update cache
                if let Ok(mut cache) = self.cache.write() {
                    cache.insert(id.to_string(), thread.clone());
                }
                Some(thread)
            }
            Err(_) => None,
        }
    }

    fn save(&self, thread: &AgentThread) -> Result<(), String> {
        // Atomic write to disk
        self.atomic_write(&thread.id, thread)?;

        // Update cache
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(thread.id.clone(), thread.clone());
        }

        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.thread_path(id);

        // Remove from disk if exists
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete thread file: {}", e))?;
        }

        // Remove from cache
        if let Ok(mut cache) = self.cache.write() {
            cache.remove(id);
        }

        Ok(())
    }

    fn list(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.base_path) else {
            return Vec::new();
        };

        entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                
                // Skip temp files and non-JSON files
                let file_name = path.file_name()?.to_str()?;
                if file_name.starts_with('.') || !file_name.ends_with(".json") {
                    return None;
                }

                // Extract thread ID (remove .json extension)
                let id = file_name.strip_suffix(".json")?;
                Some(id.to_string())
            })
            .collect()
    }

    fn list_summary(&self) -> Vec<ThreadSummary> {
        let ids = self.list();
        let mut summaries: Vec<ThreadSummary> = ids
            .iter()
            .filter_map(|id| {
                let thread = self.get(id)?;
                Some(ThreadSummary {
                    id: thread.id.clone(),
                    title: thread.title.clone(),
                    model: thread.model.clone(),
                    updated_at: thread.updated_at,
                    segment_count: thread.segments.len(),
                })
            })
            .collect();

        // Sort by most recent first
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        summaries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::context::{ContextSegment, SegmentKind};
    use crate::llm::types::Message;
    use tempfile::TempDir;

    fn create_test_store() -> (DiskThreadStore, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let store = DiskThreadStore::with_path(temp_dir.path().to_path_buf())
            .expect("Failed to create store");
        (store, temp_dir)
    }

    #[test]
    fn test_disk_store_save_load() {
        let (store, _temp_dir) = create_test_store();
        
        let mut thread = AgentThread::new("gpt-4o").with_title("Test Thread");
        let segment = ContextSegment::new(
            SegmentKind::ChatHistory,
            vec![Message::user("Hello, world!")],
            0,
        );
        thread.add_segment(segment);

        // Save
        store.save(&thread).expect("Failed to save");

        // Verify file exists
        let path = store.thread_path(&thread.id);
        assert!(path.exists(), "Thread file should exist");

        // Clear cache and reload
        store.cache.write().unwrap().clear();
        
        // Load
        let loaded = store.get(&thread.id).expect("Failed to load thread");
        assert_eq!(loaded.id, thread.id);
        assert_eq!(loaded.title, "Test Thread");
        assert_eq!(loaded.model, "gpt-4o");
        assert_eq!(loaded.segments.len(), 1);
    }

    #[test]
    fn test_disk_store_list() {
        let (store, _temp_dir) = create_test_store();

        // Create multiple threads
        let thread1 = AgentThread::new("gpt-4o").with_title("Thread 1");
        let thread2 = AgentThread::new("claude-sonnet-4-20250514").with_title("Thread 2");
        let thread3 = AgentThread::new("gemini-pro").with_title("Thread 3");

        store.save(&thread1).unwrap();
        store.save(&thread2).unwrap();
        store.save(&thread3).unwrap();

        let ids = store.list();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&thread1.id));
        assert!(ids.contains(&thread2.id));
        assert!(ids.contains(&thread3.id));
    }

    #[test]
    fn test_disk_store_list_summary() {
        let (store, _temp_dir) = create_test_store();

        let thread1 = AgentThread::new("gpt-4o").with_title("Thread 1");
        let mut thread2 = AgentThread::new("claude-sonnet-4-20250514").with_title("Thread 2");
        
        // Add segment to thread2 to test segment_count
        thread2.add_segment(ContextSegment::new(
            SegmentKind::ChatHistory,
            vec![Message::user("Test message")],
            0,
        ));

        store.save(&thread1).unwrap();
        store.save(&thread2).unwrap();

        let summaries = store.list_summary();
        assert_eq!(summaries.len(), 2);

        // Check thread2 has segment counted
        let thread2_summary = summaries.iter().find(|s| s.id == thread2.id).unwrap();
        assert_eq!(thread2_summary.segment_count, 1);
        assert_eq!(thread2_summary.title, "Thread 2");
    }

    #[test]
    fn test_disk_store_delete() {
        let (store, _temp_dir) = create_test_store();

        let thread = AgentThread::new("gpt-4o").with_title("Delete Me");
        store.save(&thread).unwrap();

        // Verify exists
        assert!(store.get(&thread.id).is_some());
        assert!(store.thread_path(&thread.id).exists());

        // Delete
        store.delete(&thread.id).unwrap();

        // Verify gone
        assert!(store.get(&thread.id).is_none());
        assert!(!store.thread_path(&thread.id).exists());
    }

    #[test]
    fn test_disk_store_atomic_write() {
        let (store, _temp_dir) = create_test_store();

        let thread = AgentThread::new("gpt-4o").with_title("Atomic Test");
        store.save(&thread).unwrap();

        // Verify no temp files left behind
        let entries: Vec<_> = fs::read_dir(store.base_path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        
        for entry in &entries {
            let name = entry.file_name();
            let name_str = name.to_str().unwrap();
            assert!(!name_str.starts_with('.'), "Temp file left behind: {}", name_str);
        }
    }

    #[test]
    fn test_disk_store_cache() {
        let (store, _temp_dir) = create_test_store();

        let thread = AgentThread::new("gpt-4o").with_title("Cache Test");
        store.save(&thread).unwrap();

        // First get - should load from disk and cache
        let loaded1 = store.get(&thread.id).unwrap();
        assert!(store.cache.read().unwrap().contains_key(&thread.id));

        // Second get - should use cache
        let loaded2 = store.get(&thread.id).unwrap();
        assert_eq!(loaded1.id, loaded2.id);
    }

    #[test]
    fn test_disk_store_path_sanitization() {
        let (store, _temp_dir) = create_test_store();

        // Path traversal attempts should be sanitized
        let path1 = store.thread_path("../malicious");
        assert!(!path1.to_str().unwrap().contains(".."));

        let path2 = store.thread_path("test/nested");
        assert!(!path2.to_str().unwrap().contains("/test/"));
    }
}
