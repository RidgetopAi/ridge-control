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
pub mod tools;
pub mod engine;

pub use models::{ModelInfo, ModelCatalog, TokenizerKind};
pub use tokens::{TokenCounter, DefaultTokenCounter};
pub use context::{ContextSegment, SegmentKind, ContextManager, BuildContextParams, BuiltContext};
pub use prompt::{SystemPromptBuilder, PlatformInfo, RepoContextInfo};
pub use thread::{AgentThread, ThreadStore, InMemoryThreadStore};
pub use tools::ToolExecutor;
pub use engine::{AgentEngine, AgentState, AgentEvent};
