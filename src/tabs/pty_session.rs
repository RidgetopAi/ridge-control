// PTY session - some methods for future use

#![allow(dead_code)]

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
use std::os::unix::io::RawFd;
use std::sync::mpsc as std_mpsc;

use tokio::sync::mpsc;

use crate::components::terminal::TerminalWidget;
use crate::error::Result;
use crate::event::PtyEvent;
use crate::pty::{MouseMode, PtyHandle};
use crate::tabs::TabId;

/// Create a Linux eventfd for cross-thread signaling.
/// Returns the fd, or -1 on failure.
fn create_eventfd() -> RawFd {
    unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) }
}

/// Signal an eventfd by writing a 1 to it.
fn signal_eventfd(fd: RawFd) {
    let val: u64 = 1;
    unsafe {
        libc::write(fd, &val as *const u64 as *const libc::c_void, 8);
    }
}

/// Drain an eventfd (read to clear the signal).
fn drain_eventfd(fd: RawFd) {
    let mut val: u64 = 0;
    unsafe {
        libc::read(fd, &mut val as *mut u64 as *mut libc::c_void, 8);
    }
}

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
    /// Eventfd to wake PTY thread instantly when writes arrive
    wake_fd: Option<RawFd>,
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
            wake_fd: None,
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

        // Create eventfd for instant PTY thread wake on writes.
        // Without this, the PTY thread sleeps in poll() for up to 10ms
        // before noticing a write arrived, adding latency to every keystroke.
        let wake_fd = create_eventfd();
        if wake_fd < 0 {
            tracing::warn!("Failed to create eventfd, falling back to poll-only PTY thread");
        }
        let thread_wake_fd = wake_fd; // Copy for the thread

        self.pty_tx = Some(write_tx);
        self.resize_tx = Some(resize_tx);
        self.wake_fd = if wake_fd >= 0 { Some(wake_fd) } else { None };
        self.alive = true;

        let tab_id = self.tab_id;

        std::thread::spawn(move || {
            let mut pty = pty;
            let mut buf = [0u8; 16384]; // 16KB read buffer
            let mut write_rx = write_rx;
            let pty_fd = pty.raw_fd();
            let has_wake = thread_wake_fd >= 0;

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

                // Poll for PTY output AND wake signal simultaneously.
                // When the event loop sends a keystroke, it signals the eventfd,
                // which wakes us from poll() instantly instead of waiting 10ms.
                if has_wake {
                    let mut fds = [
                        libc::pollfd { fd: pty_fd, events: libc::POLLIN, revents: 0 },
                        libc::pollfd { fd: thread_wake_fd, events: libc::POLLIN, revents: 0 },
                    ];
                    let poll_result = unsafe { libc::poll(fds.as_mut_ptr(), 2, 10) };

                    if poll_result > 0 {
                        // Wake signal: drain eventfd, then drain write channel
                        if fds[1].revents & libc::POLLIN != 0 {
                            drain_eventfd(thread_wake_fd);
                            while let Ok(data) = write_rx.try_recv() {
                                let _ = pty.write(&data);
                            }
                        }

                        // PTY data ready
                        if fds[0].revents & libc::POLLIN != 0 {
                            match pty.try_read(&mut buf) {
                                Ok(0) => {} // EOF
                                Ok(n) => {
                                    let _ = event_tx.send((tab_id, PtyEvent::Output(buf[..n].to_vec())));
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                                Err(e) => {
                                    let _ = event_tx.send((tab_id, PtyEvent::Error(e)));
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    // Fallback: poll only PTY fd (original behavior)
                    let mut pollfd = libc::pollfd {
                        fd: pty_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let poll_result = unsafe { libc::poll(&mut pollfd, 1, 10) };

                    if poll_result > 0 && (pollfd.revents & libc::POLLIN) != 0 {
                        match pty.try_read(&mut buf) {
                            Ok(0) => {}
                            Ok(n) => {
                                let _ = event_tx.send((tab_id, PtyEvent::Output(buf[..n].to_vec())));
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                            Err(e) => {
                                let _ = event_tx.send((tab_id, PtyEvent::Error(e)));
                                break;
                            }
                        }
                    }
                }
            }

            // Clean up eventfd
            if has_wake {
                unsafe { libc::close(thread_wake_fd); }
            }
        });

        Ok(event_rx)
    }

    /// Send input bytes to the PTY.
    /// Signals the PTY thread's eventfd so it wakes from poll() instantly.
    pub fn write(&self, data: Vec<u8>) {
        if let Some(ref tx) = self.pty_tx {
            let _ = tx.send(data);
            // Wake PTY thread immediately — don't wait for 10ms poll timeout
            if let Some(fd) = self.wake_fd {
                signal_eventfd(fd);
            }
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

    /// Get the mouse tracking mode enabled by the nested application
    pub fn mouse_mode(&self) -> MouseMode {
        self.terminal_widget.mouse_mode()
    }

    /// Check if we're in alternate screen mode (running a TUI)
    pub fn is_alternate_screen(&self) -> bool {
        self.terminal_widget.is_alternate_screen()
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Dropping the channels will cause the PTY thread to exit
        self.pty_tx = None;
        self.resize_tx = None;
        // Close eventfd
        if let Some(fd) = self.wake_fd.take() {
            unsafe { libc::close(fd); }
        }
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

    #[test]
    fn test_eventfd_create_signal_drain() {
        let fd = create_eventfd();
        assert!(fd >= 0, "eventfd creation failed");
        signal_eventfd(fd);
        drain_eventfd(fd);
        unsafe { libc::close(fd); }
    }
}
