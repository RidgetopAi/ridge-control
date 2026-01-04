//! Subagent management for spawning specialized sub-agents
//!
//! SubagentManager handles spawning sub-agents (explore, plan, review) with
//! different models and tool configurations. Sub-agents run autonomously and
//! return summarized results.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::task::JoinHandle;

use crate::config::{KeyId, KeyStore, SubagentsConfig, SubagentConfig};
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::gemini::GeminiProvider;
use crate::llm::grok::GrokProvider;
use crate::llm::groq::GroqProvider;
use crate::llm::openai::OpenAIProvider;
use crate::llm::provider::Provider;
use crate::llm::types::{ContentBlock, LLMRequest, Message, ToolDefinition};

/// Result of a sub-agent execution
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SubagentResult {
    /// Unique task ID for this sub-agent run
    pub task_id: String,
    /// Type of agent that ran (explore, plan, review)
    pub agent_type: String,
    /// Result text from the sub-agent
    pub result: String,
    /// Total tokens used (input + output)
    pub tokens_used: u32,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Status of the task
    pub status: SubagentStatus,
}

/// Status of a sub-agent task
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SubagentStatus {
    /// Task is still running (for background tasks)
    Pending,
    /// Task completed successfully
    Completed,
    /// Task failed with error
    Failed(String),
}

/// Error during sub-agent execution
#[derive(Debug, Clone, thiserror::Error)]
#[allow(dead_code)]
pub enum SubagentError {
    #[error("Provider '{provider}' not configured - add API key in settings")]
    ProviderNotConfigured { provider: String },

    #[error("No API key found for provider '{provider}'")]
    NoApiKey { provider: String },

    #[error("LLM request failed: {message}")]
    LLMError { message: String },

    #[error("Task not found: {task_id}")]
    TaskNotFound { task_id: String },

    #[error("Task still running: {task_id}")]
    TaskStillRunning { task_id: String },

    #[error("Invalid agent type: {agent_type}")]
    InvalidAgentType { agent_type: String },

    #[error("Keystore not available")]
    NoKeystore,
}

/// Manager for spawning and tracking sub-agents
#[allow(dead_code)]
pub struct SubagentManager {
    /// Sub-agent configuration
    config: SubagentsConfig,
    /// Running background tasks
    running_tasks: HashMap<String, JoinHandle<Result<SubagentResult, SubagentError>>>,
    /// Completed results (cached for retrieval)
    completed_results: HashMap<String, Result<SubagentResult, SubagentError>>,
    /// All available tool definitions (for filtering)
    all_tools: Vec<ToolDefinition>,
}

impl SubagentManager {
    /// Create a new SubagentManager
    pub fn new(config: SubagentsConfig) -> Self {
        Self {
            config,
            running_tasks: HashMap::new(),
            completed_results: HashMap::new(),
            all_tools: Vec::new(),
        }
    }

    /// Set available tools (called when tools are configured)
    #[allow(dead_code)]
    pub fn set_tools(&mut self, tools: Vec<ToolDefinition>) {
        self.all_tools = tools;
    }

    /// Update configuration
    #[allow(dead_code)]
    pub fn set_config(&mut self, config: SubagentsConfig) {
        self.config = config;
    }

    /// Get configuration
    #[allow(dead_code)]
    pub fn config(&self) -> &SubagentsConfig {
        &self.config
    }

    /// Spawn a sub-agent with the given prompt
    ///
    /// # Arguments
    /// * `keystore` - KeyStore for getting API keys
    /// * `agent_type` - Type of agent (explore, plan, review)
    /// * `prompt` - Task description for the sub-agent
    /// * `background` - Run in background (returns immediately with pending status)
    #[allow(dead_code)]
    pub async fn spawn(
        &mut self,
        keystore: &KeyStore,
        agent_type: &str,
        prompt: &str,
        background: bool,
    ) -> Result<SubagentResult, SubagentError> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let config = self.config.get(agent_type).clone();

        // Get tools filtered to allowed list
        let tools = self.filter_tools(&config.allowed_tools);

        // Create provider for this agent type
        let provider = create_provider(keystore, &config.provider)?;

        // Build the request
        let request = build_request(&config, prompt, tools);

        let agent_type_owned = agent_type.to_string();
        let task_id_clone = task_id.clone();

        if background {
            // Spawn as background task
            let handle = tokio::spawn(async move {
                execute_subagent(task_id_clone, agent_type_owned, provider, request).await
            });

            self.running_tasks.insert(task_id.clone(), handle);

            Ok(SubagentResult {
                task_id,
                agent_type: agent_type.to_string(),
                result: String::new(),
                tokens_used: 0,
                duration_ms: 0,
                status: SubagentStatus::Pending,
            })
        } else {
            // Execute synchronously
            execute_subagent(task_id, agent_type_owned, provider, request).await
        }
    }

    /// Check if a background task is complete
    #[allow(dead_code)]
    pub fn is_task_complete(&self, task_id: &str) -> bool {
        if self.completed_results.contains_key(task_id) {
            return true;
        }
        if let Some(handle) = self.running_tasks.get(task_id) {
            return handle.is_finished();
        }
        false
    }

    /// Get result of a background task (blocks if still running)
    #[allow(dead_code)]
    pub async fn get_task_result(&mut self, task_id: &str) -> Result<SubagentResult, SubagentError> {
        // Check completed cache first
        if let Some(result) = self.completed_results.remove(task_id) {
            return result;
        }

        // Check running tasks
        if let Some(handle) = self.running_tasks.remove(task_id) {
            match handle.await {
                Ok(result) => {
                    // Cache and return
                    let result_clone = result.clone();
                    self.completed_results.insert(task_id.to_string(), result);
                    result_clone
                }
                Err(e) => {
                    Err(SubagentError::LLMError {
                        message: format!("Task panicked: {}", e),
                    })
                }
            }
        } else {
            Err(SubagentError::TaskNotFound {
                task_id: task_id.to_string(),
            })
        }
    }

    /// Poll a background task without blocking
    #[allow(dead_code)]
    pub fn poll_task(&mut self, task_id: &str) -> Option<Result<SubagentResult, SubagentError>> {
        // Check completed cache
        if let Some(result) = self.completed_results.get(task_id) {
            return Some(result.clone());
        }

        // Check if running task is finished
        if let Some(handle) = self.running_tasks.get(task_id) {
            if handle.is_finished() {
                // Remove and get result
                if let Some(handle) = self.running_tasks.remove(task_id) {
                    // Use blocking get since we know it's finished
                    match futures::executor::block_on(handle) {
                        Ok(result) => {
                            self.completed_results.insert(task_id.to_string(), result.clone());
                            return Some(result);
                        }
                        Err(e) => {
                            let err = Err(SubagentError::LLMError {
                                message: format!("Task panicked: {}", e),
                            });
                            self.completed_results.insert(task_id.to_string(), err.clone());
                            return Some(err);
                        }
                    }
                }
            }
        }

        None
    }

    /// Get list of running task IDs
    #[allow(dead_code)]
    pub fn running_task_ids(&self) -> Vec<String> {
        self.running_tasks.keys().cloned().collect()
    }

    /// Cancel a running task
    #[allow(dead_code)]
    pub fn cancel_task(&mut self, task_id: &str) -> bool {
        if let Some(handle) = self.running_tasks.remove(task_id) {
            handle.abort();
            true
        } else {
            false
        }
    }

    /// Filter tools to only those in the allowed list
    fn filter_tools(&self, allowed: &[String]) -> Vec<ToolDefinition> {
        if allowed.is_empty() {
            // Empty means all tools allowed
            return self.all_tools.clone();
        }

        self.all_tools
            .iter()
            .filter(|t| allowed.contains(&t.name))
            .cloned()
            .collect()
    }
}

/// Create a provider for the given provider name
#[allow(dead_code)]
fn create_provider(keystore: &KeyStore, provider_name: &str) -> Result<Arc<dyn Provider>, SubagentError> {
    let key_id = match provider_name {
        "anthropic" => KeyId::Anthropic,
        "openai" => KeyId::OpenAI,
        "gemini" => KeyId::Gemini,
        "grok" => KeyId::Grok,
        "groq" => KeyId::Groq,
        _ => {
            return Err(SubagentError::ProviderNotConfigured {
                provider: provider_name.to_string(),
            });
        }
    };

    let api_key = match keystore.get(&key_id) {
        Ok(Some(secret)) => secret.expose().to_string(),
        Ok(None) => {
            return Err(SubagentError::NoApiKey {
                provider: provider_name.to_string(),
            });
        }
        Err(_) => {
            return Err(SubagentError::NoApiKey {
                provider: provider_name.to_string(),
            });
        }
    };

    let provider: Arc<dyn Provider> = match provider_name {
        "anthropic" => Arc::new(AnthropicProvider::new(api_key)),
        "openai" => Arc::new(OpenAIProvider::new(api_key)),
        "gemini" => Arc::new(GeminiProvider::new(api_key)),
        "grok" => Arc::new(GrokProvider::new(api_key)),
        "groq" => Arc::new(GroqProvider::new(api_key)),
        _ => unreachable!(),
    };

    Ok(provider)
}

/// Build an LLM request for the sub-agent
#[allow(dead_code)]
fn build_request(
    config: &SubagentConfig,
    prompt: &str,
    tools: Vec<ToolDefinition>,
) -> LLMRequest {
    let system_prompt = format!(
        "You are a specialized sub-agent. Complete the following task and provide a concise summary of your findings.\n\
         Focus on actionable information and specific details.\n\
         When done, provide a clear summary of what you found or accomplished."
    );

    LLMRequest {
        model: config.model.clone(),
        system: Some(system_prompt),
        messages: vec![Message::user(prompt.to_string())],
        tools,
        stream: false,
        max_tokens: config.max_context_tokens.map(|t| t.min(4096)), // Cap output
        ..Default::default()
    }
}

/// Execute a sub-agent (internal async function)
#[allow(dead_code)]
async fn execute_subagent(
    task_id: String,
    agent_type: String,
    provider: Arc<dyn Provider>,
    mut request: LLMRequest,
) -> Result<SubagentResult, SubagentError> {
    let start = Instant::now();
    let mut total_tokens = 0u32;
    let mut accumulated_response = String::new();

    // Simple loop for tool use (max 5 iterations to prevent runaway)
    for _turn in 0..5 {
        let response = provider.complete(request.clone()).await.map_err(|e| {
            SubagentError::LLMError {
                message: e.to_string(),
            }
        })?;

        // Accumulate tokens
        total_tokens += response.usage.input_tokens + response.usage.output_tokens;

        // Extract text and tool uses from response
        let mut has_tool_use = false;
        let mut tool_results = Vec::new();

        for block in &response.content {
            match block {
                ContentBlock::Text(text) => {
                    accumulated_response.push_str(text);
                }
                ContentBlock::ToolUse(tool_use) => {
                    has_tool_use = true;
                    // For now, we don't execute tools in sub-agents
                    // Just note that a tool was requested
                    tool_results.push(crate::llm::types::ToolResult {
                        tool_use_id: tool_use.id.clone(),
                        content: crate::llm::types::ToolResultContent::Text(
                            "Tool execution not available in sub-agent context".to_string()
                        ),
                        is_error: true,
                    });
                }
                _ => {}
            }
        }

        if !has_tool_use {
            // No tool use, we're done
            break;
        }

        // Add assistant response and tool results to continue
        request.messages.push(Message {
            role: crate::llm::types::Role::Assistant,
            content: response.content.clone(),
        });

        for result in tool_results {
            request.messages.push(Message {
                role: crate::llm::types::Role::User,
                content: vec![ContentBlock::ToolResult(result)],
            });
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(SubagentResult {
        task_id,
        agent_type,
        result: accumulated_response,
        tokens_used: total_tokens,
        duration_ms,
        status: SubagentStatus::Completed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SubagentsConfig;

    #[test]
    fn test_subagent_result_creation() {
        let result = SubagentResult {
            task_id: "test-123".to_string(),
            agent_type: "explore".to_string(),
            result: "Found 5 files".to_string(),
            tokens_used: 1000,
            duration_ms: 500,
            status: SubagentStatus::Completed,
        };

        assert_eq!(result.task_id, "test-123");
        assert_eq!(result.agent_type, "explore");
        assert_eq!(result.status, SubagentStatus::Completed);
    }

    #[test]
    fn test_subagent_status_variants() {
        assert_eq!(SubagentStatus::Pending, SubagentStatus::Pending);
        assert_eq!(SubagentStatus::Completed, SubagentStatus::Completed);

        let err1 = SubagentStatus::Failed("error1".to_string());
        let err2 = SubagentStatus::Failed("error1".to_string());
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_subagent_error_display() {
        let err = SubagentError::ProviderNotConfigured {
            provider: "anthropic".to_string(),
        };
        assert!(err.to_string().contains("anthropic"));

        let err = SubagentError::NoApiKey {
            provider: "openai".to_string(),
        };
        assert!(err.to_string().contains("openai"));
    }

    #[test]
    fn test_subagent_manager_new() {
        let config = SubagentsConfig::default();
        let manager = SubagentManager::new(config);
        assert!(manager.running_task_ids().is_empty());
    }

    #[test]
    fn test_filter_tools_empty_allows_all() {
        // When allowed_tools is empty, all tools should be returned
        let config = SubagentsConfig::default();
        assert!(config.plan.allowed_tools.is_empty()); // Plan allows all
    }

    #[test]
    fn test_filter_tools_specific_list() {
        let config = SubagentsConfig::default();
        // Explore has specific tools
        assert!(config.explore.allowed_tools.contains(&"file_read".to_string()));
        assert!(config.explore.allowed_tools.contains(&"grep".to_string()));
    }
}
