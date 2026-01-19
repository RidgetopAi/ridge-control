use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use super::types::ActivityMessage;

pub struct ActivityStore {
    activities: VecDeque<ActivityMessage>,
    capacity: usize,
    run_filter: Option<String>,
    /// Maps tool_id -> tool_name for looking up names in ToolResults
    tool_names: HashMap<String, String>,
    /// Current run instance info (instance_number, total_instances)
    current_instance: Option<(u32, u32)>,
}

impl ActivityStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            activities: VecDeque::with_capacity(capacity),
            capacity,
            run_filter: None,
            tool_names: HashMap::new(),
            current_instance: None,
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(1000)
    }

    pub fn push(&mut self, activity: ActivityMessage) {
        // Track tool_id -> tool_name mapping from ToolCalls
        if let ActivityMessage::ToolCall(tc) = &activity {
            self.tool_names.insert(tc.tool_id.clone(), tc.tool_name.clone());
        }

        // Update current instance info from session
        if let Some(session) = activity.session() {
            self.current_instance = Some((session.instance_number, session.total_instances));
        }

        if self.activities.len() >= self.capacity {
            self.activities.pop_front();
        }
        self.activities.push_back(activity);
    }

    /// Look up tool name by tool_id (for ToolResults)
    pub fn get_tool_name(&self, tool_id: &str) -> Option<&str> {
        self.tool_names.get(tool_id).map(|s| s.as_str())
    }

    /// Get current run instance info
    pub fn current_instance(&self) -> Option<(u32, u32)> {
        self.current_instance
    }

    pub fn get_visible(&self, offset: usize, count: usize) -> Vec<&ActivityMessage> {
        let filtered: Vec<_> = self
            .activities
            .iter()
            .filter(|a| {
                if let Some(ref filter) = self.run_filter {
                    a.session()
                        .map(|s| &s.run_name == filter)
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .collect();

        filtered
            .into_iter()
            .skip(offset)
            .take(count)
            .collect()
    }

    pub fn get_all(&self) -> Vec<&ActivityMessage> {
        self.activities.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.activities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.activities.is_empty()
    }

    pub fn clear(&mut self) {
        self.activities.clear();
        self.tool_names.clear();
        self.current_instance = None;
    }

    pub fn set_run_filter(&mut self, run_name: Option<String>) {
        self.run_filter = run_name;
    }

    pub fn run_filter(&self) -> Option<&str> {
        self.run_filter.as_deref()
    }

    pub fn filtered_len(&self) -> usize {
        if let Some(ref filter) = self.run_filter {
            self.activities
                .iter()
                .filter(|a| {
                    a.session()
                        .map(|s| &s.run_name == filter)
                        .unwrap_or(false)
                })
                .count()
        } else {
            self.activities.len()
        }
    }
}

pub type SharedActivityStore = Arc<Mutex<ActivityStore>>;

pub fn new_shared_store(capacity: usize) -> SharedActivityStore {
    Arc::new(Mutex::new(ActivityStore::new(capacity)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spindles::types::{SirkSession, ThinkingActivity};

    fn make_activity(content: &str, run_name: Option<&str>) -> ActivityMessage {
        ActivityMessage::Thinking(ThinkingActivity {
            content: content.to_string(),
            timestamp: "2026-01-17T12:00:00Z".to_string(),
            session: run_name.map(|rn| SirkSession {
                run_name: rn.to_string(),
                instance_number: 1,
                total_instances: 5,
                project: "test".to_string(),
            }),
        })
    }

    #[test]
    fn test_push_and_len() {
        let mut store = ActivityStore::new(10);
        assert!(store.is_empty());

        store.push(make_activity("first", None));
        assert_eq!(store.len(), 1);

        store.push(make_activity("second", None));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let mut store = ActivityStore::new(3);

        store.push(make_activity("a", None));
        store.push(make_activity("b", None));
        store.push(make_activity("c", None));
        assert_eq!(store.len(), 3);

        store.push(make_activity("d", None));
        assert_eq!(store.len(), 3);

        let visible = store.get_visible(0, 10);
        assert_eq!(visible.len(), 3);
        
        match &visible[0] {
            ActivityMessage::Thinking(a) => assert_eq!(a.content, "b"),
            _ => panic!("Expected Thinking"),
        }
    }

    #[test]
    fn test_get_visible_with_offset() {
        let mut store = ActivityStore::new(10);
        for i in 0..5 {
            store.push(make_activity(&format!("item-{}", i), None));
        }

        let visible = store.get_visible(2, 2);
        assert_eq!(visible.len(), 2);
        
        match &visible[0] {
            ActivityMessage::Thinking(a) => assert_eq!(a.content, "item-2"),
            _ => panic!("Expected Thinking"),
        }
    }

    #[test]
    fn test_run_filter() {
        let mut store = ActivityStore::new(10);
        store.push(make_activity("run-a-1", Some("run-a")));
        store.push(make_activity("run-b-1", Some("run-b")));
        store.push(make_activity("run-a-2", Some("run-a")));
        store.push(make_activity("no-session", None));

        assert_eq!(store.len(), 4);

        store.set_run_filter(Some("run-a".to_string()));
        assert_eq!(store.filtered_len(), 2);

        let visible = store.get_visible(0, 10);
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn test_clear() {
        let mut store = ActivityStore::new(10);
        store.push(make_activity("test", None));
        assert!(!store.is_empty());

        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn test_shared_store() {
        let store = new_shared_store(10);
        {
            let mut guard = store.lock().unwrap();
            guard.push(make_activity("shared", None));
        }
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard.len(), 1);
        }
    }
}
