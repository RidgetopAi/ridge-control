// Error types - some variants for future error conditions

#![allow(dead_code)]

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RidgeError {
    #[error("PTY error: {0}")]
    Pty(String),

    #[error("PTY spawn failed: {0}")]
    PtySpawn(#[from] std::io::Error),

    #[error("Terminal initialization failed: {0}")]
    Terminal(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Configuration file not found: {path}")]
    ConfigNotFound { path: PathBuf },

    #[error("Focus error: {0}")]
    Focus(String),

    #[error("Event channel closed")]
    ChannelClosed,
}

pub type Result<T> = std::result::Result<T, RidgeError>;
