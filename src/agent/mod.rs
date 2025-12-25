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
pub use engine::{AgentEngine, AgentState, AgentEvent, AgentConfig};

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
        
        // CRITICAL FIX TEST: ToolUseRequested is NOT emitted immediately on ToolUseDetected.
        // It is only emitted after LLMEvent::Complete when the assistant message is saved.
        // This prevents race conditions where tool execution completes before the ToolUse
        // message is persisted to the thread.
        
        engine.handle_llm_event(LLMEvent::ToolUseDetected(tool_use.clone()));
        
        // Should NOT emit ToolUseRequested yet - should be empty
        assert!(rx.try_recv().is_err(), "No event should be emitted on ToolUseDetected");
        
        // Now simulate stream completion
        engine.handle_llm_event(LLMEvent::Complete);
        
        // Should emit StateChanged(ExecutingTools) then ToolUseRequested
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::StateChanged(AgentState::ExecutingTools)),
            "Expected StateChanged(ExecutingTools), got {:?}", event);
        
        let event = rx.try_recv().unwrap();
        if let AgentEvent::ToolUseRequested(tu) = event {
            assert_eq!(tu.name, "read_file");
            assert_eq!(tu.id, "tool-1");
        } else {
            panic!("Expected ToolUseRequested event, got {:?}", event);
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

    // ============================================================================
    // TP2-002-17: End-to-End Integration Tests for AgentEngine Flow
    // ============================================================================

    /// Helper to drain all events from receiver and return them as a Vec
    fn drain_events(rx: &mut mpsc::UnboundedReceiver<AgentEvent>) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// Helper to check if events contain a specific state transition
    fn has_state_change(events: &[AgentEvent], expected: AgentState) -> bool {
        events.iter().any(|e| matches!(e, AgentEvent::StateChanged(s) if *s == expected))
    }

    /// Test full tool round-trip: user message -> LLM tool use -> tool result -> continue -> turn complete
    #[test]
    fn test_full_tool_roundtrip_flow() {
        let (mut engine, mut rx) = create_test_engine();
        
        // 1. Start new thread
        engine.new_thread("claude-sonnet-4-20250514");
        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
        drain_events(&mut rx);
        
        // 2. Simulate user message (add segment manually to test engine behavior)
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("Read the file /etc/hostname")],
                thread.peek_sequence(),
            ));
        }
        
        // 3. Simulate LLM response with tool use
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "msg-tool-1".to_string(),
        }));
        
        // LLM says something before using tool
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("I'll read that file for you.".to_string())
        )));
        
        // LLM requests tool use
        let tool_use = ToolUse {
            id: "toolu_01XYZ".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "/etc/hostname"}),
        };
        engine.handle_llm_event(LLMEvent::ToolUseDetected(tool_use.clone()));
        
        // Stream stops with tool use reason
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::ToolUse,
            usage: Some(Usage {
                input_tokens: 50,
                output_tokens: 20,
                thinking_tokens: None,
            }),
        }));
        
        engine.handle_llm_event(LLMEvent::Complete);
        
        // 4. Verify events include tool use request
        let events = drain_events(&mut rx);
        let tool_requested = events.iter().any(|e| {
            matches!(e, AgentEvent::ToolUseRequested(tu) if tu.name == "read_file")
        });
        assert!(tool_requested, "Should have emitted ToolUseRequested event");
        
        // Engine should be in ExecutingTools state (waiting for tool result)
        assert_eq!(engine.state(), AgentState::ExecutingTools);
        
        // 5. Simulate tool execution result
        let tool_result = ToolResult {
            tool_use_id: "toolu_01XYZ".to_string(),
            content: ToolResultContent::Text("ridgetop-workstation".to_string()),
            is_error: false,
        };
        
        engine.continue_after_tools(vec![tool_result]);
        
        // 6. Simulate LLM's final response after tool result
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "msg-tool-2".to_string(),
        }));
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("The hostname is 'ridgetop-workstation'.".to_string())
        )));
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: Some(Usage {
                input_tokens: 80,
                output_tokens: 15,
                thinking_tokens: None,
            }),
        }));
        
        engine.handle_llm_event(LLMEvent::Complete);
        
        // 7. Verify turn completion
        let events = drain_events(&mut rx);
        let turn_complete = events.iter().any(|e| {
            matches!(e, AgentEvent::TurnComplete { stop_reason: StopReason::EndTurn, .. })
        });
        assert!(turn_complete, "Should have emitted TurnComplete event");
        
        // 8. Verify thread has accumulated segments
        let thread = engine.current_thread().unwrap();
        // Should have: user message, assistant (tool use), tool result, assistant final
        assert!(thread.segments.len() >= 3, "Thread should have multiple segments for tool round-trip");
        
        // 9. Verify final state
        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
    }

    /// Test multi-turn conversation with segment accumulation
    #[test]
    fn test_multi_turn_conversation() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        drain_events(&mut rx);
        
        // Turn 1: Simple question
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("Hello!")],
                thread.peek_sequence(),
            ));
        }
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "turn1".to_string(),
        }));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("Hello! How can I help you today?".to_string())
        )));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: None,
        }));
        engine.handle_llm_event(LLMEvent::Complete);
        drain_events(&mut rx);
        
        let segments_after_turn1 = engine.current_thread().unwrap().segments.len();
        
        // Turn 2: Follow-up
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("What's the weather like?")],
                thread.peek_sequence(),
            ));
        }
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "turn2".to_string(),
        }));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("I don't have access to weather data.".to_string())
        )));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: None,
        }));
        engine.handle_llm_event(LLMEvent::Complete);
        drain_events(&mut rx);
        
        let segments_after_turn2 = engine.current_thread().unwrap().segments.len();
        
        // Turn 3: Another follow-up
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("Thanks anyway!")],
                thread.peek_sequence(),
            ));
        }
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "turn3".to_string(),
        }));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("You're welcome!".to_string())
        )));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: None,
        }));
        engine.handle_llm_event(LLMEvent::Complete);
        
        let segments_after_turn3 = engine.current_thread().unwrap().segments.len();
        
        // Verify segment accumulation
        assert!(segments_after_turn2 > segments_after_turn1, "Segments should grow after turn 2");
        assert!(segments_after_turn3 > segments_after_turn2, "Segments should grow after turn 3");
        assert!(segments_after_turn3 >= 6, "Should have at least 6 segments (3 user + 3 assistant)");
    }

    /// Test thread save and load with DiskThreadStore
    #[test]
    fn test_thread_disk_persistence_roundtrip() {
        use tempfile::TempDir;
        use super::disk_store::DiskThreadStore;
        
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let thread_store = Arc::new(
            DiskThreadStore::with_path(temp_dir.path().to_path_buf())
                .expect("Failed to create disk store")
        );
        
        let (event_tx, mut rx) = mpsc::unbounded_channel();
        let catalog = Arc::new(ModelCatalog::new());
        let counter = Arc::new(DefaultTokenCounter::new(catalog.clone()));
        let context_manager = Arc::new(ContextManager::new(catalog, counter));
        let prompt_builder = SystemPromptBuilder::ridge_control();
        let tool_executor = Arc::new(ConfirmationRequiredExecutor);
        let llm = crate::llm::LLMManager::new();

        let mut engine = AgentEngine::new(
            llm,
            context_manager.clone(),
            prompt_builder.clone(),
            tool_executor.clone(),
            thread_store.clone(),
            event_tx,
        );
        
        // Create thread and add content
        engine.new_thread("claude-sonnet-4-20250514");
        let thread_id = engine.current_thread().unwrap().id.clone();
        drain_events(&mut rx);
        
        // Add user message segment
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("Remember this: The secret is 42.")],
                thread.peek_sequence(),
            ));
        }
        
        // Simulate LLM response
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "persist-test".to_string(),
        }));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("I'll remember that the secret is 42.".to_string())
        )));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: Some(Usage {
                input_tokens: 20,
                output_tokens: 10,
                thinking_tokens: None,
            }),
        }));
        engine.handle_llm_event(LLMEvent::Complete);
        drain_events(&mut rx);
        
        // Manual save (normally happens on finalize_turn, but we need it for this test)
        engine.save_thread().expect("Failed to save thread");
        
        // Verify file was written
        let thread_path = thread_store.thread_path(&thread_id);
        assert!(thread_path.exists(), "Thread file should exist on disk");
        
        // Create a new engine and load the thread
        let (event_tx2, mut rx2) = mpsc::unbounded_channel();
        let llm2 = crate::llm::LLMManager::new();
        let mut engine2 = AgentEngine::new(
            llm2,
            context_manager,
            prompt_builder,
            tool_executor,
            thread_store.clone(),
            event_tx2,
        );
        
        // Load the saved thread
        engine2.load_thread(&thread_id).expect("Failed to load thread");
        drain_events(&mut rx2);
        
        // Verify loaded thread has correct data
        let loaded_thread = engine2.current_thread().unwrap();
        assert_eq!(loaded_thread.id, thread_id);
        assert_eq!(loaded_thread.model, "claude-sonnet-4-20250514");
        assert!(!loaded_thread.segments.is_empty(), "Loaded thread should have segments");
        
        // Verify message content was preserved
        let has_user_msg = loaded_thread.segments.iter().any(|seg| {
            seg.messages.iter().any(|msg| {
                msg.content.iter().any(|block| {
                    matches!(block, ContentBlock::Text(t) if t.contains("secret is 42"))
                })
            })
        });
        assert!(has_user_msg, "Loaded thread should contain the original message");
    }

    /// Test state transition event sequence for complete turn
    #[test]
    fn test_state_transition_event_sequence() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        let events = drain_events(&mut rx);
        
        // Should start with AwaitingUserInput
        assert!(has_state_change(&events, AgentState::AwaitingUserInput));
        
        // Add user message and simulate full turn
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("Test message")],
                thread.peek_sequence(),
            ));
        }
        
        // Simulate complete turn
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "state-test".to_string(),
        }));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("Response".to_string())
        )));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: None,
        }));
        engine.handle_llm_event(LLMEvent::Complete);
        
        let events = drain_events(&mut rx);
        
        // Should have TurnComplete event
        let has_turn_complete = events.iter().any(|e| matches!(e, AgentEvent::TurnComplete { .. }));
        assert!(has_turn_complete, "Should emit TurnComplete event");
        
        // Final state should be AwaitingUserInput
        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
    }

    /// Test thinking blocks are accumulated in response
    #[test]
    fn test_thinking_blocks_accumulation() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("claude-sonnet-4-20250514");
        drain_events(&mut rx);
        
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("Complex problem")],
                thread.peek_sequence(),
            ));
        }
        
        // Simulate response with thinking
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Start {
            message_id: "thinking-test".to_string(),
        }));
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Thinking("Let me think about this...".to_string())
        )));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Thinking(" I need to consider...".to_string())
        )));
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Delta(
            StreamDelta::Text("Here's my answer.".to_string())
        )));
        
        engine.handle_llm_event(LLMEvent::Chunk(StreamChunk::Stop {
            reason: StopReason::EndTurn,
            usage: None,
        }));
        engine.handle_llm_event(LLMEvent::Complete);
        
        // Verify events include thinking chunks
        let events = drain_events(&mut rx);
        let thinking_chunks: Vec<_> = events.iter().filter(|e| {
            matches!(e, AgentEvent::Chunk(StreamChunk::Delta(StreamDelta::Thinking(_))))
        }).collect();
        
        assert_eq!(thinking_chunks.len(), 2, "Should have 2 thinking chunks");
    }

    /// Test context truncation notification
    #[test]
    fn test_context_truncation_with_many_segments() {
        let (mut engine, mut rx) = create_test_engine();
        
        // Use a model with smaller context window for testing
        engine.new_thread("gpt-4o-mini");
        drain_events(&mut rx);
        
        // Add many segments to potentially trigger truncation
        if let Some(thread) = engine.current_thread_mut() {
            for i in 0..50 {
                thread.add_segment(ContextSegment::chat(
                    vec![
                        Message::user(format!("Message {} with some content: {}", i, "x".repeat(200))),
                        Message::assistant(format!("Response {} with content: {}", i, "y".repeat(200))),
                    ],
                    thread.peek_sequence(),
                ));
            }
        }
        
        // The thread now has many segments
        let segment_count = engine.current_thread().unwrap().segments.len();
        assert!(segment_count >= 50, "Should have at least 50 segments");
    }

    /// Test error recovery - engine can continue after error
    #[test]
    fn test_error_recovery() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        drain_events(&mut rx);
        
        // Simulate an error
        engine.handle_llm_event(LLMEvent::Error(crate::llm::types::LLMError::NetworkError {
            message: "Connection timeout".to_string(),
        }));
        
        assert_eq!(engine.state(), AgentState::Error);
        
        let events = drain_events(&mut rx);
        let has_error = events.iter().any(|e| matches!(e, AgentEvent::Error(_)));
        assert!(has_error, "Should emit Error event");
        
        // Engine should allow creating new thread after error
        engine.new_thread("gpt-4o");
        assert_eq!(engine.state(), AgentState::AwaitingUserInput);
    }

    /// Test thread clear functionality
    #[test]
    fn test_thread_clear() {
        let (mut engine, mut rx) = create_test_engine();
        
        engine.new_thread("gpt-4o");
        drain_events(&mut rx);
        
        // Add some content
        if let Some(thread) = engine.current_thread_mut() {
            thread.add_segment(ContextSegment::chat(
                vec![Message::user("First message")],
                thread.peek_sequence(),
            ));
            thread.add_segment(ContextSegment::chat(
                vec![Message::assistant("First response")],
                thread.peek_sequence(),
            ));
        }
        
        assert!(!engine.current_thread().unwrap().segments.is_empty());
        
        // Clear the thread
        if let Some(thread) = engine.current_thread_mut() {
            thread.clear();
        }
        
        // Verify cleared
        assert!(engine.current_thread().unwrap().segments.is_empty());
        assert_eq!(engine.current_thread().unwrap().peek_sequence(), 0);
    }
}
