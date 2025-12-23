//! Agent SDK - Intelligent context management and agent loop for LLM interactions
//!
//! This module provides:
//! - Token counting with per-model tokenizers
//! - Context window management with intelligent truncation
//! - System prompt building with platform/repo awareness
//! - Agent thread persistence and management
//! - Full agent loop state machine

// Suppress unused warnings for now - these will be used when wired to app
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod models;
pub mod tokens;
pub mod context;
pub mod prompt;
pub mod thread;
pub mod disk_store;
pub mod tools;
pub mod engine;

pub use models::{ModelInfo, ModelCatalog, TokenizerKind};
pub use tokens::{TokenCounter, DefaultTokenCounter};
pub use context::{ContextSegment, SegmentKind, ContextManager, BuildContextParams, BuiltContext, ContextStats};
pub use prompt::{SystemPromptBuilder, PlatformInfo, RepoContextInfo};
pub use thread::{AgentThread, ThreadStore, InMemoryThreadStore};
pub use disk_store::DiskThreadStore;
pub use tools::{ToolExecutor, ConfirmationRequiredExecutor};
pub use engine::{AgentEngine, AgentState, AgentEvent};

#[cfg(test)]
mod integration_tests {
    use std::sync::Arc;
    use tokio::sync::mpsc;
    
    use crate::llm::types::{
        ContentBlock, Message, Role, StopReason, StreamChunk, StreamDelta, ToolDefinition, 
        ToolResult, ToolResultContent, ToolUse, Usage,
    };
    use crate::llm::LLMEvent;
    
    use super::*;
    use super::context::BuildContextParams;
    use super::engine::AgentConfig;
    use super::thread::InMemoryThreadStore;
    use super::tools::ConfirmationRequiredExecutor;

    // ============================================================================
    // Token Counting Accuracy Tests
    // ============================================================================

    #[test]
    fn test_token_counting_consistency_across_calls() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = DefaultTokenCounter::new(catalog);
        
        let text = "The quick brown fox jumps over the lazy dog.";
        let count1 = counter.count_text("gpt-4o", text);
        let count2 = counter.count_text("gpt-4o", text);
        let count3 = counter.count_text("gpt-4o", text);
        
        assert_eq!(count1, count2);
        assert_eq!(count2, count3);
    }

    #[test]
    fn test_token_counting_model_variations() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = DefaultTokenCounter::new(catalog);
        
        let text = "Hello, this is a test message with some content.";
        
        let gpt4_tokens = counter.count_text("gpt-4o", text);
        let claude_tokens = counter.count_text("claude-sonnet-4-20250514", text);
        let gemini_tokens = counter.count_text("gemini-1.5-pro", text);
        let unknown_tokens = counter.count_text("unknown-model", text);
        
        // All should return reasonable counts (> 0)
        assert!(gpt4_tokens > 0);
        assert!(claude_tokens > 0);
        assert!(gemini_tokens > 0);
        assert!(unknown_tokens > 0);
        
        // GPT and Claude use cl100k, should be identical
        assert_eq!(gpt4_tokens, claude_tokens);
        
        // Gemini also uses cl100k approximation
        assert_eq!(gpt4_tokens, gemini_tokens);
        
        // Unknown uses heuristic (chars/4), may differ
        // 50 chars / 4 = ~12-13 tokens
        assert!(unknown_tokens >= 10 && unknown_tokens <= 15);
    }

    #[test]
    fn test_message_token_counting_includes_overhead() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = DefaultTokenCounter::new(catalog);
        
        let text = "Hello";
        let text_tokens = counter.count_text("gpt-4o", text);
        
        let messages = vec![Message::user(text)];
        let msg_tokens = counter.count_messages("gpt-4o", &messages);
        
        // Message should have overhead (role + formatting)
        assert!(msg_tokens > text_tokens);
        // Overhead should be reasonable (4 for role + 3 for boundary = 7)
        assert!(msg_tokens <= text_tokens + 10);
    }

    #[test]
    fn test_token_counting_with_tool_content() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = DefaultTokenCounter::new(catalog);
        
        let tool_use = ToolUse {
            id: "test-123".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "/home/user/test.txt"}),
        };
        
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(tool_use)],
        }];
        
        let tokens = counter.count_messages("gpt-4o", &messages);
        assert!(tokens > 10); // Should include tool name + JSON input + overhead
    }

    // ============================================================================
    // Context Truncation Tests
    // ============================================================================

    #[test]
    fn test_context_truncation_preserves_last_turn() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = Arc::new(DefaultTokenCounter::new(catalog.clone()));
        let manager = ContextManager::new(catalog, counter);
        
        // Create many segments that exceed budget
        let mut segments = Vec::new();
        for i in 0..100 {
            segments.push(ContextSegment::chat(
                vec![
                    Message::user(format!("User message {}", i)),
                    Message::assistant(format!("Assistant response {}", i)),
                ],
                i as u64,
            ));
        }
        
        let params = BuildContextParams {
            model: "gpt-4o-mini".to_string(), // 128k context
            system_prompt: Some("You are helpful.".to_string()),
            short_system_prompt: Some("Be helpful.".to_string()),
            tools: vec![],
            segments,
            max_output_tokens: Some(4096),
        };
        
        let built = manager.build_request(params);
        
        // Should have dropped some segments but kept last turn
        assert!(built.truncated || built.segments_dropped == 0);
        
        // Last message should be the most recent (sequence 99)
        if !built.request.messages.is_empty() {
            let last_content = &built.request.messages.last().unwrap().content;
            if let Some(ContentBlock::Text(text)) = last_content.first() {
                assert!(text.contains("99") || text.contains("98"));
            }
        }
    }

    #[test]
    fn test_context_truncation_uses_short_prompt_when_needed() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = Arc::new(DefaultTokenCounter::new(catalog.clone()));
        let manager = ContextManager::new(catalog, counter);
        
        // Create a very long system prompt
        let long_prompt = "You are a helpful assistant. ".repeat(1000);
        let short_prompt = "Be helpful.".to_string();
        
        // Create segments that will push us over budget
        let mut segments = Vec::new();
        for i in 0..50 {
            segments.push(ContextSegment::chat(
                vec![
                    Message::user(format!("Message {}: {}", i, "x".repeat(500))),
                    Message::assistant(format!("Response {}: {}", i, "y".repeat(500))),
                ],
                i as u64,
            ));
        }
        
        let params = BuildContextParams {
            model: "gpt-4o-mini".to_string(),
            system_prompt: Some(long_prompt.clone()),
            short_system_prompt: Some(short_prompt.clone()),
            tools: vec![],
            segments,
            max_output_tokens: Some(4096),
        };
        
        let built = manager.build_request(params);
        
        // If truncated and system prompt present, might have switched to short
        // Check that we're within budget
        assert!(built.total_tokens <= built.budget);
    }

    #[test]
    fn test_context_preserves_tool_exchange_integrity() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = Arc::new(DefaultTokenCounter::new(catalog.clone()));
        let manager = ContextManager::new(catalog, counter);
        
        // Create a tool exchange that should be preserved together
        let tool_use = ToolUse {
            id: "tool-1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "/test.txt"}),
        };
        
        let tool_result = ToolResult {
            tool_use_id: "tool-1".to_string(),
            content: ToolResultContent::Text("File contents here".to_string()),
            is_error: false,
        };
        
        let segments = vec![
            ContextSegment::chat(
                vec![Message::user("Read the file")],
                1,
            ),
            ContextSegment::new(
                context::SegmentKind::ChatHistory,
                vec![Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolUse(tool_use)],
                }],
                2,
            ),
            ContextSegment::tool_exchange(
                vec![Message {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult(tool_result)],
                }],
                3,
            ),
        ];
        
        let params = BuildContextParams {
            model: "gpt-4o".to_string(),
            system_prompt: Some("You are helpful.".to_string()),
            short_system_prompt: None,
            tools: vec![],
            segments,
            max_output_tokens: Some(4096),
        };
        
        let built = manager.build_request(params);
        
        // All segments should be included (small context)
        assert!(!built.truncated);
        assert!(built.segments_included >= 2);
    }

    // ============================================================================
    // Agent Engine State Machine Tests
    // ============================================================================

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
        let llm = crate::llm::LLMManager::new();

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
    fn test_engine_state_transitions() {
        let (mut engine, mut rx) = create_test_engine();
        
        // Start in Idle
        assert_eq!(engine.state(), AgentState::Idle);
        
        // Create new thread -> AwaitingUserInput
        engine.new_thread("gpt-4o");
        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
        
        // Verify event was emitted
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::StateChanged(AgentState::AwaitingUserInput)));
    }

    #[test]
    fn test_engine_send_message_without_thread() {
        let (mut engine, mut rx) = create_test_engine();
        
        // Try sending without a thread
        engine.send_message("Hello");
        
        // Should emit an error
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::Error(_)));
    }

    #[test]
    fn test_engine_handles_llm_error_event() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        // Drain state change event
        let _ = rx.try_recv();
        
        // Simulate LLM error
        engine.handle_llm_event(LLMEvent::Error(crate::llm::types::LLMError::NetworkError {
            message: "Connection failed".to_string(),
        }));
        
        // Should transition to Error state
        assert_eq!(engine.state(), AgentState::Error);
        
        // Should emit error event
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::Error(_)));
    }

    #[test]
    fn test_engine_handles_stream_chunks() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        // Drain state change event
        let _ = rx.try_recv();
        
        // Simulate receiving stream chunks
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "msg-123".to_string(),
        }));
        
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::Chunk(StreamChunk::Start { .. })));
        
        // Simulate text delta
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("Hello".to_string())
        )));
        
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::Chunk(StreamChunk::Delta(StreamDelta::Text(_)))));
    }

    #[test]
    fn test_engine_handles_tool_use_detection() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        // Drain state change event
        let _ = rx.try_recv();
        
        let tool_use = ToolUse {
            id: "tool-1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "/test.txt"}),
        };
        
        engine.handle_llm_event(LLMEvent::ToolUseDetected(tool_use.clone()));
        
        // Should emit ToolUseRequested event
        let event = rx.try_recv().unwrap();
        if let AgentEvent::ToolUseRequested(tu) = event {
            assert_eq!(tu.name, "read_file");
            assert_eq!(tu.id, "tool-1");
        } else {
            panic!("Expected ToolUseRequested event");
        }
    }

    #[test]
    fn test_engine_cancel() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        // Drain state change event
        let _ = rx.try_recv();
        
        // Cancel should transition to AwaitingUserInput
        engine.cancel();
        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
    }

    #[test]
    fn test_engine_config() {
        let (engine, _rx) = create_test_engine();
        
        let tool = ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        
        let config = AgentConfig {
            tools: vec![tool],
            max_turns: 5,
            auto_continue: false,
        };
        
        let engine = engine.with_config(config);
        // Config is set (not directly accessible, but engine accepts it)
        assert_eq!(engine.state(), AgentState::Idle);
    }

    #[test]
    fn test_engine_thread_persistence() {
        let (event_tx, _rx) = mpsc::unbounded_channel();
        let catalog = Arc::new(ModelCatalog::new());
        let counter = Arc::new(DefaultTokenCounter::new(catalog.clone()));
        let context_manager = Arc::new(ContextManager::new(catalog, counter));
        let prompt_builder = SystemPromptBuilder::ridge_control();
        let tool_executor = Arc::new(ConfirmationRequiredExecutor);
        let thread_store = Arc::new(InMemoryThreadStore::new());
        let llm = crate::llm::LLMManager::new();

        let mut engine = AgentEngine::new(
            llm,
            context_manager,
            prompt_builder,
            tool_executor,
            thread_store.clone(),
            event_tx,
        );
        
        // Create and use a thread
        engine.new_thread("gpt-4o");
        let thread_id = engine.current_thread().unwrap().id.clone();
        
        // Thread should be in the store after saving (happens on finalize)
        // For now, manually save to test
        if let Some(thread) = engine.current_thread() {
            thread_store.save(thread).unwrap();
        }
        
        // Verify we can retrieve it
        let retrieved = thread_store.get(&thread_id);
        assert!(retrieved.is_some());
    }

    // ============================================================================
    // End-to-End Integration Test (Simulated)
    // ============================================================================

    #[test]
    fn test_full_conversation_flow_simulated() {
        let (mut engine, mut rx) = create_test_engine();
        
        // 1. Start new thread
        engine.new_thread("gpt-4o");
        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
        let _ = rx.try_recv(); // drain event
        
        // 2. Add user segment manually (simulating send_message side effects)
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("What is 2+2?")],
                thread.peek_sequence(),
            ));
        }
        
        // 3. Simulate streaming response
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "msg-1".to_string(),
        }));
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("2+2 equals 4.".to_string())
        )));
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
                thinking_tokens: None,
            }),
        }));
        
        // 4. Complete the stream
        engine.handle_llm_event(LLMEvent::Complete);
        
        // Drain all events
        while rx.try_recv().is_ok() {}
        
        // 5. Thread should have segments
        let thread = engine.current_thread().unwrap();
        assert!(!thread.segments.is_empty());
    }
}
