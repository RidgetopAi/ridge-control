//! ForgeController - Subprocess management for Forge runner
//!
//! Spawns and controls the Forge subprocess, parsing stdout JSONL events
//! and providing control methods for stop/kill.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;

use tokio::process::ChildStdin;

use super::types::{ForgeConfig, ForgeEvent, ForgeResumeResponse, StderrLineEvent};

/// Default timeout for graceful shutdown (seconds)
const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;

/// Path to forge project (relative to home)
const FORGE_PROJECT_PATH: &str = "projects/forge";

/// Connection state for the Forge subprocess
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeConnectionState {
    /// Not running
    Disconnected,
    /// Starting up
    Connecting,
    /// Running and producing events
    Connected,
    /// Shutting down
    Disconnecting,
    /// Crashed or failed
    Failed,
}

/// ForgeController manages the Forge subprocess lifecycle
pub struct ForgeController {
    /// Child process handle
    child: Option<Child>,
    /// Current connection state
    state: Arc<Mutex<ForgeConnectionState>>,
    /// Event sender for parsed ForgeEvents
    event_tx: Option<mpsc::UnboundedSender<ForgeEvent>>,
    /// Last error message
    last_error: Arc<Mutex<Option<String>>>,
    /// Current run configuration
    config: Option<ForgeConfig>,
    /// Stdin handle for sending resume responses
    stdin: Arc<Mutex<Option<ChildStdin>>>,
}

impl ForgeController {
    /// Create a new ForgeController
    pub fn new() -> Self {
        Self {
            child: None,
            state: Arc::new(Mutex::new(ForgeConnectionState::Disconnected)),
            event_tx: None,
            last_error: Arc::new(Mutex::new(None)),
            config: None,
            stdin: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new ForgeController with an event channel
    pub fn with_event_channel(event_tx: mpsc::UnboundedSender<ForgeEvent>) -> Self {
        Self {
            child: None,
            state: Arc::new(Mutex::new(ForgeConnectionState::Disconnected)),
            event_tx: Some(event_tx),
            last_error: Arc::new(Mutex::new(None)),
            config: None,
            stdin: Arc::new(Mutex::new(None)),
        }
    }

    /// Get current connection state
    pub async fn state(&self) -> ForgeConnectionState {
        *self.state.lock().await
    }

    /// Check if Forge is currently running
    pub async fn is_running(&self) -> bool {
        matches!(
            *self.state.lock().await,
            ForgeConnectionState::Connected | ForgeConnectionState::Connecting
        )
    }

    /// Get the last error message
    pub async fn last_error(&self) -> Option<String> {
        self.last_error.lock().await.clone()
    }

    /// Get current configuration
    pub fn config(&self) -> Option<&ForgeConfig> {
        self.config.as_ref()
    }

    /// Send a resume response to Forge (used after receiving ResumePrompt event)
    ///
    /// The response tells Forge whether to continue the run or abort.
    pub async fn send_resume_response(
        &self,
        response: ForgeResumeResponse,
    ) -> Result<(), ForgeControllerError> {
        let mut stdin_guard = self.stdin.lock().await;
        if let Some(ref mut stdin) = *stdin_guard {
            let json = serde_json::to_string(&response)
                .map_err(|e| ForgeControllerError::ConfigSerializationFailed(e.to_string()))?;
            stdin
                .write_all(json.as_bytes())
                .await
                .map_err(|e| ForgeControllerError::StdinWriteFailed(e.to_string()))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| ForgeControllerError::StdinWriteFailed(e.to_string()))?;
            stdin
                .flush()
                .await
                .map_err(|e| ForgeControllerError::StdinWriteFailed(e.to_string()))?;
            Ok(())
        } else {
            Err(ForgeControllerError::StdinUnavailable)
        }
    }

    /// Spawn the Forge subprocess with the given configuration
    ///
    /// Returns an event receiver channel for ForgeEvents parsed from stdout.
    /// Stderr is logged but not returned.
    pub async fn spawn(
        &mut self,
        config: ForgeConfig,
    ) -> Result<mpsc::UnboundedReceiver<ForgeEvent>, ForgeControllerError> {
        // Check if already running
        if self.is_running().await {
            return Err(ForgeControllerError::AlreadyRunning);
        }

        *self.state.lock().await = ForgeConnectionState::Connecting;
        *self.last_error.lock().await = None;

        // Resolve forge project path
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/home/ridgetop".to_string());
        let forge_path = format!("{}/{}", home_dir, FORGE_PROJECT_PATH);

        // Serialize config to JSON for stdin
        let config_json = serde_json::to_string(&config)
            .map_err(|e| ForgeControllerError::ConfigSerializationFailed(e.to_string()))?;

        // Spawn forge using npm start
        let mut child = Command::new("npm")
            .arg("start")
            .current_dir(&forge_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ForgeControllerError::SpawnFailed(e.to_string()))?;

        // Write config to stdin (with newline to signal end of config JSON)
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(config_json.as_bytes())
                .await
                .map_err(|e| ForgeControllerError::StdinWriteFailed(e.to_string()))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| ForgeControllerError::StdinWriteFailed(e.to_string()))?;
            stdin
                .flush()
                .await
                .map_err(|e| ForgeControllerError::StdinWriteFailed(e.to_string()))?;
            // Keep stdin open for resume responses
            *self.stdin.lock().await = Some(stdin);
        }

        // Create event channel
        let (tx, rx) = mpsc::unbounded_channel();

        // Take stdout for event parsing
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ForgeControllerError::StdoutUnavailable)?;

        // Take stderr for logging
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ForgeControllerError::StderrUnavailable)?;

        // Store the child process
        self.child = Some(child);
        self.config = Some(config);

        // Clone state for async tasks
        let state = self.state.clone();
        let last_error = self.last_error.clone();
        let external_tx = self.event_tx.clone();

        // Spawn stdout reader task
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();

                // Skip empty lines and npm startup output (not JSON events)
                if trimmed.is_empty()
                    || trimmed.starts_with('>')  // npm script output like "> forge@0.1.0 start"
                    || trimmed.starts_with("npm ")  // npm messages
                {
                    continue;
                }

                // Try to parse as ForgeEvent
                match serde_json::from_str::<ForgeEvent>(&line) {
                    Ok(event) => {
                        // Send to internal channel
                        let _ = tx_clone.send(event.clone());
                        // Send to external channel if configured
                        if let Some(ref ext_tx) = external_tx {
                            let _ = ext_tx.send(event);
                        }
                    }
                    Err(_e) => {
                        // Non-JSON output - silently ignore (could be debug output)
                        // Don't use eprintln! as it bypasses the TUI
                    }
                }
            }

            // stdout closed - process likely exited
            *state.lock().await = ForgeConnectionState::Disconnected;
        });

        // Spawn stderr reader task - sends stderr lines as events
        let last_error_clone = last_error.clone();
        let stderr_tx = tx.clone();
        let stderr_external_tx = self.event_tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    // Create timestamp for the stderr line
                    let timestamp = chrono::Utc::now().to_rfc3339();
                    let event = ForgeEvent::StderrLine(StderrLineEvent {
                        line: line.clone(),
                        timestamp,
                    });
                    // Send to internal channel
                    let _ = stderr_tx.send(event.clone());
                    // Send to external channel if configured
                    if let Some(ref ext_tx) = stderr_external_tx {
                        let _ = ext_tx.send(event);
                    }
                    // Store last error line
                    *last_error_clone.lock().await = Some(line);
                }
            }
        });

        *self.state.lock().await = ForgeConnectionState::Connected;

        Ok(rx)
    }

    /// Stop the Forge subprocess gracefully
    ///
    /// Sends SIGTERM and waits for graceful shutdown.
    /// If the process doesn't exit within the timeout, sends SIGKILL.
    pub async fn stop(&mut self) -> Result<(), ForgeControllerError> {
        let child = match self.child.take() {
            Some(c) => c,
            None => return Ok(()), // Already stopped
        };

        *self.state.lock().await = ForgeConnectionState::Disconnecting;

        // Get the process ID for signaling
        let pid = child.id();

        if let Some(pid) = pid {
            // Send SIGTERM for graceful shutdown
            #[cfg(unix)]
            {
                use libc::{kill, SIGTERM};
                unsafe {
                    kill(pid as i32, SIGTERM);
                }
            }

            // Wait for graceful shutdown with timeout
            let mut child = child;
            let result = timeout(
                Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                child.wait(),
            )
            .await;

            match result {
                Ok(Ok(_status)) => {
                    // Process exited gracefully
                    *self.state.lock().await = ForgeConnectionState::Disconnected;
                }
                Ok(Err(e)) => {
                    // Error waiting for process
                    *self.state.lock().await = ForgeConnectionState::Failed;
                    *self.last_error.lock().await = Some(e.to_string());
                    return Err(ForgeControllerError::WaitFailed(e.to_string()));
                }
                Err(_) => {
                    // Timeout - send SIGKILL
                    #[cfg(unix)]
                    {
                        use libc::{kill, SIGKILL};
                        unsafe {
                            kill(pid as i32, SIGKILL);
                        }
                    }

                    // Wait a bit more for SIGKILL to take effect
                    let _ = timeout(Duration::from_secs(1), child.wait()).await;
                    *self.state.lock().await = ForgeConnectionState::Disconnected;
                }
            }
        } else {
            // No PID available, just mark as disconnected
            *self.state.lock().await = ForgeConnectionState::Disconnected;
        }

        self.config = None;
        *self.stdin.lock().await = None;
        Ok(())
    }

    /// Kill the Forge subprocess immediately (SIGKILL)
    pub async fn kill(&mut self) -> Result<(), ForgeControllerError> {
        if let Some(mut child) = self.child.take() {
            child
                .kill()
                .await
                .map_err(|e| ForgeControllerError::KillFailed(e.to_string()))?;
        }

        *self.state.lock().await = ForgeConnectionState::Disconnected;
        self.config = None;
        *self.stdin.lock().await = None;
        Ok(())
    }
}

impl Default for ForgeController {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors from ForgeController operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum ForgeControllerError {
    #[error("Forge is already running")]
    AlreadyRunning,

    #[error("Failed to serialize config: {0}")]
    ConfigSerializationFailed(String),

    #[error("Failed to spawn Forge process: {0}")]
    SpawnFailed(String),

    #[error("Failed to write config to stdin: {0}")]
    StdinWriteFailed(String),

    #[error("Stdout pipe unavailable")]
    StdoutUnavailable,

    #[error("Stderr pipe unavailable")]
    StderrUnavailable,

    #[error("Stdin pipe unavailable for resume response")]
    StdinUnavailable,

    #[error("Failed to wait for process: {0}")]
    WaitFailed(String),

    #[error("Failed to kill process: {0}")]
    KillFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_new() {
        let controller = ForgeController::new();
        assert!(controller.child.is_none());
        assert!(controller.config.is_none());
    }

    #[tokio::test]
    async fn test_controller_state_disconnected() {
        let controller = ForgeController::new();
        assert_eq!(controller.state().await, ForgeConnectionState::Disconnected);
        assert!(!controller.is_running().await);
    }

    #[tokio::test]
    async fn test_controller_no_error_initially() {
        let controller = ForgeController::new();
        assert!(controller.last_error().await.is_none());
    }

    #[test]
    fn test_forge_connection_state_variants() {
        // Ensure all variants are defined
        let states = vec![
            ForgeConnectionState::Disconnected,
            ForgeConnectionState::Connecting,
            ForgeConnectionState::Connected,
            ForgeConnectionState::Disconnecting,
            ForgeConnectionState::Failed,
        ];
        assert_eq!(states.len(), 5);
    }

    #[test]
    fn test_error_display() {
        let err = ForgeControllerError::AlreadyRunning;
        assert_eq!(err.to_string(), "Forge is already running");

        let err = ForgeControllerError::SpawnFailed("test error".to_string());
        assert!(err.to_string().contains("test error"));
    }

    #[tokio::test]
    async fn test_stop_when_not_running() {
        let mut controller = ForgeController::new();
        // Should not error when nothing is running
        let result = controller.stop().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_kill_when_not_running() {
        let mut controller = ForgeController::new();
        // Should not error when nothing is running
        let result = controller.kill().await;
        assert!(result.is_ok());
    }
}
