#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusArea {
    #[default]
    Terminal,
    ProcessMonitor,
    Menu,
}

impl FocusArea {
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
