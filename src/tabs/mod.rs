//! Tab System for Ridge-Control
//!
//! Manages multiple terminal tabs with independent PTY sessions.
//! Per CONTRACT.md Section 4.3:
//! - Multiple tabs supported
//! - Main tab: "Ridge-Control" with full layout
//! - Additional tabs: User-configurable
//! - Tab creation, closing, renaming
//! - Keyboard navigation between tabs
//!
//! TRC-005: Each tab has its own isolated PTY session

mod pty_session;
mod tab_bar;

pub use pty_session::PtySession;
pub use tab_bar::{TabBar, TabBarStyle};

use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::error::Result;
use crate::event::PtyEvent;

/// Unique identifier for a tab
pub type TabId = u32;

/// A single tab in the tab system
#[derive(Debug, Clone)]
pub struct Tab {
    /// Unique identifier
    id: TabId,
    /// Display name shown in tab bar
    name: String,
    /// Whether this is the main "Ridge-Control" tab (cannot be closed)
    is_main: bool,
    /// When the tab was created
    created_at: Instant,
    /// Whether tab has unsaved changes or activity indicator
    has_activity: bool,
}

impl Tab {
    /// Create a new tab with generated ID
    pub fn new(id: TabId, name: impl Into<String>, is_main: bool) -> Self {
        Self {
            id,
            name: name.into(),
            is_main,
            created_at: Instant::now(),
            has_activity: false,
        }
    }

    /// Create the main "Ridge-Control" tab
    pub fn main() -> Self {
        Self::new(0, "Ridge-Control", true)
    }

    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_main(&self) -> bool {
        self.is_main
    }

    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    pub fn has_activity(&self) -> bool {
        self.has_activity
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn set_activity(&mut self, has_activity: bool) {
        self.has_activity = has_activity;
    }
}

/// Manages all tabs and tracks the active tab
/// Each tab has its own isolated PTY session (TRC-005)
pub struct TabManager {
    /// All tabs in order
    tabs: Vec<Tab>,
    /// Index of currently active tab
    active_index: usize,
    /// Counter for generating unique tab IDs
    next_id: TabId,
    /// PTY sessions indexed by tab ID
    pty_sessions: HashMap<TabId, PtySession>,
    /// Terminal size for new PTY sessions
    terminal_size: (u16, u16),
}

impl std::fmt::Debug for TabManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabManager")
            .field("tabs", &self.tabs)
            .field("active_index", &self.active_index)
            .field("next_id", &self.next_id)
            .field("pty_sessions_count", &self.pty_sessions.len())
            .field("terminal_size", &self.terminal_size)
            .finish()
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TabManager {
    /// Create a new TabManager with the main tab
    pub fn new() -> Self {
        Self {
            tabs: vec![Tab::main()],
            active_index: 0,
            next_id: 1, // 0 is reserved for main tab
            pty_sessions: HashMap::new(),
            terminal_size: (80, 24), // Default, will be set properly on first resize
        }
    }

    /// Set the terminal size for PTY sessions
    pub fn set_terminal_size(&mut self, cols: u16, rows: u16) {
        self.terminal_size = (cols, rows);
        // Resize all existing PTY sessions
        for session in self.pty_sessions.values_mut() {
            session.resize(cols, rows);
        }
    }

    /// Spawn PTY for a tab if not already spawned
    /// Returns a receiver for PTY events from this tab
    pub fn spawn_pty_for_tab(&mut self, tab_id: TabId) -> Result<Option<mpsc::UnboundedReceiver<(TabId, PtyEvent)>>> {
        // Check if session already exists and is alive
        if let Some(session) = self.pty_sessions.get(&tab_id) {
            if session.is_alive() {
                return Ok(None); // Already spawned
            }
        }

        let (cols, rows) = self.terminal_size;
        let mut session = PtySession::new(tab_id, cols as usize, rows as usize);
        let rx = session.spawn(cols, rows)?;
        self.pty_sessions.insert(tab_id, session);
        Ok(Some(rx))
    }

    /// Spawn PTY for the active tab
    pub fn spawn_pty_for_active(&mut self) -> Result<Option<mpsc::UnboundedReceiver<(TabId, PtyEvent)>>> {
        let tab_id = self.active_tab().id();
        self.spawn_pty_for_tab(tab_id)
    }

    /// Get PTY session for a tab
    pub fn get_pty_session(&self, tab_id: TabId) -> Option<&PtySession> {
        self.pty_sessions.get(&tab_id)
    }

    /// Get mutable PTY session for a tab
    pub fn get_pty_session_mut(&mut self, tab_id: TabId) -> Option<&mut PtySession> {
        self.pty_sessions.get_mut(&tab_id)
    }

    /// Get PTY session for the active tab
    pub fn active_pty_session(&self) -> Option<&PtySession> {
        let tab_id = self.active_tab().id();
        self.pty_sessions.get(&tab_id)
    }

    /// Get mutable PTY session for the active tab
    pub fn active_pty_session_mut(&mut self) -> Option<&mut PtySession> {
        let tab_id = self.active_tab().id();
        self.pty_sessions.get_mut(&tab_id)
    }

    /// Write input to the active tab's PTY
    pub fn write_to_active_pty(&self, data: Vec<u8>) {
        let tab_id = self.active_tab().id();
        if let Some(session) = self.pty_sessions.get(&tab_id) {
            session.write(data);
        }
    }

    /// Process PTY output for a specific tab
    pub fn process_pty_output(&mut self, tab_id: TabId, data: &[u8]) {
        if let Some(session) = self.pty_sessions.get_mut(&tab_id) {
            session.process_output(data);
        }
    }

    /// Mark a PTY session as dead
    pub fn mark_pty_dead(&mut self, tab_id: TabId) {
        if let Some(session) = self.pty_sessions.get_mut(&tab_id) {
            session.mark_dead();
        }
    }

    /// Remove PTY session for a tab (called when tab is closed)
    fn remove_pty_session(&mut self, tab_id: TabId) {
        self.pty_sessions.remove(&tab_id);
    }

    /// Get all tabs
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Get the currently active tab
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_index]
    }

    /// Get mutable reference to active tab
    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_index]
    }

    /// Get active tab index
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Get tab count
    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    /// Create a new tab and make it active
    /// Returns the new tab's ID
    pub fn create_tab(&mut self, name: impl Into<String>) -> TabId {
        let id = self.next_id;
        self.next_id += 1;

        let tab = Tab::new(id, name, false);
        self.tabs.push(tab);
        self.active_index = self.tabs.len() - 1;

        id
    }

    /// Create a new tab with default name "Tab N"
    pub fn create_tab_default(&mut self) -> TabId {
        let name = format!("Tab {}", self.next_id);
        self.create_tab(name)
    }

    /// Close a tab by ID
    /// Returns true if tab was closed, false if not found or is main tab
    /// Also cleans up the associated PTY session (TRC-005)
    pub fn close_tab(&mut self, id: TabId) -> bool {
        // Find the tab
        let Some(idx) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };

        // Cannot close main tab
        if self.tabs[idx].is_main {
            return false;
        }

        // Remove the tab and its PTY session
        self.tabs.remove(idx);
        self.remove_pty_session(id);

        // Adjust active index
        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        } else if self.active_index > idx {
            self.active_index -= 1;
        }

        true
    }

    /// Close the active tab (if not main)
    /// Returns true if closed
    pub fn close_active_tab(&mut self) -> bool {
        let id = self.tabs[self.active_index].id;
        self.close_tab(id)
    }

    /// Switch to a specific tab by index
    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_index = index;
        }
    }

    /// Switch to tab by ID
    pub fn select_by_id(&mut self, id: TabId) {
        if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
            self.active_index = idx;
        }
    }

    /// Switch to next tab (wraps around)
    pub fn next_tab(&mut self) {
        self.active_index = (self.active_index + 1) % self.tabs.len();
    }

    /// Switch to previous tab (wraps around)
    pub fn prev_tab(&mut self) {
        if self.active_index == 0 {
            self.active_index = self.tabs.len() - 1;
        } else {
            self.active_index -= 1;
        }
    }

    /// Rename a tab by ID
    pub fn rename_tab(&mut self, id: TabId, name: impl Into<String>) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.set_name(name);
            true
        } else {
            false
        }
    }

    /// Rename the active tab
    pub fn rename_active_tab(&mut self, name: impl Into<String>) {
        self.tabs[self.active_index].set_name(name);
    }

    /// Move a tab to a new position
    pub fn move_tab(&mut self, from_idx: usize, to_idx: usize) {
        if from_idx >= self.tabs.len() || to_idx >= self.tabs.len() {
            return;
        }

        // Don't allow moving the main tab from position 0
        if from_idx == 0 || to_idx == 0 {
            return;
        }

        let tab = self.tabs.remove(from_idx);
        self.tabs.insert(to_idx, tab);

        // Adjust active index
        if self.active_index == from_idx {
            self.active_index = to_idx;
        } else if from_idx < self.active_index && to_idx >= self.active_index {
            self.active_index -= 1;
        } else if from_idx > self.active_index && to_idx <= self.active_index {
            self.active_index += 1;
        }
    }

    /// Set activity indicator on a tab
    pub fn set_tab_activity(&mut self, id: TabId, has_activity: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.set_activity(has_activity);
        }
    }

    /// Clear activity on active tab (typically when user views it)
    pub fn clear_active_activity(&mut self) {
        self.tabs[self.active_index].set_activity(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_manager_new_has_main_tab() {
        let tm = TabManager::new();
        assert_eq!(tm.count(), 1);
        assert!(tm.active_tab().is_main());
        assert_eq!(tm.active_tab().name(), "Ridge-Control");
    }

    #[test]
    fn test_create_tab() {
        let mut tm = TabManager::new();
        let id = tm.create_tab("Test Tab");

        assert_eq!(tm.count(), 2);
        assert_eq!(tm.active_index(), 1); // New tab is active
        assert_eq!(tm.active_tab().name(), "Test Tab");
        assert_eq!(tm.active_tab().id(), id);
        assert!(!tm.active_tab().is_main());
    }

    #[test]
    fn test_close_tab() {
        let mut tm = TabManager::new();
        let id = tm.create_tab("Test Tab");

        assert!(tm.close_tab(id));
        assert_eq!(tm.count(), 1);
        assert_eq!(tm.active_index(), 0);
    }

    #[test]
    fn test_cannot_close_main_tab() {
        let mut tm = TabManager::new();
        let main_id = tm.active_tab().id();

        assert!(!tm.close_tab(main_id));
        assert_eq!(tm.count(), 1);
    }

    #[test]
    fn test_tab_navigation() {
        let mut tm = TabManager::new();
        tm.create_tab("Tab 1");
        tm.create_tab("Tab 2");
        tm.create_tab("Tab 3");

        // Now at Tab 3 (index 3)
        assert_eq!(tm.active_index(), 3);

        tm.prev_tab();
        assert_eq!(tm.active_index(), 2);

        tm.select(0);
        assert_eq!(tm.active_index(), 0);

        tm.next_tab();
        assert_eq!(tm.active_index(), 1);
    }

    #[test]
    fn test_wrap_around_navigation() {
        let mut tm = TabManager::new();
        tm.create_tab("Tab 1");
        // 2 tabs total

        tm.select(1); // Last tab
        tm.next_tab();
        assert_eq!(tm.active_index(), 0); // Wrapped to first

        tm.prev_tab();
        assert_eq!(tm.active_index(), 1); // Wrapped to last
    }

    #[test]
    fn test_rename_tab() {
        let mut tm = TabManager::new();
        let id = tm.create_tab("Old Name");

        tm.rename_tab(id, "New Name");
        assert_eq!(tm.active_tab().name(), "New Name");
    }

    #[test]
    fn test_close_adjusts_active_index() {
        let mut tm = TabManager::new();
        tm.create_tab("Tab 1");
        tm.create_tab("Tab 2");
        tm.create_tab("Tab 3");

        // At Tab 3 (index 3)
        tm.select(1); // Go to Tab 1

        // Close Tab 2 (index 2)
        let tab2_id = tm.tabs()[2].id();
        tm.close_tab(tab2_id);

        // Active index should still be 1 (Tab 1)
        assert_eq!(tm.active_index(), 1);
        assert_eq!(tm.count(), 3);
    }
}
