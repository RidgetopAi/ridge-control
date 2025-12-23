//! Agent thread management - conversation persistence and storage

use std::collections::HashMap;
use std::sync::RwLock;

use uuid::Uuid;

use serde::{Deserialize, Serialize};

use super::context::ContextSegment;

/// An agent conversation thread
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentThread {
    /// Unique thread identifier
    pub id: String,
    /// Human-readable title
    pub title: String,
    /// Model used for this thread
    pub model: String,
    /// Context segments in order
    pub segments: Vec<ContextSegment>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last activity timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Sequence counter for ordering segments
    next_sequence: u64,
    /// Arbitrary metadata
    pub metadata: HashMap<String, String>,
}

impl AgentThread {
    pub fn new(model: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: format!("T-{}", Uuid::new_v4()),
            title: "New conversation".to_string(),
            model: model.into(),
            segments: Vec::new(),
            created_at: now,
            updated_at: now,
            next_sequence: 0,
            metadata: HashMap::new(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Add a segment and return its sequence number
    pub fn add_segment(&mut self, mut segment: ContextSegment) -> u64 {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        segment.sequence = seq;
        self.segments.push(segment);
        self.updated_at = chrono::Utc::now();
        seq
    }

    /// Get all segments
    pub fn segments(&self) -> &[ContextSegment] {
        &self.segments
    }

    /// Get the next sequence number without incrementing
    pub fn peek_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Clear all segments
    pub fn clear(&mut self) {
        self.segments.clear();
        self.next_sequence = 0;
        self.updated_at = chrono::Utc::now();
    }

    /// Update the model
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
        self.updated_at = chrono::Utc::now();
    }

    /// Update the thread title
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
        self.updated_at = chrono::Utc::now();
    }
}

/// Trait for thread storage backends
pub trait ThreadStore: Send + Sync {
    /// Get a thread by ID
    fn get(&self, id: &str) -> Option<AgentThread>;

    /// Save or update a thread
    fn save(&self, thread: &AgentThread) -> Result<(), String>;

    /// Delete a thread
    fn delete(&self, id: &str) -> Result<(), String>;

    /// List all thread IDs
    fn list(&self) -> Vec<String>;

    /// List threads with basic info (id, title, updated_at)
    fn list_summary(&self) -> Vec<ThreadSummary>;
}

/// Summary info for thread listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub model: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub segment_count: usize,
}

/// In-memory thread store (for development/testing)
pub struct InMemoryThreadStore {
    threads: RwLock<HashMap<String, AgentThread>>,
}

impl InMemoryThreadStore {
    pub fn new() -> Self {
        Self {
            threads: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryThreadStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadStore for InMemoryThreadStore {
    fn get(&self, id: &str) -> Option<AgentThread> {
        self.threads.read().ok()?.get(id).cloned()
    }

    fn save(&self, thread: &AgentThread) -> Result<(), String> {
        let mut threads = self
            .threads
            .write()
            .map_err(|e| format!("Lock poisoned: {}", e))?;
        threads.insert(thread.id.clone(), thread.clone());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let mut threads = self
            .threads
            .write()
            .map_err(|e| format!("Lock poisoned: {}", e))?;
        threads.remove(id);
        Ok(())
    }

    fn list(&self) -> Vec<String> {
        self.threads
            .read()
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn list_summary(&self) -> Vec<ThreadSummary> {
        self.threads
            .read()
            .map(|threads| {
                let mut summaries: Vec<_> = threads
                    .values()
                    .map(|t| ThreadSummary {
                        id: t.id.clone(),
                        title: t.title.clone(),
                        model: t.model.clone(),
                        updated_at: t.updated_at,
                        segment_count: t.segments.len(),
                    })
                    .collect();
                // Sort by most recent first
                summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                summaries
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::context::SegmentKind;
    use crate::llm::types::Message;

    #[test]
    fn test_thread_creation() {
        let thread = AgentThread::new("gpt-4o").with_title("Test Thread");
        assert!(thread.id.starts_with("T-"));
        assert_eq!(thread.title, "Test Thread");
        assert_eq!(thread.model, "gpt-4o");
    }

    #[test]
    fn test_thread_segments() {
        let mut thread = AgentThread::new("claude-sonnet-4-20250514");
        let segment = ContextSegment::new(
            SegmentKind::ChatHistory,
            vec![Message::user("Hello")],
            0,
        );
        let seq = thread.add_segment(segment);
        assert_eq!(seq, 0);
        assert_eq!(thread.segments.len(), 1);
    }

    #[test]
    fn test_in_memory_store() {
        let store = InMemoryThreadStore::new();
        let thread = AgentThread::new("gpt-4o").with_title("Test");

        store.save(&thread).unwrap();
        assert_eq!(store.list().len(), 1);

        let retrieved = store.get(&thread.id).unwrap();
        assert_eq!(retrieved.title, "Test");

        store.delete(&thread.id).unwrap();
        assert!(store.get(&thread.id).is_none());
    }

    #[test]
    fn test_set_title() {
        let mut thread = AgentThread::new("gpt-4o");
        assert_eq!(thread.title, "New conversation");

        let old_updated = thread.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(10));

        thread.set_title("My Custom Thread");
        assert_eq!(thread.title, "My Custom Thread");
        assert!(thread.updated_at > old_updated);
    }
}
