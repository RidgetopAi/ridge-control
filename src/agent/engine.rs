//! Agent engine - main state machine for the agent loop

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::llm::types::{ContentBlock, Message, Role, StopReason, StreamChunk, ToolDefinition, ToolResult, ToolUse, Usage};
use crate::llm::{LLMEvent, LLMManager};

use super::context::{BuildContextParams, ContextManager, ContextSegment, SegmentKind};
use super::prompt::SystemPromptBuilder;
use super::thread::{AgentThread, ThreadStore};
use super::tools::ToolExecutor;

/// Agent state machine states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    /// Initial state, no active conversation
    Idle,
    /// Waiting for user input
    AwaitingUserInput,
    /// Building and preparing request
    PreparingRequest,
    /// Streaming response from LLM
    StreamingResponse,
    /// Executing tools from LLM request
    ExecutingTools,
    /// Finalizing the turn (saving to thread)
    FinalizingTurn,
    /// Error state (recoverable)
    Error,
}

/// Events emitted by the agent engine
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// State changed
    StateChanged(AgentState),
    /// Stream chunk received
    Chunk(StreamChunk),
    /// Tool use requested
    ToolUseRequested(ToolUse),
    /// Tool execution complete
    ToolExecuted {
        tool_use_id: String,
        success: bool,
    },
    /// Turn complete
    TurnComplete {
        stop_reason: StopReason,
        usage: Option<Usage>,
    },
    /// Error occurred
    Error(String),
    /// Context was truncated
    ContextTruncated {
        segments_dropped: usize,
        tokens_used: u32,
        budget: u32,
    },
}

/// Configuration for the agent engine
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Tool definitions available to the agent
    pub tools: Vec<ToolDefinition>,
    /// Maximum turns in a single agent loop (prevents runaway)
    pub max_turns: usize,
    /// Whether to auto-continue after tool execution
    pub auto_continue: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            max_turns: 10,
            auto_continue: true,
        }
    }
}

/// The main agent engine - orchestrates the agent loop
pub struct AgentEngine<S: ThreadStore> {
    /// LLM manager for API calls
    llm: LLMManager,
    /// Context manager for window management
    context_manager: Arc<ContextManager>,
    /// System prompt builder
    prompt_builder: SystemPromptBuilder,
    /// Tool executor
    tool_executor: Arc<dyn ToolExecutor>,
    /// Thread storage
    thread_store: Arc<S>,
    /// Current active thread
    current_thread: Option<AgentThread>,
    /// Current state
    state: AgentState,
    /// Configuration
    config: AgentConfig,
    /// Event sender
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    /// Turn counter for current loop
    turn_count: usize,
    /// Accumulated assistant response for current turn
    current_response: Vec<ContentBlock>,
    /// Pending tool uses
    pending_tools: Vec<ToolUse>,
}

impl<S: ThreadStore> AgentEngine<S> {
    pub fn new(
        llm: LLMManager,
        context_manager: Arc<ContextManager>,
        prompt_builder: SystemPromptBuilder,
        tool_executor: Arc<dyn ToolExecutor>,
        thread_store: Arc<S>,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Self {
        Self {
            llm,
            context_manager,
            prompt_builder,
            tool_executor,
            thread_store,
            current_thread: None,
            state: AgentState::Idle,
            config: AgentConfig::default(),
            event_tx,
            turn_count: 0,
            current_response: Vec::new(),
            pending_tools: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Get current state
    pub fn state(&self) -> AgentState {
        self.state
    }

    /// Get current thread
    pub fn current_thread(&self) -> Option<&AgentThread> {
        self.current_thread.as_ref()
    }

    /// Get current thread mutably
    pub fn current_thread_mut(&mut self) -> Option<&mut AgentThread> {
        self.current_thread.as_mut()
    }

    /// Start a new conversation thread
    pub fn new_thread(&mut self, model: impl Into<String>) {
        let thread = AgentThread::new(model);
        self.current_thread = Some(thread);
        self.turn_count = 0;
        self.transition(AgentState::AwaitingUserInput);
    }

    /// Load an existing thread
    pub fn load_thread(&mut self, id: &str) -> Result<(), String> {
        let thread = self
            .thread_store
            .get(id)
            .ok_or_else(|| format!("Thread not found: {}", id))?;
        self.current_thread = Some(thread);
        self.turn_count = 0;
        self.transition(AgentState::AwaitingUserInput);
        Ok(())
    }

    /// Manually save the current thread to storage
    /// Returns Ok(()) on success, or an error message on failure.
    /// This is for manual save operations - threads are also auto-saved on TurnComplete.
    pub fn save_thread(&self) -> Result<(), String> {
        match self.current_thread.as_ref() {
            Some(thread) => self.thread_store.save(thread),
            None => Err("No active thread to save".to_string()),
        }
    }

    /// Send a user message and start the agent loop
    pub fn send_message(&mut self, message: impl Into<String>) {
        let message = message.into();

        let thread = match self.current_thread.as_mut() {
            Some(t) => t,
            None => {
                self.emit(AgentEvent::Error("No active thread".to_string()));
                return;
            }
        };

        // Add user message as a chat segment
        let user_msg = Message::user(message);
        let segment = ContextSegment::new(
            SegmentKind::ChatHistory,
            vec![user_msg],
            thread.peek_sequence(),
        );
        thread.add_segment(segment);

        // Reset turn counter
        self.turn_count = 0;
        self.current_response.clear();
        self.pending_tools.clear();

        // Build and send request
        self.prepare_and_send();
    }

    /// Continue after tool execution
    pub fn continue_after_tools(&mut self, results: Vec<ToolResult>) {
        let thread = match self.current_thread.as_mut() {
            Some(t) => t,
            None => {
                self.emit(AgentEvent::Error("No active thread".to_string()));
                return;
            }
        };

        // Add tool results as messages
        let tool_messages: Vec<Message> = results
            .into_iter()
            .map(|r| Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult(r)],
            })
            .collect();

        let segment = ContextSegment::new(
            SegmentKind::ToolExchange,
            tool_messages,
            thread.peek_sequence(),
        );
        thread.add_segment(segment);

        // Clear state for next turn - tools have been handled
        self.pending_tools.clear();
        self.current_response.clear();

        // Continue the loop
        self.prepare_and_send();
    }

    /// Handle an LLM event from the stream
    pub fn handle_llm_event(&mut self, event: LLMEvent) {
        match event {
            LLMEvent::Chunk(chunk) => {
                self.handle_chunk(chunk);
            }
            LLMEvent::Complete => {
                self.handle_completion();
            }
            LLMEvent::Error(e) => {
                self.emit(AgentEvent::Error(format!("LLM error: {}", e)));
                self.transition(AgentState::Error);
            }
            LLMEvent::ToolUseDetected(tool_use) => {
                self.pending_tools.push(tool_use.clone());
                self.emit(AgentEvent::ToolUseRequested(tool_use));
            }
        }
    }

    /// Cancel current operation
    pub fn cancel(&mut self) {
        self.llm.cancel();
        self.transition(AgentState::AwaitingUserInput);
    }

    fn prepare_and_send(&mut self) {
        self.transition(AgentState::PreparingRequest);

        let thread = match self.current_thread.as_ref() {
            Some(t) => t,
            None => return,
        };

        // Build context with truncation
        let params = BuildContextParams {
            model: thread.model.clone(),
            system_prompt: Some(self.prompt_builder.build()),
            short_system_prompt: Some(self.prompt_builder.build_short()),
            tools: self.config.tools.clone(),
            segments: thread.segments.clone(),
            max_output_tokens: None,
        };

        let built = self.context_manager.build_request(params);

        if built.truncated {
            self.emit(AgentEvent::ContextTruncated {
                segments_dropped: built.segments_dropped,
                tokens_used: built.total_tokens,
                budget: built.budget,
            });
        }

        // Send via LLM manager
        self.llm.clear_conversation();
        for msg in &built.request.messages {
            match msg.role {
                Role::User => {
                    // Handle different content types
                    for block in &msg.content {
                        match block {
                            ContentBlock::Text(t) => self.llm.add_user_message(t.clone()),
                            ContentBlock::ToolResult(r) => self.llm.add_tool_result(r.clone()),
                            _ => {}
                        }
                    }
                }
                Role::Assistant => {
                    for block in &msg.content {
                        match block {
                            ContentBlock::Text(t) => self.llm.add_assistant_message(t.clone()),
                            ContentBlock::ToolUse(tu) => self.llm.add_tool_use(tu.clone()),
                            _ => {}
                        }
                    }
                }
            }
        }

        self.llm.continue_after_tool(built.request.system, self.config.tools.clone());
        self.transition(AgentState::StreamingResponse);
    }

    fn handle_chunk(&mut self, chunk: StreamChunk) {
        self.emit(AgentEvent::Chunk(chunk.clone()));

        // Accumulate response content
        if let StreamChunk::Delta(delta) = chunk {
            match delta {
                crate::llm::types::StreamDelta::Text(text) => {
                    // Add to current text block or create new one
                    if let Some(ContentBlock::Text(ref mut t)) = self.current_response.last_mut() {
                        t.push_str(&text);
                    } else {
                        self.current_response.push(ContentBlock::Text(text));
                    }
                }
                crate::llm::types::StreamDelta::ToolInput { .. } => {
                    // Tool inputs are handled via ToolUseDetected event
                }
                crate::llm::types::StreamDelta::Thinking(text) => {
                    if let Some(ContentBlock::Thinking(ref mut t)) = self.current_response.last_mut() {
                        t.push_str(&text);
                    } else {
                        self.current_response.push(ContentBlock::Thinking(text));
                    }
                }
            }
        }
    }

    fn handle_completion(&mut self) {
        self.turn_count += 1;

        // Save assistant response to thread
        if let Some(thread) = self.current_thread.as_mut() {
            if !self.current_response.is_empty() {
                let assistant_msg = Message {
                    role: Role::Assistant,
                    content: self.current_response.clone(),
                };
                let segment = ContextSegment::new(
                    SegmentKind::ChatHistory,
                    vec![assistant_msg],
                    thread.peek_sequence(),
                );
                thread.add_segment(segment);
            }
        }

        // Check if we have pending tools
        if !self.pending_tools.is_empty() {
            self.transition(AgentState::ExecutingTools);
            // UI will handle tool execution and call continue_after_tools
        } else {
            self.finalize_turn(StopReason::EndTurn, None);
        }
    }

    fn finalize_turn(&mut self, reason: StopReason, usage: Option<Usage>) {
        self.transition(AgentState::FinalizingTurn);

        // Save thread to store
        if let Some(thread) = self.current_thread.as_ref() {
            if let Err(e) = self.thread_store.save(thread) {
                self.emit(AgentEvent::Error(format!("Failed to save thread: {}", e)));
            }
        }

        self.emit(AgentEvent::TurnComplete {
            stop_reason: reason,
            usage,
        });

        self.current_response.clear();
        self.pending_tools.clear();
        self.transition(AgentState::AwaitingUserInput);
    }

    fn transition(&mut self, new_state: AgentState) {
        if self.state != new_state {
            self.state = new_state;
            self.emit(AgentEvent::StateChanged(new_state));
        }
    }

    fn emit(&self, event: AgentEvent) {
        let _ = self.event_tx.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::models::ModelCatalog;
    use crate::agent::thread::InMemoryThreadStore;
    use crate::agent::tokens::DefaultTokenCounter;
    use crate::agent::tools::ConfirmationRequiredExecutor;

    fn create_test_engine() -> (
        AgentEngine<InMemoryThreadStore>,
        mpsc::UnboundedReceiver<AgentEvent>,
    ) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let catalog = Arc::new(ModelCatalog::new());
        let counter = Arc::new(DefaultTokenCounter::new(catalog.clone()));
        let context_manager = Arc::new(ContextManager::new(catalog, counter));
        let prompt_builder = SystemPromptBuilder::ridge_control();
        let tool_executor = Arc::new(ConfirmationRequiredExecutor);
        let thread_store = Arc::new(InMemoryThreadStore::new());
        let llm = LLMManager::new();

        let engine = AgentEngine::new(
            llm,
            context_manager,
            prompt_builder,
            tool_executor,
            thread_store,
            event_tx,
        );

        (engine, event_rx)
    }

    #[test]
    fn test_engine_creation() {
        let (engine, _rx) = create_test_engine();
        assert_eq!(engine.state(), AgentState::Idle);
    }

    #[test]
    fn test_new_thread() {
        let (mut engine, mut rx) = create_test_engine();
        engine.new_thread("gpt-4o");

        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
        assert!(engine.current_thread().is_some());

        // Should have received state change event
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::StateChanged(AgentState::AwaitingUserInput)));
    }
}
