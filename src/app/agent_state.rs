// AgentRuntimeState - Extracted agent/LLM/tool state from App struct (Order 8.4)
// Contains: AgentEngine, streaming buffers, tool orchestration, chat UI, thread management
// Named "AgentRuntimeState" to avoid collision with crate::agent::AgentState enum

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::agent::{
    AgentEngine, AgentEvent, DiskThreadStore, ModelCatalog, SubagentManager, TokenCounter,
};
use crate::components::chat_input::ChatInput;
use crate::components::conversation_viewer::ConversationViewer;
use crate::components::thread_picker::ThreadPicker;
use crate::llm::{BlockType, LLMEvent, PendingToolUse, ToolExecutor, ToolResult};

pub struct AgentRuntimeState {
    // Core agent engine and event channels
    pub agent_engine: AgentEngine<DiskThreadStore>,
    pub agent_event_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    pub agent_llm_event_rx: Option<mpsc::UnboundedReceiver<LLMEvent>>,

    // Thread management
    pub current_thread_id: Option<String>,
    pub thread_rename_buffer: Option<String>,
    pub thread_picker: ThreadPicker,

    // Streaming state
    pub llm_response_buffer: String,
    pub thinking_buffer: String,
    pub current_block_type: Option<BlockType>,
    #[allow(dead_code)]
    pub collapse_thinking: bool,

    // Token counting
    pub model_catalog: Arc<ModelCatalog>,
    pub token_counter: Arc<dyn TokenCounter>,
    pub cached_token_count: Option<(usize, u32)>,

    // Chat UI components (agent-centric)
    pub conversation_viewer: ConversationViewer,
    pub chat_input: ChatInput,
    pub show_conversation: bool,

    // Sub-agents (T2.2)
    pub subagent_manager: Option<SubagentManager>,

    // Tool execution
    pub tool_executor: ToolExecutor,

    // Tool batch tracking (Order 6 optimization)
    pub pending_tools: HashMap<String, PendingToolUse>,
    pub current_batch_id: u64,
    pub expected_tool_batch: Option<(u64, usize)>,
    pub collected_results: HashMap<String, ToolResult>,
    pub tool_batch_map: HashMap<String, u64>,

    // Tool streaming state
    pub confirming_tool_id: Option<String>,
    pub current_tool_id: Option<String>,
    pub current_tool_name: Option<String>,
    pub current_tool_input: String,
    pub tool_result_rxs:
        HashMap<String, mpsc::UnboundedReceiver<std::result::Result<ToolResult, crate::llm::ToolError>>>,

    // Dangerous mode (TRC-018)
    pub dangerous_mode: bool,
}

impl AgentRuntimeState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_engine: AgentEngine<DiskThreadStore>,
        agent_event_rx: mpsc::UnboundedReceiver<AgentEvent>,
        agent_llm_event_rx: Option<mpsc::UnboundedReceiver<LLMEvent>>,
        model_catalog: Arc<ModelCatalog>,
        token_counter: Arc<dyn TokenCounter>,
        tool_executor: ToolExecutor,
        subagent_manager: Option<SubagentManager>,
    ) -> Self {
        Self {
            agent_engine,
            agent_event_rx: Some(agent_event_rx),
            agent_llm_event_rx,
            current_thread_id: None,
            thread_rename_buffer: None,
            thread_picker: ThreadPicker::new(),
            llm_response_buffer: String::new(),
            thinking_buffer: String::new(),
            current_block_type: None,
            collapse_thinking: false,
            model_catalog,
            token_counter,
            cached_token_count: None,
            conversation_viewer: ConversationViewer::new(),
            chat_input: ChatInput::new(),
            show_conversation: false,
            subagent_manager,
            tool_executor,
            pending_tools: HashMap::new(),
            current_batch_id: 0,
            expected_tool_batch: None,
            collected_results: HashMap::new(),
            tool_batch_map: HashMap::new(),
            confirming_tool_id: None,
            current_tool_id: None,
            current_tool_name: None,
            current_tool_input: String::new(),
            tool_result_rxs: HashMap::new(),
            dangerous_mode: false,
        }
    }

    /// Clear all streaming buffers (response, thinking, tool input)
    pub fn clear_streaming_buffers(&mut self) {
        self.llm_response_buffer.clear();
        self.thinking_buffer.clear();
        self.current_block_type = None;
        self.current_tool_id = None;
        self.current_tool_name = None;
        self.current_tool_input.clear();
    }

    /// Set dangerous mode on both the state and tool executor
    pub fn set_dangerous_mode(&mut self, enabled: bool) {
        self.dangerous_mode = enabled;
        self.tool_executor.set_dangerous_mode(enabled);
    }

    /// Invalidate token count cache (call when messages change)
    #[inline]
    pub fn invalidate_token_cache(&mut self) {
        self.cached_token_count = None;
    }
}
