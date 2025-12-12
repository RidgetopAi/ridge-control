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
}

impl FocusArea {
    /// Focus ring for Tab cycling (excludes overlay areas)
    pub const RING: &'static [FocusArea] = &[
        FocusArea::Terminal,
        FocusArea::ProcessMonitor,
        FocusArea::Menu,
    ];

    pub fn next(&self) -> FocusArea {
        let idx = Self::RING.iter().position(|f| f == self).unwrap_or(0);
        Self::RING[(idx + 1) % Self::RING.len()]
    }

    pub fn prev(&self) -> FocusArea {
        let idx = Self::RING.iter().position(|f| f == self).unwrap_or(0);
        if idx == 0 {
            Self::RING[Self::RING.len() - 1]
        } else {
            Self::RING[idx - 1]
        }
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

    pub fn next(&mut self) {
        self.current = self.current.next();
    }

    pub fn prev(&mut self) {
        self.current = self.current.prev();
    }

    pub fn is_focused(&self, area: FocusArea) -> bool {
        self.current == area
    }
}
