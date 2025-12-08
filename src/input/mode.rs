#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputMode {
    PtyRaw,
    #[default]
    Normal,
    /// Confirmation dialog mode (e.g., for tool execution)
    Confirm {
        title: String,
        message: String,
    },
}

impl InputMode {
    pub fn is_pty_raw(&self) -> bool {
        matches!(self, InputMode::PtyRaw)
    }

    pub fn is_normal(&self) -> bool {
        matches!(self, InputMode::Normal)
    }
    
    pub fn is_confirm(&self) -> bool {
        matches!(self, InputMode::Confirm { .. })
    }
}
