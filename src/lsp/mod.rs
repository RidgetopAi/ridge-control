//! Language Server Protocol client infrastructure
//!
//! This module provides LSP client capabilities for ridge-control,
//! enabling semantic code navigation for the LLM agent.
//!
//! # Components
//!
//! - [`types`] - LSP type definitions (Position, Range, Location, etc.)
//! - [`protocol`] - JSON-RPC message handling
//! - [`client`] - Per-server LSP client
//! - [`manager`] - Multi-server lifecycle management
//! - [`document`] - Document synchronization tracking
//!
//! # Supported Languages
//!
//! - TypeScript/JavaScript (typescript-language-server)
//! - Rust (rust-analyzer)
//! - Python (pyright-langserver)

mod types;
mod protocol;
mod client;
mod manager;
mod document;

// Re-export only what's needed externally
pub use manager::LspManager;
