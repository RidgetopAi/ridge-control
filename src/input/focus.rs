/// Focus areas per RIDGE-CONTROL-MASTER.md Section 1.4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusArea {
    /// Left 2/3 - Terminal emulator
    #[default]
    Terminal,
    /// Top-right - Process monitor
    ProcessMonitor,
    /// Bottom-right - Menu/stream list
    Menu,
    /// When a stream is actively being viewed (overlay)
    StreamViewer,
    /// When config panel is open
    ConfigPanel,
    /// When log viewer is open (overlay) - TRC-013
    LogViewer,
    /// When chat input is focused (in conversation view)
    ChatInput,
    /// When settings editor is open (overlay) - TS-012
    SettingsEditor,
}

impl FocusArea {
    /// Focus ring for Tab cycling (excludes overlay areas)
    /// ChatInput is conditionally included when conversation is visible
    pub const RING: &'static [FocusArea] = &[
        FocusArea::Terminal,
        FocusArea::ProcessMonitor,
        FocusArea::Menu,
        FocusArea::ChatInput,
    ];

    /// Get next focus area, optionally skipping ChatInput
    pub fn next_with_skip(&self, skip_chat: bool) -> FocusArea {
        let idx = Self::RING.iter().position(|f| f == self).unwrap_or(0);
        let mut next_idx = (idx + 1) % Self::RING.len();
        
        if skip_chat && Self::RING[next_idx] == FocusArea::ChatInput {
            next_idx = (next_idx + 1) % Self::RING.len();
        }
        
        Self::RING[next_idx]
    }

    /// Get previous focus area, optionally skipping ChatInput
    pub fn prev_with_skip(&self, skip_chat: bool) -> FocusArea {
        let idx = Self::RING.iter().position(|f| f == self).unwrap_or(0);
        let ring_len = Self::RING.len();
        let mut prev_idx = if idx == 0 { ring_len - 1 } else { idx - 1 };
        
        if skip_chat && Self::RING[prev_idx] == FocusArea::ChatInput {
            prev_idx = if prev_idx == 0 { ring_len - 1 } else { prev_idx - 1 };
        }
        
        Self::RING[prev_idx]
    }

    #[allow(dead_code)]
    pub fn next(&self) -> FocusArea {
        self.next_with_skip(false)
    }

    #[allow(dead_code)]
    pub fn prev(&self) -> FocusArea {
        self.prev_with_skip(false)
    }
}

#[derive(Debug, Default, Clone)]
pub struct FocusManager {
    pub current: FocusArea,
}

impl FocusManager {
    pub fn new() -> Self {
        Self {
            current: FocusArea::Terminal,
        }
    }

    pub fn current(&self) -> FocusArea {
        self.current
    }

    pub fn focus(&mut self, area: FocusArea) {
        self.current = area;
    }

    #[allow(dead_code)]
    pub fn next(&mut self) {
        self.current = self.current.next();
    }

    #[allow(dead_code)]
    pub fn prev(&mut self) {
        self.current = self.current.prev();
    }

    pub fn next_skip_chat(&mut self, skip_chat: bool) {
        self.current = self.current.next_with_skip(skip_chat);
    }

    pub fn prev_skip_chat(&mut self, skip_chat: bool) {
        self.current = self.current.prev_with_skip(skip_chat);
    }

    pub fn is_focused(&self, area: FocusArea) -> bool {
        self.current == area
    }
}
