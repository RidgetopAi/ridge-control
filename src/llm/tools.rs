// Tool execution - some types for future features
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::time::timeout;

use super::types::{ToolDefinition, ToolResult, ToolResultContent, ToolUse};

/// Tool execution policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicy {
    pub name: String,
    pub require_confirmation: bool,
    pub dangerous_mode_only: bool,
    pub timeout_secs: u64,
    pub max_output_bytes: usize,
    pub allowed_paths: Vec<String>,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            name: String::new(),
            require_confirmation: true,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 1_048_576, // 1MB
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        }
    }
}

/// Result of checking if a tool can be executed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolExecutionCheck {
    Allowed,
    RequiresConfirmation,
    RequiresDangerousMode,
    UnknownTool,
    PathNotAllowed,
}

/// Error during tool execution
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),
    
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    
    #[error("Timeout after {0}s")]
    Timeout(u64),
    
    #[error("Path not allowed: {0}")]
    PathNotAllowed(String),
    
    #[error("Dangerous mode required")]
    DangerousModeRequired,
    
    #[error("I/O error: {0}")]
    IoError(String),
    
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// Tool registry with policies
pub struct ToolRegistry {
    policies: HashMap<String, ToolPolicy>,
    dangerous_mode: bool,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            policies: HashMap::new(),
            dangerous_mode: false,
        };
        
        // Register default tools
        registry.register_defaults();
        registry
    }
    
    fn register_defaults(&mut self) {
        // File read - safe, no confirmation needed
        self.policies.insert("file_read".to_string(), ToolPolicy {
            name: "file_read".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 10,
            max_output_bytes: 1_048_576,
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });
        
        // File write - requires confirmation
        self.policies.insert("file_write".to_string(), ToolPolicy {
            name: "file_write".to_string(),
            require_confirmation: true,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 1_048_576,
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });
        
        // File delete - dangerous mode only
        self.policies.insert("file_delete".to_string(), ToolPolicy {
            name: "file_delete".to_string(),
            require_confirmation: true,
            dangerous_mode_only: true,
            timeout_secs: 10,
            max_output_bytes: 4096,
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });
        
        // Bash execute - dangerous mode only
        self.policies.insert("bash_execute".to_string(), ToolPolicy {
            name: "bash_execute".to_string(),
            require_confirmation: true,
            dangerous_mode_only: true,
            timeout_secs: 60,
            max_output_bytes: 1_048_576,
            allowed_paths: vec![],
        });
        
        // List directory - safe
        self.policies.insert("list_directory".to_string(), ToolPolicy {
            name: "list_directory".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 10,
            max_output_bytes: 102_400,
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });
    }
    
    pub fn set_dangerous_mode(&mut self, enabled: bool) {
        self.dangerous_mode = enabled;
    }
    
    pub fn is_dangerous_mode(&self) -> bool {
        self.dangerous_mode
    }
    
    pub fn get_policy(&self, tool_name: &str) -> Option<&ToolPolicy> {
        self.policies.get(tool_name)
    }
    
    pub fn can_execute(&self, tool_name: &str, user_confirmed: bool) -> ToolExecutionCheck {
        let policy = match self.policies.get(tool_name) {
            Some(p) => p,
            None => return ToolExecutionCheck::UnknownTool,
        };
        
        if policy.dangerous_mode_only && !self.dangerous_mode {
            return ToolExecutionCheck::RequiresDangerousMode;
        }
        
        if policy.require_confirmation && !user_confirmed {
            return ToolExecutionCheck::RequiresConfirmation;
        }
        
        ToolExecutionCheck::Allowed
    }
    
    /// Get tool definitions for LLM
    pub fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "file_read".to_string(),
                description: "Read the contents of a file".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to read"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "file_write".to_string(),
                description: "Write content to a file".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to write"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDefinition {
                name: "list_directory".to_string(),
                description: "List contents of a directory".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the directory to list"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "bash_execute".to_string(),
                description: "Execute a bash command (requires dangerous mode)".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The bash command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            },
        ]
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool executor handles running tools with sandboxing
pub struct ToolExecutor {
    registry: ToolRegistry,
    working_dir: PathBuf,
}

impl ToolExecutor {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            registry: ToolRegistry::new(),
            working_dir,
        }
    }
    
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }
    
    pub fn registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.registry
    }
    
    pub fn set_dangerous_mode(&mut self, enabled: bool) {
        self.registry.set_dangerous_mode(enabled);
    }
    
    /// Check if a tool can be executed
    pub fn can_execute(&self, tool: &ToolUse, user_confirmed: bool) -> ToolExecutionCheck {
        let check = self.registry.can_execute(&tool.name, user_confirmed);
        
        if check != ToolExecutionCheck::Allowed {
            return check;
        }
        
        // Check path restrictions for file tools
        if let Some(path) = self.extract_path(&tool.input) {
            if !self.is_path_allowed(&tool.name, &path) {
                return ToolExecutionCheck::PathNotAllowed;
            }
        }
        
        ToolExecutionCheck::Allowed
    }
    
    fn extract_path(&self, input: &serde_json::Value) -> Option<PathBuf> {
        input.get("path").and_then(|p| p.as_str()).map(PathBuf::from)
    }
    
    fn is_path_allowed(&self, tool_name: &str, path: &Path) -> bool {
        let policy = match self.registry.get_policy(tool_name) {
            Some(p) => p,
            None => return false,
        };
        
        if policy.allowed_paths.is_empty() {
            return true; // No restrictions
        }
        
        // Resolve the path
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.working_dir.join(path)
        };
        
        // Check for path traversal
        let path_str = resolved.to_string_lossy();
        if path_str.contains("..") {
            return false;
        }
        
        // Check against allowed patterns
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        
        for pattern in &policy.allowed_paths {
            let expanded = if let Some(stripped) = pattern.strip_prefix("~/") {
                home_dir.join(stripped)
            } else {
                PathBuf::from(pattern)
            };
            
            if resolved.starts_with(&expanded) {
                return true;
            }
        }
        
        false
    }
    
    /// Execute a tool and return the result
    pub async fn execute(&self, tool: &ToolUse) -> Result<ToolResult, ToolError> {
        let policy = self.registry.get_policy(&tool.name)
            .ok_or_else(|| ToolError::NotFound(tool.name.clone()))?;
        
        let result = match tool.name.as_str() {
            "file_read" => self.execute_file_read(tool, policy).await,
            "file_write" => self.execute_file_write(tool, policy).await,
            "list_directory" => self.execute_list_directory(tool, policy).await,
            "bash_execute" => self.execute_bash(tool, policy).await,
            "file_delete" => self.execute_file_delete(tool, policy).await,
            _ => Err(ToolError::NotFound(tool.name.clone())),
        };
        
        match result {
            Ok(content) => Ok(ToolResult {
                tool_use_id: tool.id.clone(),
                content: ToolResultContent::Text(content),
                is_error: false,
            }),
            Err(e) => Ok(ToolResult {
                tool_use_id: tool.id.clone(),
                content: ToolResultContent::Text(e.to_string()),
                is_error: true,
            }),
        }
    }
    
    async fn execute_file_read(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'path' parameter".to_string()))?;
        
        let resolved = self.resolve_path(path);
        
        if !self.is_path_allowed(&tool.name, &resolved) {
            return Err(ToolError::PathNotAllowed(path.to_string()));
        }
        
        let read_future = tokio::fs::read_to_string(&resolved);
        let content = timeout(Duration::from_secs(policy.timeout_secs), read_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::IoError(e.to_string()))?;
        
        // Truncate if too large
        if content.len() > policy.max_output_bytes {
            Ok(format!(
                "{}...\n\n[TRUNCATED: File exceeds {} bytes]",
                &content[..policy.max_output_bytes],
                policy.max_output_bytes
            ))
        } else {
            Ok(content)
        }
    }
    
    async fn execute_file_write(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'path' parameter".to_string()))?;
        
        let content = tool.input.get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'content' parameter".to_string()))?;
        
        let resolved = self.resolve_path(path);
        
        if !self.is_path_allowed(&tool.name, &resolved) {
            return Err(ToolError::PathNotAllowed(path.to_string()));
        }
        
        // Create parent directories if needed
        if let Some(parent) = resolved.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        
        let write_future = tokio::fs::write(&resolved, content);
        timeout(Duration::from_secs(policy.timeout_secs), write_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::IoError(e.to_string()))?;
        
        Ok(format!("Successfully wrote {} bytes to {}", content.len(), path))
    }
    
    async fn execute_file_delete(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'path' parameter".to_string()))?;
        
        let resolved = self.resolve_path(path);
        
        if !self.is_path_allowed(&tool.name, &resolved) {
            return Err(ToolError::PathNotAllowed(path.to_string()));
        }
        
        let delete_future = tokio::fs::remove_file(&resolved);
        timeout(Duration::from_secs(policy.timeout_secs), delete_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::IoError(e.to_string()))?;
        
        Ok(format!("Successfully deleted {}", path))
    }
    
    async fn execute_list_directory(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'path' parameter".to_string()))?;
        
        let resolved = self.resolve_path(path);
        
        if !self.is_path_allowed(&tool.name, &resolved) {
            return Err(ToolError::PathNotAllowed(path.to_string()));
        }
        
        let list_future = async {
            let mut entries = tokio::fs::read_dir(&resolved).await?;
            let mut result = Vec::new();
            
            while let Some(entry) = entries.next_entry().await? {
                let file_type = entry.file_type().await?;
                let name = entry.file_name().to_string_lossy().to_string();
                let suffix = if file_type.is_dir() { "/" } else { "" };
                result.push(format!("{}{}", name, suffix));
            }
            
            result.sort();
            Ok::<_, std::io::Error>(result)
        };
        
        let entries = timeout(Duration::from_secs(policy.timeout_secs), list_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::IoError(e.to_string()))?;
        
        Ok(entries.join("\n"))
    }
    
    async fn execute_bash(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        if !self.registry.is_dangerous_mode() {
            return Err(ToolError::DangerousModeRequired);
        }
        
        let command = tool.input.get("command")
            .and_then(|c| c.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'command' parameter".to_string()))?;
        
        let exec_future = async {
            let child = Command::new("bash")
                .arg("-c")
                .arg(command)
                .current_dir(&self.working_dir)
                .env_clear()
                .env("PATH", "/usr/local/bin:/usr/bin:/bin")
                .env("HOME", dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
                .env("TERM", "xterm-256color")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            
            let output = child.wait_with_output().await?;
            Ok::<_, std::io::Error>(output)
        };
        
        let output = timeout(Duration::from_secs(policy.timeout_secs), exec_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);
        
        let mut result = String::new();
        
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n\n--- STDERR ---\n");
            }
            result.push_str(&stderr);
        }
        
        result.push_str(&format!("\n\n[Exit code: {}]", exit_code));
        
        // Truncate if too large
        if result.len() > policy.max_output_bytes {
            result = format!(
                "{}...\n\n[TRUNCATED: Output exceeds {} bytes]",
                &result[..policy.max_output_bytes],
                policy.max_output_bytes
            );
        }
        
        Ok(result)
    }
    
    fn resolve_path(&self, path: &str) -> PathBuf {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        
        if let Some(stripped) = path.strip_prefix("~/") {
            home_dir.join(stripped)
        } else if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            self.working_dir.join(path)
        }
    }
}

/// Pending tool use waiting for confirmation
#[derive(Debug, Clone)]
pub struct PendingToolUse {
    pub tool: ToolUse,
    pub check: ToolExecutionCheck,
}

impl PendingToolUse {
    pub fn new(tool: ToolUse, check: ToolExecutionCheck) -> Self {
        Self { tool, check }
    }
    
    pub fn tool_name(&self) -> &str {
        &self.tool.name
    }
    
    pub fn tool_id(&self) -> &str {
        &self.tool.id
    }
    
    pub fn input_summary(&self) -> String {
        match self.tool.name.as_str() {
            "file_read" | "file_write" | "list_directory" | "file_delete" => {
                self.tool.input.get("path")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string())
            }
            "bash_execute" => {
                self.tool.input.get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| {
                        if s.len() > 60 {
                            format!("{}...", &s[..60])
                        } else {
                            s.to_string()
                        }
                    })
                    .unwrap_or_else(|| "<unknown>".to_string())
            }
            _ => serde_json::to_string(&self.tool.input)
                .unwrap_or_else(|_| "<error>".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_registry_defaults() {
        let registry = ToolRegistry::new();
        assert!(registry.get_policy("file_read").is_some());
        assert!(registry.get_policy("file_write").is_some());
        assert!(registry.get_policy("bash_execute").is_some());
    }

    #[test]
    fn test_tool_execution_check() {
        let registry = ToolRegistry::new();
        
        // file_read doesn't require confirmation
        assert_eq!(
            registry.can_execute("file_read", false),
            ToolExecutionCheck::Allowed
        );
        
        // file_write requires confirmation
        assert_eq!(
            registry.can_execute("file_write", false),
            ToolExecutionCheck::RequiresConfirmation
        );
        assert_eq!(
            registry.can_execute("file_write", true),
            ToolExecutionCheck::Allowed
        );
        
        // bash_execute requires dangerous mode
        assert_eq!(
            registry.can_execute("bash_execute", true),
            ToolExecutionCheck::RequiresDangerousMode
        );
    }

    #[test]
    fn test_dangerous_mode() {
        let mut registry = ToolRegistry::new();
        
        assert_eq!(
            registry.can_execute("bash_execute", true),
            ToolExecutionCheck::RequiresDangerousMode
        );
        
        registry.set_dangerous_mode(true);
        
        assert_eq!(
            registry.can_execute("bash_execute", true),
            ToolExecutionCheck::Allowed
        );
    }
}
