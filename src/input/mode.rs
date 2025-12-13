// Input modes - some variants for future mode types
#![allow(dead_code)]

/// Input mode state machine per RIDGE-CONTROL-MASTER.md Section 1.3
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputMode {
    /// All input → PTY (Ctrl+Esc exits)
    PtyRaw,
    /// Vim-style navigation mode
    #[default]
    Normal,
    /// Text input for filters/rename
    Insert { target: InsertTarget },
    /// Fuzzy command search (nucleo integration)
    CommandPalette,
    /// Confirmation dialog mode (e.g., for tool execution)
    Confirm { title: String, message: String },
}

/// Target for Insert mode text input
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertTarget {
    ProcessFilter,
    StreamFilter,
    TabRename,
    Search,
}

impl InputMode {
    pub fn is_pty_raw(&self) -> bool {
        matches!(self, InputMode::PtyRaw)
    }

    pub fn is_normal(&self) -> bool {
        matches!(self, InputMode::Normal)
    }
    
    pub fn is_insert(&self) -> bool {
        matches!(self, InputMode::Insert { .. })
    }
    
    pub fn is_command_palette(&self) -> bool {
        matches!(self, InputMode::CommandPalette)
    }
    
    pub fn is_confirm(&self) -> bool {
        matches!(self, InputMode::Confirm { .. })
    }
}
