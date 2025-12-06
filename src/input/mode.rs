#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    PtyRaw,
    #[default]
    Normal,
}

impl InputMode {
    pub fn is_pty_raw(&self) -> bool {
        matches!(self, InputMode::PtyRaw)
    }

    pub fn is_normal(&self) -> bool {
        matches!(self, InputMode::Normal)
    }
}
