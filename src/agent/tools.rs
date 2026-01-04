//! Agent tool orchestrator - bridge between agent loop and existing tool system

use async_trait::async_trait;

use crate::llm::types::{ToolResult, ToolResultContent, ToolUse};

/// Result of tool execution
#[derive(Debug)]
#[allow(dead_code)]
pub struct ToolExecutionResult {
    pub tool_use_id: String,
    pub result: ToolResult,
    /// Whether to continue the agent loop after this tool
    pub should_continue: bool,
}

/// Error during tool orchestration
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AgentToolOrchestratorError {
    #[error("Unknown tool: {name}")]
    UnknownTool { name: String },

    #[error("Invalid input: {message}")]
    InvalidInput { message: String },

    #[error("Execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("Tool requires confirmation")]
    RequiresConfirmation { tool_use: ToolUse },

    #[error("Tool was rejected by user")]
    Rejected,

    #[error("Timeout after {timeout_secs}s")]
    Timeout { timeout_secs: u32 },
}

/// Trait for orchestrating tool execution within the agent loop
#[async_trait]
#[allow(dead_code)]
pub trait AgentToolOrchestrator: Send + Sync {
    /// Execute a tool and return the result
    async fn execute(&self, tool_use: ToolUse) -> Result<ToolExecutionResult, AgentToolOrchestratorError>;

    /// Check if a tool requires user confirmation
    fn requires_confirmation(&self, tool_name: &str) -> bool;

    /// Get list of available tool names
    fn available_tools(&self) -> Vec<String>;
}

/// Default tool orchestrator that bridges to the existing llm::tools system
#[allow(dead_code)]
pub struct DefaultAgentToolOrchestrator {
    /// Tool names that require confirmation
    confirmation_required: Vec<String>,
    /// Whether to require confirmation for all tools (dangerously_allow_all = false)
    confirm_all: bool,
}

#[allow(dead_code)]
impl DefaultAgentToolOrchestrator {
    pub fn new() -> Self {
        Self {
            confirmation_required: vec![
                "bash_run".to_string(),
                "file_write".to_string(),
                "file_delete".to_string(),
            ],
            confirm_all: true,
        }
    }

    /// Set whether to require confirmation for all tools
    pub fn with_confirm_all(mut self, confirm_all: bool) -> Self {
        self.confirm_all = confirm_all;
        self
    }

    /// Add a tool that requires confirmation
    pub fn require_confirmation(mut self, tool_name: impl Into<String>) -> Self {
        self.confirmation_required.push(tool_name.into());
        self
    }
}

impl Default for DefaultAgentToolOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentToolOrchestrator for DefaultAgentToolOrchestrator {
    async fn execute(&self, tool_use: ToolUse) -> Result<ToolExecutionResult, AgentToolOrchestratorError> {
        // Check if confirmation is required
        if self.requires_confirmation(&tool_use.name) {
            return Err(AgentToolOrchestratorError::RequiresConfirmation { tool_use });
        }

        // Execute the tool based on name
        // Currently all tools go through UI confirmation flow
        // This will be expanded when we wire up direct tool execution
        Err(AgentToolOrchestratorError::UnknownTool {
            name: tool_use.name.clone(),
        })
    }

    fn requires_confirmation(&self, tool_name: &str) -> bool {
        if self.confirm_all {
            return true;
        }
        self.confirmation_required.iter().any(|t| t == tool_name)
    }

    fn available_tools(&self) -> Vec<String> {
        // Will be populated from registered tools
        vec![]
    }
}

/// A pass-through orchestrator that always requires confirmation
/// Used when the UI handles tool confirmation
#[allow(dead_code)]
pub struct ConfirmationRequiredExecutor;

#[async_trait]
impl AgentToolOrchestrator for ConfirmationRequiredExecutor {
    async fn execute(&self, tool_use: ToolUse) -> Result<ToolExecutionResult, AgentToolOrchestratorError> {
        Err(AgentToolOrchestratorError::RequiresConfirmation { tool_use })
    }

    fn requires_confirmation(&self, _tool_name: &str) -> bool {
        true
    }

    fn available_tools(&self) -> Vec<String> {
        vec![]
    }
}

/// Helper to create a successful tool result
#[allow(dead_code)]
pub fn success_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> ToolResult {
    ToolResult {
        tool_use_id: tool_use_id.into(),
        content: ToolResultContent::Text(content.into()),
        is_error: false,
    }
}

/// Helper to create an error tool result
#[allow(dead_code)]
pub fn error_result(tool_use_id: impl Into<String>, error: impl Into<String>) -> ToolResult {
    ToolResult {
        tool_use_id: tool_use_id.into(),
        content: ToolResultContent::Text(error.into()),
        is_error: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_result() {
        let result = success_result("tool-123", "Operation completed");
        assert!(!result.is_error);
        assert_eq!(result.tool_use_id, "tool-123");
    }

    #[test]
    fn test_error_result() {
        let result = error_result("tool-456", "Failed to execute");
        assert!(result.is_error);
    }

    #[test]
    fn test_confirmation_required() {
        let executor = DefaultAgentToolOrchestrator::new();
        assert!(executor.requires_confirmation("bash_run"));
        assert!(executor.requires_confirmation("file_write"));
    }
}
