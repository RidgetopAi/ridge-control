// Event types - scaffolding for async event system
#![allow(dead_code)]

use std::path::PathBuf;
use crossterm::event::Event as CrosstermEvent;

/// Main event enum per RIDGE-CONTROL-MASTER.md Section 3.1
#[derive(Debug)]
pub enum Event {
    /// Input from crossterm (keyboard, mouse, paste)
    Input(CrosstermEvent),
    /// PTY terminal events
    Pty(PtyEvent),
    /// Stream events (WebSocket, SSE, etc.)
    Stream(StreamEvent),
    /// Periodic tick for refresh
    Tick,
    /// Terminal resize
    Resize { cols: u16, rows: u16 },
    /// Configuration file changed
    ConfigChanged(PathBuf),
    /// Async error from background task
    Error(AsyncError),
}

/// PTY-specific events
#[derive(Debug)]
pub enum PtyEvent {
    /// Raw output bytes from PTY
    Output(Vec<u8>),
    /// PTY process exited with code
    Exited(i32),
    /// I/O error occurred
    Error(std::io::Error),
}

/// Stream-related events per MASTER.md
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Connected(String),
    Disconnected(String, Option<String>),
    Data(String, Vec<u8>),
    Error(String, String),
}

/// Async error wrapper for background task errors
#[derive(Debug)]
pub enum AsyncError {
    Network(String),
    Parse(String),
    Timeout,
    Cancelled,
}
