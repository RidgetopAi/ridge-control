pub mod grid;

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::process::Child;

use pty_process::blocking::{Command, Pty};

use crate::error::{Result, RidgeError};

pub struct PtyHandle {
    pty: Pty,
    child: Child,
}

impl PtyHandle {
    pub fn spawn() -> Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        let pty = Pty::new().map_err(|e| RidgeError::Pty(e.to_string()))?;

        let pts = pty.pts().map_err(|e| RidgeError::Pty(e.to_string()))?;

        let child = Command::new(&shell)
            .spawn(&pts)
            .map_err(|e| RidgeError::Pty(e.to_string()))?;

        Ok(Self { pty, child })
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.pty
            .resize(pty_process::Size::new(rows, cols))
            .map_err(|e| RidgeError::Pty(e.to_string()))
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.pty.write_all(data)?;
        Ok(())
    }

    pub fn try_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.pty.read(buf)
    }

    pub fn raw_fd(&self) -> RawFd {
        self.pty.as_raw_fd()
    }

    pub fn try_wait(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(status)) => status.code(),
            _ => None,
        }
    }

    pub fn is_alive(&mut self) -> bool {
        self.try_wait().is_none()
    }
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
