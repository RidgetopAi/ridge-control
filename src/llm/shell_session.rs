// Shell Session Pool - Persistent shell sessions with background execution
//
// Provides named shell sessions that maintain state (cwd, env) across calls,
// plus background task execution with output buffering.

use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
// Note: pty_process async APIs available but using tokio::process for simpler integration
// Can switch to PTY for interactive shell features in the future
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{interval, timeout};
use uuid::Uuid;

/// Maximum number of concurrent sessions
const MAX_SESSIONS: usize = 10;

/// Idle timeout before session cleanup (30 minutes)
const IDLE_TIMEOUT_SECS: u64 = 1800;

/// Maximum output buffer size per background task (1MB)
const MAX_OUTPUT_BUFFER: usize = 1_048_576;

/// Output buffer for background tasks
#[derive(Debug, Default)]
struct OutputBuffer {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
}

impl OutputBuffer {
    fn append_stdout(&mut self, data: &[u8]) {
        if self.stdout.len() + data.len() > MAX_OUTPUT_BUFFER {
            let remaining = MAX_OUTPUT_BUFFER.saturating_sub(self.stdout.len());
            self.stdout.extend_from_slice(&data[..remaining]);
            self.truncated = true;
        } else {
            self.stdout.extend_from_slice(data);
        }
    }

    fn append_stderr(&mut self, data: &[u8]) {
        if self.stderr.len() + data.len() > MAX_OUTPUT_BUFFER {
            let remaining = MAX_OUTPUT_BUFFER.saturating_sub(self.stderr.len());
            self.stderr.extend_from_slice(&data[..remaining]);
            self.truncated = true;
        } else {
            self.stderr.extend_from_slice(data);
        }
    }
}

/// Status of a background task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

/// A background task running in a session
pub struct BackgroundTask {
    pub task_id: String,
    pub command: String,
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    output_buffer: Arc<Mutex<OutputBuffer>>,
    status: Arc<Mutex<TaskStatus>>,
    exit_code: Arc<Mutex<Option<i32>>>,
    handle: Option<JoinHandle<()>>,
}

#[allow(dead_code)]
impl BackgroundTask {
    /// Get current status
    pub async fn status(&self) -> TaskStatus {
        *self.status.lock().await
    }

    /// Get exit code (if completed)
    pub async fn exit_code(&self) -> Option<i32> {
        *self.exit_code.lock().await
    }

    /// Get current output (non-blocking)
    pub async fn output(&self) -> BackgroundTaskOutput {
        let buffer = self.output_buffer.lock().await;
        BackgroundTaskOutput {
            task_id: self.task_id.clone(),
            session_id: self.session_id.clone(),
            command: self.command.clone(),
            status: *self.status.lock().await,
            stdout: String::from_utf8_lossy(&buffer.stdout).to_string(),
            stderr: String::from_utf8_lossy(&buffer.stderr).to_string(),
            exit_code: *self.exit_code.lock().await,
            truncated: buffer.truncated,
            started_at: self.started_at,
        }
    }

    /// Kill the background task
    pub async fn kill(&mut self) -> bool {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            *self.status.lock().await = TaskStatus::Killed;
            true
        } else {
            false
        }
    }
}

/// Output from a background task
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundTaskOutput {
    pub task_id: String,
    pub session_id: String,
    pub command: String,
    pub status: TaskStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub started_at: DateTime<Utc>,
}

/// Result of foreground command execution
#[derive(Debug, Clone, Serialize)]
pub struct ExecResult {
    pub session_id: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub cwd: String,
    pub duration_ms: u64,
    pub truncated: bool,
}

/// Result of spawning a background task
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundSpawnResult {
    pub session_id: String,
    pub task_id: String,
    pub command: String,
    pub message: String,
}

/// Shell session errors
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum SessionError {
    #[error("Session pool full (max {0} sessions)")]
    PoolFull(usize),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("PTY error: {0}")]
    PtyError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Timeout after {0}s")]
    Timeout(u64),

    #[error("Command failed: {0}")]
    CommandFailed(String),
}

/// A persistent shell session
#[allow(dead_code)]
pub struct ShellSession {
    pub id: String,
    cwd: PathBuf,
    env: HashMap<String, String>,
    created_at: DateTime<Utc>,
    last_used: DateTime<Utc>,
    background_tasks: HashMap<String, BackgroundTask>,
}

#[allow(dead_code)]
impl ShellSession {
    /// Create a new session with default working directory
    pub fn new(id: String) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let mut env = HashMap::new();

        // Set basic environment
        env.insert("PATH".to_string(), "/usr/local/bin:/usr/bin:/bin".to_string());
        if let Some(home) = dirs::home_dir() {
            env.insert("HOME".to_string(), home.to_string_lossy().to_string());
        }
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("LANG".to_string(), "en_US.UTF-8".to_string());

        Self {
            id,
            cwd,
            env,
            created_at: Utc::now(),
            last_used: Utc::now(),
            background_tasks: HashMap::new(),
        }
    }

    /// Execute a command in foreground (blocking with timeout)
    pub async fn execute(
        &mut self,
        command: &str,
        timeout_secs: u64,
        cwd_override: Option<&str>,
        max_output: usize,
    ) -> Result<ExecResult, SessionError> {
        self.last_used = Utc::now();
        let start = std::time::Instant::now();

        // Determine working directory
        let work_dir = if let Some(cwd) = cwd_override {
            PathBuf::from(cwd)
        } else {
            self.cwd.clone()
        };

        // Build the command with cd and env setup
        // We use a wrapper script to capture cwd changes
        let wrapped_command = format!(
            r#"cd {} 2>/dev/null || true; {}; echo ""; echo "__PWD__:$(pwd)""#,
            shell_escape(&work_dir.to_string_lossy()),
            command
        );

        // Spawn the command using tokio::process for simpler async handling
        let child = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(&wrapped_command)
            .current_dir(&work_dir)
            .envs(&self.env)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        // Wait with timeout
        let output = timeout(
            Duration::from_secs(timeout_secs),
            child.wait_with_output()
        )
        .await
        .map_err(|_| SessionError::Timeout(timeout_secs))?
        .map_err(SessionError::IoError)?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let exit_code = output.status.code()
            .or_else(|| output.status.signal().map(|s| 128 + s))
            .unwrap_or(-1);

        // Parse stdout, extracting new cwd if present
        let stdout_raw = String::from_utf8_lossy(&output.stdout);
        let (stdout, new_cwd) = extract_cwd(&stdout_raw);

        // Update session cwd if command changed it
        if let Some(cwd) = new_cwd {
            self.cwd = PathBuf::from(cwd);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Check truncation
        let (stdout, truncated) = truncate_output(&stdout, max_output);
        let (stderr, stderr_truncated) = truncate_output(&stderr, max_output / 2);

        Ok(ExecResult {
            session_id: self.id.clone(),
            command: command.to_string(),
            stdout,
            stderr,
            exit_code,
            cwd: self.cwd.to_string_lossy().to_string(),
            duration_ms,
            truncated: truncated || stderr_truncated,
        })
    }

    /// Spawn a command in background
    pub async fn spawn_background(&mut self, command: &str) -> Result<BackgroundSpawnResult, SessionError> {
        self.last_used = Utc::now();

        let task_id = format!("bg-{}", Uuid::new_v4().to_string().split('-').next().unwrap());
        let session_id = self.id.clone();
        let work_dir = self.cwd.clone();
        let env = self.env.clone();
        let command_owned = command.to_string();

        let output_buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        let status = Arc::new(Mutex::new(TaskStatus::Running));
        let exit_code = Arc::new(Mutex::new(None));

        let buffer_clone = output_buffer.clone();
        let status_clone = status.clone();
        let exit_code_clone = exit_code.clone();
        let command_for_task = command_owned.clone();

        // Spawn the background task
        let handle = tokio::spawn(async move {
            let result = run_background_command(
                &command_for_task,
                &work_dir,
                &env,
                buffer_clone,
            ).await;

            match result {
                Ok(code) => {
                    *exit_code_clone.lock().await = Some(code);
                    *status_clone.lock().await = TaskStatus::Completed;
                }
                Err(_) => {
                    *status_clone.lock().await = TaskStatus::Failed;
                }
            }
        });

        let task = BackgroundTask {
            task_id: task_id.clone(),
            command: command_owned,
            session_id: session_id.clone(),
            started_at: Utc::now(),
            output_buffer,
            status,
            exit_code,
            handle: Some(handle),
        };

        self.background_tasks.insert(task_id.clone(), task);

        Ok(BackgroundSpawnResult {
            session_id,
            task_id,
            command: command.to_string(),
            message: "Command running in background. Use bash_output to check status.".to_string(),
        })
    }

    /// Get a background task by ID
    pub fn get_task(&self, task_id: &str) -> Option<&BackgroundTask> {
        self.background_tasks.get(task_id)
    }

    /// Get a mutable background task by ID
    pub fn get_task_mut(&mut self, task_id: &str) -> Option<&mut BackgroundTask> {
        self.background_tasks.get_mut(task_id)
    }

    /// Remove completed tasks older than the threshold
    pub async fn cleanup_tasks(&mut self, max_age: Duration) {
        let now = Utc::now();
        let to_remove: Vec<_> = self.background_tasks.iter()
            .filter(|(_, task)| {
                let age = now.signed_duration_since(task.started_at);
                age.to_std().map(|d| d > max_age).unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_remove {
            self.background_tasks.remove(&id);
        }
    }

    /// Update environment variable
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_string(), value.to_string());
    }

    /// Get current working directory
    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    /// Check if session is idle
    pub fn is_idle(&self, threshold: Duration) -> bool {
        let idle_time = Utc::now().signed_duration_since(self.last_used);
        idle_time.to_std().map(|d| d > threshold).unwrap_or(false)
    }

    /// List all background tasks in this session
    pub fn list_tasks(&self) -> Vec<&BackgroundTask> {
        self.background_tasks.values().collect()
    }
}

/// Run a command in background, streaming output to buffer
async fn run_background_command(
    command: &str,
    work_dir: &PathBuf,
    env: &HashMap<String, String>,
    buffer: Arc<Mutex<OutputBuffer>>,
) -> Result<i32, SessionError> {
    let mut child = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(work_dir)
        .envs(env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Spawn tasks to read stdout and stderr
    let buffer_stdout = buffer.clone();
    let stdout_handle = if let Some(mut stdout) = stdout {
        Some(tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        buffer_stdout.lock().await.append_stdout(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }
        }))
    } else {
        None
    };

    let buffer_stderr = buffer.clone();
    let stderr_handle = if let Some(mut stderr) = stderr {
        Some(tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        buffer_stderr.lock().await.append_stderr(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }
        }))
    } else {
        None
    };

    // Wait for child to complete
    let status = child.wait().await?;

    // Wait for output readers to finish
    if let Some(h) = stdout_handle {
        let _ = h.await;
    }
    if let Some(h) = stderr_handle {
        let _ = h.await;
    }

    Ok(status.code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(-1))
}

/// Shell session pool managing multiple named sessions
pub struct ShellSessionPool {
    sessions: HashMap<String, ShellSession>,
    max_sessions: usize,
    idle_timeout: Duration,
}

impl Default for ShellSessionPool {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl ShellSessionPool {
    /// Create a new session pool
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            max_sessions: MAX_SESSIONS,
            idle_timeout: Duration::from_secs(IDLE_TIMEOUT_SECS),
        }
    }

    /// Get or create a session by ID
    pub fn get_or_create(&mut self, session_id: &str) -> Result<&mut ShellSession, SessionError> {
        if !self.sessions.contains_key(session_id) {
            // Check capacity
            if self.sessions.len() >= self.max_sessions {
                // Try to clean up idle sessions first
                self.cleanup_idle();

                if self.sessions.len() >= self.max_sessions {
                    return Err(SessionError::PoolFull(self.max_sessions));
                }
            }

            let session = ShellSession::new(session_id.to_string());
            self.sessions.insert(session_id.to_string(), session);
        }

        Ok(self.sessions.get_mut(session_id).unwrap())
    }

    /// Get an existing session
    pub fn get(&self, session_id: &str) -> Option<&ShellSession> {
        self.sessions.get(session_id)
    }

    /// Get a mutable session
    pub fn get_mut(&mut self, session_id: &str) -> Option<&mut ShellSession> {
        self.sessions.get_mut(session_id)
    }

    /// Remove a session
    pub fn remove(&mut self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }

    /// Clean up idle sessions
    pub fn cleanup_idle(&mut self) {
        let timeout = self.idle_timeout;
        self.sessions.retain(|_, session| !session.is_idle(timeout));
    }

    /// List all sessions
    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions.values()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                cwd: s.cwd.to_string_lossy().to_string(),
                created_at: s.created_at,
                last_used: s.last_used,
                task_count: s.background_tasks.len(),
            })
            .collect()
    }

    /// Find a task across all sessions
    pub fn find_task(&self, task_id: &str) -> Option<(&ShellSession, &BackgroundTask)> {
        for session in self.sessions.values() {
            if let Some(task) = session.get_task(task_id) {
                return Some((session, task));
            }
        }
        None
    }

    /// Find a mutable task across all sessions
    pub fn find_task_mut(&mut self, task_id: &str) -> Option<&mut BackgroundTask> {
        for session in self.sessions.values_mut() {
            if let Some(task) = session.get_task_mut(task_id) {
                return Some(task);
            }
        }
        None
    }

    /// Get session count
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Check if pool is empty
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Start background cleanup task
    pub fn start_cleanup_task(pool: Arc<Mutex<Self>>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;
                let mut pool = pool.lock().await;
                pool.cleanup_idle();
            }
        })
    }
}

/// Session info for listing
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct SessionInfo {
    pub id: String,
    pub cwd: String,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub task_count: usize,
}

/// Shell-escape a string for use in bash
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Extract new cwd from command output (looks for __PWD__: marker)
fn extract_cwd(output: &str) -> (String, Option<String>) {
    if let Some(pos) = output.rfind("__PWD__:") {
        let before = output[..pos].trim_end().to_string();
        let after = output[pos + 8..].trim().to_string();
        (before, Some(after))
    } else {
        (output.to_string(), None)
    }
}

/// Truncate output to max bytes, preserving UTF-8 boundaries
fn truncate_output(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_string(), false);
    }

    // Find last valid UTF-8 boundary
    let mut boundary = max;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }

    (format!("{}...[truncated]", &s[..boundary]), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("hello"), "'hello'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
        assert_eq!(shell_escape("/path/to/dir"), "'/path/to/dir'");
    }

    #[test]
    fn test_extract_cwd() {
        let output = "some output\n__PWD__:/home/user";
        let (content, cwd) = extract_cwd(output);
        assert_eq!(content, "some output");
        assert_eq!(cwd, Some("/home/user".to_string()));
    }

    #[test]
    fn test_extract_cwd_no_marker() {
        let output = "just regular output";
        let (content, cwd) = extract_cwd(output);
        assert_eq!(content, "just regular output");
        assert_eq!(cwd, None);
    }

    #[test]
    fn test_truncate_output() {
        let short = "hello";
        let (result, truncated) = truncate_output(short, 100);
        assert_eq!(result, "hello");
        assert!(!truncated);

        let long = "a".repeat(200);
        let (result, truncated) = truncate_output(&long, 100);
        assert!(result.len() <= 115); // 100 + "...[truncated]"
        assert!(truncated);
    }

    #[test]
    fn test_session_pool_capacity() {
        let mut pool = ShellSessionPool::new();

        // Fill the pool
        for i in 0..MAX_SESSIONS {
            assert!(pool.get_or_create(&format!("session-{}", i)).is_ok());
        }

        // Should fail when full
        assert!(matches!(
            pool.get_or_create("one-more"),
            Err(SessionError::PoolFull(_))
        ));
    }

    #[test]
    fn test_session_creation() {
        let session = ShellSession::new("test".to_string());
        assert_eq!(session.id, "test");
        assert!(!session.cwd.as_os_str().is_empty());
        assert!(session.env.contains_key("PATH"));
        assert!(session.background_tasks.is_empty());
    }

    #[tokio::test]
    async fn test_execute_simple_command() {
        let mut session = ShellSession::new("test".to_string());
        let result = session.execute("echo hello", 30, None, 1_048_576).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.stdout.contains("hello"));
        assert_eq!(output.exit_code, 0);
    }

    #[tokio::test]
    async fn test_cwd_persistence() {
        let mut session = ShellSession::new("test".to_string());

        // Change directory
        let result = session.execute("cd /tmp", 30, None, 1_048_576).await;
        assert!(result.is_ok());

        // Verify cwd changed
        assert_eq!(session.cwd(), &PathBuf::from("/tmp"));

        // Verify next command runs in new directory
        let result = session.execute("pwd", 30, None, 1_048_576).await;
        assert!(result.is_ok());
        assert!(result.unwrap().stdout.contains("/tmp"));
    }
}
