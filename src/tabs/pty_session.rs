//! Per-Tab PTY Session Management
//!
//! Each tab gets its own isolated PTY session with independent:
//! - Shell process (PtyHandle)
//! - Terminal grid + scrollback (Grid)
//! - Input/Output channels
//! - Resize handling
//!
//! This implements TRC-005: PTY Per Tab Isolation

use std::io::{self};
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::components::terminal::TerminalWidget;
use crate::error::{Result, RidgeError};
use crate::event::PtyEvent;
use crate::pty::PtyHandle;
use crate::tabs::TabId;

/// A PTY session tied to a specific tab
pub struct PtySession {
    /// The tab this session belongs to
    tab_id: TabId,
    /// Terminal widget for rendering (owns the Grid)
    pub terminal_widget: TerminalWidget,
    /// Channel to send input to the PTY
    pty_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    /// Channel to send resize commands to the PTY thread
    resize_tx: Option<std_mpsc::Sender<(u16, u16)>>,
    /// Whether the PTY process is alive
    alive: bool,
}

impl PtySession {
    /// Create a new PTY session for a tab (without spawning yet)
    pub fn new(tab_id: TabId, cols: usize, rows: usize) -> Self {
        Self {
            tab_id,
            terminal_widget: TerminalWidget::new(cols, rows),
            pty_tx: None,
            resize_tx: None,
            alive: false,
        }
    }

    /// Get the tab ID this session belongs to
    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    /// Check if the PTY is alive
    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// Spawn the PTY process and start the I/O thread
    /// Returns a receiver for PTY events that should be polled by the app
    pub fn spawn(&mut self, cols: u16, rows: u16) -> Result<mpsc::UnboundedReceiver<(TabId, PtyEvent)>> {
        let pty = PtyHandle::spawn()?;
        pty.resize(cols, rows)?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (write_tx, write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, resize_rx) = std_mpsc::channel::<(u16, u16)>();

        self.pty_tx = Some(write_tx);
        self.resize_tx = Some(resize_tx);
        self.alive = true;

        let tab_id = self.tab_id;

        std::thread::spawn(move || {
            let mut pty = pty;
            let mut buf = [0u8; 4096];
            let mut write_rx = write_rx;

            loop {
                // Check if PTY process has exited
                if let Some(code) = pty.try_wait() {
                    let _ = event_tx.send((tab_id, PtyEvent::Exited(code)));
                    break;
                }

                // Handle resize requests
                while let Ok((cols, rows)) = resize_rx.try_recv() {
                    let _ = pty.resize(cols, rows);
                }

                // Handle write requests
                while let Ok(data) = write_rx.try_recv() {
                    let _ = pty.write(&data);
                }

                // Read PTY output
                match pty.try_read(&mut buf) {
                    Ok(0) => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Ok(n) => {
                        let _ = event_tx.send((tab_id, PtyEvent::Output(buf[..n].to_vec())));
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        let _ = event_tx.send((tab_id, PtyEvent::Error(e)));
                        break;
                    }
                }
            }
        });

        Ok(event_rx)
    }

    /// Send input bytes to the PTY
    pub fn write(&self, data: Vec<u8>) {
        if let Some(ref tx) = self.pty_tx {
            let _ = tx.send(data);
        }
    }

    /// Request PTY resize
    pub fn resize(&mut self, cols: u16, rows: u16) {
        // Resize the terminal widget's grid
        self.terminal_widget.resize(cols as usize, rows as usize);
        // Send resize to PTY process
        if let Some(ref tx) = self.resize_tx {
            let _ = tx.send((cols, rows));
        }
    }

    /// Process PTY output data
    pub fn process_output(&mut self, data: &[u8]) {
        self.terminal_widget.process_output(data);
    }

    /// Mark this session as dead (PTY exited)
    pub fn mark_dead(&mut self) {
        self.alive = false;
    }

    /// Get terminal widget reference for rendering
    pub fn terminal(&self) -> &TerminalWidget {
        &self.terminal_widget
    }

    /// Get mutable terminal widget reference
    pub fn terminal_mut(&mut self) -> &mut TerminalWidget {
        &mut self.terminal_widget
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Dropping the channels will cause the PTY thread to exit
        self.pty_tx = None;
        self.resize_tx = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_session_new() {
        let session = PtySession::new(1, 80, 24);
        assert_eq!(session.tab_id(), 1);
        assert!(!session.is_alive());
        assert_eq!(session.terminal().size(), (80, 24));
    }
}
