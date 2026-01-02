//! Mandrel integration for cross-session memory
//!
//! Connects to Mandrel MCP server for persistent context storage,
//! task management, and semantic search across sessions.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Error types for Mandrel operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum MandrelError {
    #[error("HTTP request failed: {message}")]
    HttpError { message: String },

    #[error("Failed to parse response: {message}")]
    ParseError { message: String },

    #[error("Mandrel server error: {message}")]
    ServerError { message: String },

    #[error("Not connected to Mandrel")]
    NotConnected,

    #[error("Invalid project: {project}")]
    InvalidProject { project: String },
}

impl From<reqwest::Error> for MandrelError {
    fn from(e: reqwest::Error) -> Self {
        MandrelError::HttpError {
            message: e.to_string(),
        }
    }
}

/// Configuration for Mandrel connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MandrelConfig {
    /// Base URL of the Mandrel server (e.g., "https://mandrel.ridgetopai.net")
    pub base_url: String,
    /// Current project name
    pub project: String,
    /// Whether Mandrel integration is enabled
    pub enabled: bool,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for MandrelConfig {
    fn default() -> Self {
        Self {
            base_url: "https://mandrel.ridgetopai.net".to_string(),
            project: "ridge-control".to_string(),
            enabled: true,
            timeout_secs: 30,
        }
    }
}

/// Context entry from Mandrel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub context_type: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub similarity: Option<f64>,
}

/// Task entry from Mandrel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Project info from Mandrel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub context_count: Option<u64>,
}

/// Task progress summary from Mandrel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub total: u32,
    pub completed: u32,
    pub in_progress: u32,
    pub blocked: u32,
    #[serde(default)]
    pub completion_percentage: f64,
    #[serde(default)]
    pub by_status: std::collections::HashMap<String, u32>,
}

/// Smart search result from Mandrel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub contexts: Vec<Context>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub total_results: u32,
}

/// Client for interacting with Mandrel MCP server
pub struct MandrelClient {
    client: Client,
    config: MandrelConfig,
}

impl MandrelClient {
    /// Create a new MandrelClient
    pub fn new(config: MandrelConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Check if Mandrel is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the current project
    pub fn project(&self) -> &str {
        &self.config.project
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    /// Update configuration
    pub fn set_config(&mut self, config: MandrelConfig) {
        self.config = config;
    }

    /// Make a tool call to Mandrel and return the text response
    async fn call_tool(&self, tool_name: &str, arguments: serde_json::Value) -> Result<String, MandrelError> {
        if !self.config.enabled {
            return Err(MandrelError::NotConnected);
        }

        let url = format!("{}/mcp/tools/{}", self.config.base_url, tool_name);
        let body = serde_json::json!({ "arguments": arguments });

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MandrelError::ServerError {
                message: format!("HTTP {}: {}", status, text),
            });
        }

        let text = response.text().await?;

        // Parse the MCP response format: {"success": true, "result": {"content": [{"type": "text", "text": "..."}]}}
        let raw: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            MandrelError::ParseError {
                message: format!("Failed to parse JSON: {} - Response: {}", e, &text[..text.len().min(200)]),
            }
        })?;

        // Extract text from result.content[0].text
        if let Some(result) = raw.get("result") {
            if let Some(content) = result.get("content") {
                if let Some(arr) = content.as_array() {
                    if let Some(first) = arr.first() {
                        if let Some(text_content) = first.get("text") {
                            if let Some(text_str) = text_content.as_str() {
                                return Ok(text_str.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Fallback: return the raw response
        Ok(text)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Project Management
    // ─────────────────────────────────────────────────────────────────────────

    /// Switch to a project
    pub async fn project_switch(&mut self, project: &str) -> Result<String, MandrelError> {
        let args = serde_json::json!({ "project": project });
        let result = self.call_tool("project_switch", args).await?;
        self.config.project = project.to_string();
        Ok(result)
    }

    /// Get current project info
    pub async fn project_current(&self) -> Result<String, MandrelError> {
        self.call_tool("project_current", serde_json::json!({})).await
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Context Management
    // ─────────────────────────────────────────────────────────────────────────

    /// Store a context entry
    pub async fn context_store(
        &self,
        content: &str,
        context_type: &str,
        tags: &[String],
    ) -> Result<String, MandrelError> {
        let args = serde_json::json!({
            "content": content,
            "type": context_type,
            "tags": tags
        });
        self.call_tool("context_store", args).await
    }

    /// Get recent contexts
    pub async fn context_get_recent(&self, limit: Option<u32>) -> Result<String, MandrelError> {
        let args = if let Some(l) = limit {
            serde_json::json!({ "limit": l })
        } else {
            serde_json::json!({})
        };
        self.call_tool("context_get_recent", args).await
    }

    /// Search contexts semantically
    pub async fn context_search(&self, query: &str) -> Result<String, MandrelError> {
        let args = serde_json::json!({ "query": query });
        self.call_tool("context_search", args).await
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Task Management
    // ─────────────────────────────────────────────────────────────────────────

    /// Create a new task
    pub async fn task_create(
        &self,
        title: &str,
        description: Option<&str>,
        priority: Option<&str>,
    ) -> Result<String, MandrelError> {
        let mut args = serde_json::json!({ "title": title });
        if let Some(desc) = description {
            args["description"] = serde_json::Value::String(desc.to_string());
        }
        if let Some(pri) = priority {
            args["priority"] = serde_json::Value::String(pri.to_string());
        }
        self.call_tool("task_create", args).await
    }

    /// Update a task's status
    pub async fn task_update(&self, task_id: &str, status: &str) -> Result<String, MandrelError> {
        let args = serde_json::json!({
            "taskId": task_id,
            "status": status
        });
        self.call_tool("task_update", args).await
    }

    /// List tasks with optional filtering
    pub async fn task_list(&self, status: Option<&str>) -> Result<String, MandrelError> {
        let args = if let Some(s) = status {
            serde_json::json!({ "status": s })
        } else {
            serde_json::json!({})
        };
        self.call_tool("task_list", args).await
    }

    /// Get task details
    pub async fn task_details(&self, task_id: &str) -> Result<String, MandrelError> {
        let args = serde_json::json!({ "taskId": task_id });
        self.call_tool("task_details", args).await
    }

    /// Get task progress summary
    pub async fn task_progress_summary(&self) -> Result<String, MandrelError> {
        self.call_tool("task_progress_summary", serde_json::json!({})).await
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Smart Search
    // ─────────────────────────────────────────────────────────────────────────

    /// Smart search across all data sources
    pub async fn smart_search(&self, query: &str) -> Result<String, MandrelError> {
        let args = serde_json::json!({ "query": query });
        self.call_tool("smart_search", args).await
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Utility Methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Test connection to Mandrel
    pub async fn ping(&self) -> Result<String, MandrelError> {
        self.call_tool("mandrel_ping", serde_json::json!({})).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mandrel_config_default() {
        let config = MandrelConfig::default();
        assert_eq!(config.base_url, "https://mandrel.ridgetopai.net");
        assert_eq!(config.project, "ridge-control");
        assert!(config.enabled);
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn test_mandrel_config_serialization() {
        let config = MandrelConfig {
            base_url: "http://localhost:8080".to_string(),
            project: "test-project".to_string(),
            enabled: false,
            timeout_secs: 60,
        };

        let toml = toml::to_string(&config).unwrap();
        assert!(toml.contains("http://localhost:8080"));
        assert!(toml.contains("test-project"));

        let parsed: MandrelConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.base_url, config.base_url);
        assert_eq!(parsed.project, config.project);
    }

    #[test]
    fn test_mandrel_client_creation() {
        let config = MandrelConfig::default();
        let client = MandrelClient::new(config.clone());
        assert!(client.is_enabled());
        assert_eq!(client.project(), "ridge-control");
        assert_eq!(client.base_url(), "https://mandrel.ridgetopai.net");
    }

    #[test]
    fn test_mandrel_client_disabled() {
        let config = MandrelConfig {
            enabled: false,
            ..Default::default()
        };
        let client = MandrelClient::new(config);
        assert!(!client.is_enabled());
    }

    #[test]
    fn test_context_struct() {
        let json = r#"{
            "id": "ctx-123",
            "content": "Test content",
            "type": "code",
            "tags": ["rust", "test"],
            "created_at": "2024-01-01T00:00:00Z"
        }"#;

        let context: Context = serde_json::from_str(json).unwrap();
        assert_eq!(context.id, "ctx-123");
        assert_eq!(context.content, "Test content");
        assert_eq!(context.context_type, "code");
        assert_eq!(context.tags.len(), 2);
    }

    #[test]
    fn test_task_struct() {
        let json = r#"{
            "id": "task-456",
            "title": "Implement feature",
            "status": "in_progress",
            "priority": "high"
        }"#;

        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "task-456");
        assert_eq!(task.title, "Implement feature");
        assert_eq!(task.status, "in_progress");
        assert_eq!(task.priority, Some("high".to_string()));
    }

    #[test]
    fn test_task_progress_struct() {
        let json = r#"{
            "total": 10,
            "completed": 3,
            "in_progress": 2,
            "blocked": 1,
            "completion_percentage": 30.0,
            "by_status": {"todo": 4, "done": 3}
        }"#;

        let progress: TaskProgress = serde_json::from_str(json).unwrap();
        assert_eq!(progress.total, 10);
        assert_eq!(progress.completed, 3);
        assert_eq!(progress.completion_percentage, 30.0);
    }

    #[test]
    fn test_mandrel_error_display() {
        let err = MandrelError::HttpError {
            message: "Connection refused".to_string(),
        };
        assert!(err.to_string().contains("Connection refused"));

        let err = MandrelError::NotConnected;
        assert!(err.to_string().contains("Not connected"));

        let err = MandrelError::InvalidProject {
            project: "bad-project".to_string(),
        };
        assert!(err.to_string().contains("bad-project"));
    }

    #[test]
    fn test_search_result_struct() {
        let json = r#"{
            "contexts": [{"id": "c1", "content": "test", "type": "code", "tags": []}],
            "tasks": [{"id": "t1", "title": "Test task", "status": "todo"}],
            "total_results": 2
        }"#;

        let result: SearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.contexts.len(), 1);
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.total_results, 2);
    }
}
