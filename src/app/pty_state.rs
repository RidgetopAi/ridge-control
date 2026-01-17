// PtyState - Extracted PTY/terminal-related state from App struct (Order 8.3)
// Contains terminal, tab management, and PTY event receivers

use std::io::{self, Stdout};

use crossterm::{
    event::{EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{enable_raw_mode, EnterAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};
use tokio::sync::mpsc;

use crate::error::{Result, RidgeError};
use crate::event::PtyEvent;
use crate::tabs::{TabId, TabManager};

pub struct PtyState {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    pub tab_manager: TabManager,
    pub pty_receivers: Vec<mpsc::UnboundedReceiver<(TabId, PtyEvent)>>,
}

impl PtyState {
    pub fn new(term_cols: u16, term_rows: u16) -> Result<Self> {
        enable_raw_mode().map_err(|e| RidgeError::Terminal(e.to_string()))?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)
            .map_err(|e| RidgeError::Terminal(e.to_string()))?;

        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).map_err(|e| RidgeError::Terminal(e.to_string()))?;

        let mut tab_manager = TabManager::new();
        tab_manager.set_terminal_size(term_cols, term_rows);

        Ok(Self {
            terminal,
            tab_manager,
            pty_receivers: Vec::new(),
        })
    }

    /// Calculate terminal size from layout area
    pub fn calculate_terminal_size(area: Rect) -> (u16, u16) {
        let terminal_width = (area.width * 2 / 3).saturating_sub(2);
        let terminal_height = area.height.saturating_sub(2);
        (terminal_width, terminal_height)
    }

    /// Spawn PTY for a tab and register the receiver
    pub fn spawn_pty_for_tab(&mut self, tab_id: TabId) -> Result<()> {
        if let Some(rx) = self.tab_manager.spawn_pty_for_tab(tab_id)? {
            self.pty_receivers.push(rx);
        }
        Ok(())
    }

    /// Spawn PTY for the main/active tab
    pub fn spawn_main_pty(&mut self) -> Result<()> {
        let main_tab_id = self.tab_manager.active_tab().id();
        self.spawn_pty_for_tab(main_tab_id)
    }
}
