// Tool execution - some types for future features
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::timeout;

use super::types::{ToolDefinition, ToolResult, ToolResultContent, ToolUse};
use crate::agent::mandrel::MandrelClient;

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

    #[error("Mandrel not configured")]
    MandrelNotConfigured,

    #[error("Mandrel error: {0}")]
    MandrelError(String),

    #[error("Waiting for user input")]
    WaitingForUserInput {
        /// Tool use ID for matching response
        tool_use_id: String,
        /// Parsed questions for the dialog
        questions: Vec<ParsedQuestion>,
    },
}

/// Parsed question for ask_user tool
#[derive(Debug, Clone)]
pub struct ParsedQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<ParsedOption>,
    pub multi_select: bool,
}

/// Parsed option for ask_user tool
#[derive(Debug, Clone)]
pub struct ParsedOption {
    pub label: String,
    pub description: String,
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

        // Grep - search tool, safe, read-only
        self.policies.insert("grep".to_string(), ToolPolicy {
            name: "grep".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 524_288, // 512KB for search results
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });

        // Glob - file discovery, safe, read-only
        self.policies.insert("glob".to_string(), ToolPolicy {
            name: "glob".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 15,
            max_output_bytes: 102_400,
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });

        // Tree - directory structure, safe, read-only
        self.policies.insert("tree".to_string(), ToolPolicy {
            name: "tree".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 15,
            max_output_bytes: 102_400,
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });

        // find_symbol - code symbol search via ctags, safe, read-only
        self.policies.insert("find_symbol".to_string(), ToolPolicy {
            name: "find_symbol".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 524_288, // 512KB for search results
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });

        // ast_search - structural code search via ast-grep, safe, read-only
        self.policies.insert("ast_search".to_string(), ToolPolicy {
            name: "ast_search".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 524_288, // 512KB for search results
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });

        // edit - surgical string replacement, requires confirmation (modifies files)
        self.policies.insert("edit".to_string(), ToolPolicy {
            name: "edit".to_string(),
            require_confirmation: true,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 102_400,
            allowed_paths: vec!["~/".to_string(), "/tmp/".to_string()],
        });

        // task - spawn sub-agents, no confirmation needed (sub-agents have their own tool restrictions)
        self.policies.insert("task".to_string(), ToolPolicy {
            name: "task".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 300, // 5 minutes for sub-agent execution
            max_output_bytes: 1_048_576, // 1MB for sub-agent results
            allowed_paths: vec![], // No path restrictions
        });

        // ─────────────────────────────────────────────────────────────────────
        // Mandrel Cross-Session Memory Tools
        // ─────────────────────────────────────────────────────────────────────

        // project_switch - switch active Mandrel project
        self.policies.insert("project_switch".to_string(), ToolPolicy {
            name: "project_switch".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 8192,
            allowed_paths: vec![],
        });

        // project_current - get current project info
        self.policies.insert("project_current".to_string(), ToolPolicy {
            name: "project_current".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 8192,
            allowed_paths: vec![],
        });

        // context_store - store context for cross-session memory
        self.policies.insert("context_store".to_string(), ToolPolicy {
            name: "context_store".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 8192,
            allowed_paths: vec![],
        });

        // context_get_recent - get recent contexts
        self.policies.insert("context_get_recent".to_string(), ToolPolicy {
            name: "context_get_recent".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 102_400, // 100KB for context list
            allowed_paths: vec![],
        });

        // context_search - semantic search contexts
        self.policies.insert("context_search".to_string(), ToolPolicy {
            name: "context_search".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 102_400, // 100KB for search results
            allowed_paths: vec![],
        });

        // task_create - create task in Mandrel
        self.policies.insert("task_create".to_string(), ToolPolicy {
            name: "task_create".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 8192,
            allowed_paths: vec![],
        });

        // task_update - update task status
        self.policies.insert("task_update".to_string(), ToolPolicy {
            name: "task_update".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 8192,
            allowed_paths: vec![],
        });

        // task_list - list tasks
        self.policies.insert("task_list".to_string(), ToolPolicy {
            name: "task_list".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 102_400, // 100KB for task list
            allowed_paths: vec![],
        });

        // task_details - get task details
        self.policies.insert("task_details".to_string(), ToolPolicy {
            name: "task_details".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 16384,
            allowed_paths: vec![],
        });

        // task_progress_summary - get progress overview
        self.policies.insert("task_progress_summary".to_string(), ToolPolicy {
            name: "task_progress_summary".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 30,
            max_output_bytes: 16384,
            allowed_paths: vec![],
        });

        // smart_search - cross-entity intelligent search
        self.policies.insert("smart_search".to_string(), ToolPolicy {
            name: "smart_search".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 60, // Can be slower for semantic search
            max_output_bytes: 102_400, // 100KB for search results
            allowed_paths: vec![],
        });

        // ─────────────────────────────────────────────────────────────────────
        // User Interaction Tools
        // ─────────────────────────────────────────────────────────────────────

        // ask_user - present questions to the user and get responses
        self.policies.insert("ask_user".to_string(), ToolPolicy {
            name: "ask_user".to_string(),
            require_confirmation: false,
            dangerous_mode_only: false,
            timeout_secs: 300, // 5 minutes to wait for user response
            max_output_bytes: 16384,
            allowed_paths: vec![],
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
                description: "Read contents of a file. Supports reading specific line ranges for efficiency.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to read"
                        },
                        "start_line": {
                            "type": "integer",
                            "description": "First line to read (1-indexed, default: 1)"
                        },
                        "end_line": {
                            "type": "integer",
                            "description": "Last line to read (inclusive, default: end of file)"
                        },
                        "max_lines": {
                            "type": "integer",
                            "description": "Maximum lines to return (default: 500)"
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
                description: "Execute a bash command".to_string(),
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
            ToolDefinition {
                name: "file_delete".to_string(),
                description: "Delete a file".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to delete"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "grep".to_string(),
                description: "Search for text patterns in files. Returns matches with line numbers and context. Uses ripgrep for fast searching.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "The text or regex pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory or file to search in (default: current directory)"
                        },
                        "include": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Glob patterns to include (e.g. ['*.rs', '*.toml'])"
                        },
                        "exclude": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Glob patterns to exclude (e.g. ['target/**', 'node_modules/**'])"
                        },
                        "literal": {
                            "type": "boolean",
                            "description": "Treat pattern as literal text, not regex (default: false)"
                        },
                        "case_sensitive": {
                            "type": "boolean",
                            "description": "Case sensitive matching (default: true)"
                        },
                        "context_lines": {
                            "type": "integer",
                            "description": "Lines of context before/after match (default: 2)"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum matches to return (default: 50)"
                        },
                        "output_mode": {
                            "type": "string",
                            "enum": ["content", "files_with_matches", "count"],
                            "default": "files_with_matches",
                            "description": "Output format: 'content' for full lines with context, 'files_with_matches' for paths only (default), 'count' for match counts per file"
                        },
                        "head_limit": {
                            "type": "integer",
                            "description": "Limit results to first N entries (default: no limit, use max_results for content mode)"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Skip first N entries for pagination (default: 0)"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "glob".to_string(),
                description: "Find files matching a glob pattern. Returns file paths with metadata. Use to discover files before reading or searching.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Glob pattern (e.g. 'src/**/*.rs', '**/test_*.py', '*.json')"
                        },
                        "path": {
                            "type": "string",
                            "description": "Base directory to search from (default: current directory)"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum files to return (default: 100)"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "tree".to_string(),
                description: "Show directory structure as a tree. Useful for understanding project layout.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Root directory (default: current directory)"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "Maximum depth to traverse (default: 3)"
                        },
                        "show_hidden": {
                            "type": "boolean",
                            "description": "Show hidden files/directories (default: false)"
                        },
                        "dirs_only": {
                            "type": "boolean",
                            "description": "Show only directories, not files (default: false)"
                        }
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "find_symbol".to_string(),
                description: "Find code symbol definitions (functions, classes, methods, structs, etc.) by name. Uses ctags for fast, language-agnostic symbol search. Returns structured results with file paths and line numbers.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Symbol name or pattern to search for (supports partial matching)"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory to search in (default: current directory)"
                        },
                        "kind": {
                            "type": "string",
                            "description": "Filter by symbol kind: function, class, struct, method, interface, type, const, variable, module, enum, trait, impl (optional)"
                        },
                        "exact": {
                            "type": "boolean",
                            "description": "Require exact name match instead of substring (default: false)"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum symbols to return (default: 50)"
                        }
                    },
                    "required": ["name"]
                }),
            },
            ToolDefinition {
                name: "ast_search".to_string(),
                description: "Search code using structural AST patterns (tree-sitter based). More accurate than text search for finding function definitions, method calls, and code patterns. Examples: '$FUNC($ARGS)' matches function calls, 'fn $NAME($$$)' matches Rust function definitions, '$EXPR.unwrap()' matches unwrap calls.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "AST pattern to search for. Use $NAME for single nodes, $$$ for multiple nodes. Examples: 'fn $NAME($$$) { $$$ }' for function defs, '$EXPR.unwrap()' for unwrap calls, 'struct $NAME { $$$ }' for struct definitions"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory or file to search in (default: current directory)"
                        },
                        "lang": {
                            "type": "string",
                            "enum": ["rust", "typescript", "javascript", "python", "go", "c", "cpp", "java", "tsx", "jsx"],
                            "description": "Language to parse as (auto-detected from file extension if not specified)"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum matches to return (default: 30)"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "edit".to_string(),
                description: "Replace exact string in a file. More efficient than rewriting entire file. The old_string must match exactly (including whitespace and indentation). Use replace_all to change all occurrences, otherwise old_string must be unique in the file.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Path to the file to edit"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "Exact string to find and replace (must be unique unless replace_all=true)"
                        },
                        "new_string": {
                            "type": "string",
                            "description": "Replacement string"
                        },
                        "replace_all": {
                            "type": "boolean",
                            "default": false,
                            "description": "Replace all occurrences instead of requiring unique match (default: false)"
                        }
                    },
                    "required": ["file_path", "old_string", "new_string"]
                }),
            },
            ToolDefinition {
                name: "task".to_string(),
                description: "Spawn a sub-agent to handle complex tasks autonomously. \
                    Use 'explore' for codebase research and file discovery, \
                    'plan' for architecture decisions and implementation planning, \
                    'review' for code review. Sub-agents return summarized results.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "Task description for the sub-agent"
                        },
                        "agent_type": {
                            "type": "string",
                            "enum": ["explore", "plan", "review", "general"],
                            "default": "explore",
                            "description": "Type of sub-agent: explore (research), plan (architecture), review (code review)"
                        },
                        "run_in_background": {
                            "type": "boolean",
                            "default": false,
                            "description": "Run asynchronously and retrieve results later"
                        }
                    },
                    "required": ["prompt"]
                }),
            },
            // ─────────────────────────────────────────────────────────────────────
            // Mandrel Cross-Session Memory Tools
            // ─────────────────────────────────────────────────────────────────────
            ToolDefinition {
                name: "project_switch".to_string(),
                description: "Switch to a different Mandrel project. Use this to set the context for cross-session memory operations.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "project": {
                            "type": "string",
                            "description": "Project name to switch to"
                        }
                    },
                    "required": ["project"]
                }),
            },
            ToolDefinition {
                name: "project_current".to_string(),
                description: "Get information about the currently active Mandrel project.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "context_store".to_string(),
                description: "Store context in Mandrel for cross-session memory. Use for important findings, \
                    decisions, completions, handoff notes, or any information that should persist across sessions. \
                    Contexts are automatically embedded for semantic search.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "The context content to store"
                        },
                        "type": {
                            "type": "string",
                            "enum": ["code", "decision", "error", "discussion", "planning", "completion", "milestone", "reflections", "handoff"],
                            "description": "Context type: code, decision, error, discussion, planning, completion, milestone, reflections, handoff"
                        },
                        "tags": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Optional tags for categorization (e.g., ['bug-fix', 'authentication'])"
                        }
                    },
                    "required": ["content", "type"]
                }),
            },
            ToolDefinition {
                name: "context_get_recent".to_string(),
                description: "Get recent contexts in chronological order (newest first). Use at session start \
                    for continuity or to review recent work.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "limit": {
                            "type": "integer",
                            "description": "Maximum contexts to return (default: 5)"
                        }
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "context_search".to_string(),
                description: "Search stored contexts using semantic similarity. Use before re-investigating \
                    something that may have been explored in previous sessions.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query using semantic similarity"
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolDefinition {
                name: "task_create".to_string(),
                description: "Create a new task in Mandrel for coordination and tracking. Tasks persist \
                    across sessions and can be updated as work progresses.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Task title"
                        },
                        "description": {
                            "type": "string",
                            "description": "Optional detailed description"
                        },
                        "priority": {
                            "type": "string",
                            "enum": ["low", "medium", "high", "critical"],
                            "description": "Task priority level"
                        }
                    },
                    "required": ["title"]
                }),
            },
            ToolDefinition {
                name: "task_update".to_string(),
                description: "Update a task's status. Use to mark tasks as in_progress, completed, blocked, or cancelled.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Task ID to update"
                        },
                        "status": {
                            "type": "string",
                            "enum": ["todo", "in_progress", "blocked", "completed", "cancelled"],
                            "description": "New status"
                        }
                    },
                    "required": ["task_id", "status"]
                }),
            },
            ToolDefinition {
                name: "task_list".to_string(),
                description: "List tasks with optional status filter. Use to see what work is pending or in progress.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "status": {
                            "type": "string",
                            "enum": ["todo", "in_progress", "blocked", "completed", "cancelled"],
                            "description": "Filter by status (optional)"
                        }
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "task_details".to_string(),
                description: "Get detailed information about a specific task including full description and history.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Task ID to get details for"
                        }
                    },
                    "required": ["task_id"]
                }),
            },
            ToolDefinition {
                name: "task_progress_summary".to_string(),
                description: "Get task progress summary with completion percentages and status breakdown.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "smart_search".to_string(),
                description: "Intelligent search across all Mandrel data sources (contexts, tasks, decisions). \
                    Use for broad discovery across the project's history.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }),
            },
            // ─────────────────────────────────────────────────────────────────────
            // User Interaction Tools
            // ─────────────────────────────────────────────────────────────────────
            ToolDefinition {
                name: "ask_user".to_string(),
                description: "Ask the user questions to gather preferences, clarify requirements, or get decisions. \
                    Use when you need input before proceeding. Each question can have 2-4 predefined options plus \
                    an 'Other' option for custom text input. Use multiSelect: true when choices aren't mutually exclusive.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "questions": {
                            "type": "array",
                            "description": "Questions to ask (1-4 questions)",
                            "minItems": 1,
                            "maxItems": 4,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "header": {
                                        "type": "string",
                                        "description": "Short label displayed as chip/tag (max 12 chars). Examples: 'Auth method', 'Library', 'Approach'",
                                        "maxLength": 12
                                    },
                                    "question": {
                                        "type": "string",
                                        "description": "The complete question to ask. Should be clear and end with '?'"
                                    },
                                    "options": {
                                        "type": "array",
                                        "description": "Available choices (2-4 options). Each should be distinct.",
                                        "minItems": 2,
                                        "maxItems": 4,
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "label": {
                                                    "type": "string",
                                                    "description": "Display text for this option (1-5 words)"
                                                },
                                                "description": {
                                                    "type": "string",
                                                    "description": "Explanation of what this option means"
                                                }
                                            },
                                            "required": ["label", "description"]
                                        }
                                    },
                                    "multiSelect": {
                                        "type": "boolean",
                                        "description": "Allow multiple selections (default: false)",
                                        "default": false
                                    }
                                },
                                "required": ["header", "question", "options"]
                            }
                        }
                    },
                    "required": ["questions"]
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
    /// Optional Mandrel client for cross-session memory
    mandrel_client: Option<Arc<RwLock<MandrelClient>>>,
}

impl ToolExecutor {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            registry: ToolRegistry::new(),
            working_dir,
            mandrel_client: None,
        }
    }

    /// Set the Mandrel client for cross-session memory tools
    pub fn set_mandrel_client(&mut self, client: Arc<RwLock<MandrelClient>>) {
        self.mandrel_client = Some(client);
    }

    /// Check if Mandrel is available
    pub fn has_mandrel(&self) -> bool {
        self.mandrel_client.is_some()
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
    
    /// Get tool definitions for LLM requests
    pub fn tool_definitions_for_llm(&self) -> Vec<ToolDefinition> {
        self.registry.get_tool_definitions()
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
            // File operations
            "file_read" => self.execute_file_read(tool, policy).await,
            "file_write" => self.execute_file_write(tool, policy).await,
            "list_directory" => self.execute_list_directory(tool, policy).await,
            "bash_execute" => self.execute_bash(tool, policy).await,
            "file_delete" => self.execute_file_delete(tool, policy).await,
            // Search tools
            "grep" => self.execute_grep(tool, policy).await,
            "glob" => self.execute_glob(tool, policy).await,
            "tree" => self.execute_tree(tool, policy).await,
            "find_symbol" => self.execute_find_symbol(tool, policy).await,
            "ast_search" => self.execute_ast_search(tool, policy).await,
            "edit" => self.execute_edit(tool, policy).await,
            // Mandrel cross-session memory tools
            "project_switch" => self.execute_mandrel_project_switch(tool).await,
            "project_current" => self.execute_mandrel_project_current(tool).await,
            "context_store" => self.execute_mandrel_context_store(tool).await,
            "context_get_recent" => self.execute_mandrel_context_get_recent(tool).await,
            "context_search" => self.execute_mandrel_context_search(tool).await,
            "task_create" => self.execute_mandrel_task_create(tool).await,
            "task_update" => self.execute_mandrel_task_update(tool).await,
            "task_list" => self.execute_mandrel_task_list(tool).await,
            "task_details" => self.execute_mandrel_task_details(tool).await,
            "task_progress_summary" => self.execute_mandrel_task_progress_summary(tool).await,
            "smart_search" => self.execute_mandrel_smart_search(tool).await,
            // User interaction tools
            "ask_user" => self.execute_ask_user(tool).await,
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

        // Parse optional line range parameters
        let start_line = tool.input.get("start_line")
            .and_then(|v| v.as_i64())
            .map(|n| n.max(1) as usize)
            .unwrap_or(1);
        let end_line = tool.input.get("end_line")
            .and_then(|v| v.as_i64())
            .map(|n| n as usize);
        let max_lines = tool.input.get("max_lines")
            .and_then(|v| v.as_i64())
            .unwrap_or(500) as usize;
        
        let read_future = tokio::fs::read_to_string(&resolved);
        let content = timeout(Duration::from_secs(policy.timeout_secs), read_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::IoError(e.to_string()))?;

        // Process line range
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        
        // Calculate actual range
        let start_idx = (start_line - 1).min(total_lines);
        let end_idx = end_line.map(|e| e.min(total_lines)).unwrap_or(total_lines);
        let end_idx = end_idx.min(start_idx + max_lines);
        
        // Build output with line numbers
        let mut output = String::new();
        let truncated = end_idx < total_lines && end_line.is_none();
        
        for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
            let line_num = start_idx + i + 1;
            output.push_str(&format!("{:>4}: {}\n", line_num, line));
        }

        // Add metadata
        if start_line > 1 || end_idx < total_lines {
            output.push_str(&format!(
                "\n[Lines {}-{} of {} total]",
                start_idx + 1,
                end_idx,
                total_lines
            ));
            if truncated {
                output.push_str(&format!(" [TRUNCATED at {} lines]", max_lines));
            }
        }
        
        // Final size check
        if output.len() > policy.max_output_bytes {
            Ok(format!(
                "{}...\n\n[TRUNCATED: Output exceeds {} bytes]",
                &output[..policy.max_output_bytes],
                policy.max_output_bytes
            ))
        } else {
            Ok(output)
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

    async fn execute_grep(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let pattern = tool.input.get("pattern")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'pattern' parameter".to_string()))?;

        let search_path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .unwrap_or(".");
        let resolved = self.resolve_path(search_path);

        if !self.is_path_allowed("grep", &resolved) {
            return Err(ToolError::PathNotAllowed(search_path.to_string()));
        }

        let literal = tool.input.get("literal")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let case_sensitive = tool.input.get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let context_lines = tool.input.get("context_lines")
            .and_then(|v| v.as_i64())
            .unwrap_or(2) as usize;
        let max_results = tool.input.get("max_results")
            .and_then(|v| v.as_i64())
            .unwrap_or(50) as usize;

        // Parse output_mode - default to files_with_matches for efficiency
        let output_mode = tool.input.get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("files_with_matches");

        // Parse pagination parameters
        let head_limit = tool.input.get("head_limit")
            .and_then(|v| v.as_i64())
            .map(|v| v as usize);
        let offset = tool.input.get("offset")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as usize;

        // Build ripgrep command based on output mode
        let mut cmd = Command::new("rg");

        match output_mode {
            "content" => {
                // Full content mode: JSON output with context lines
                cmd.arg("--json")
                    .arg("--max-count").arg(max_results.to_string())
                    .arg("-C").arg(context_lines.to_string());
            }
            "files_with_matches" => {
                // Files only mode: just list matching file paths
                cmd.arg("--files-with-matches");
            }
            "count" => {
                // Count mode: show match count per file
                cmd.arg("--count");
            }
            _ => {
                return Err(ToolError::ParseError(
                    format!("Invalid output_mode: '{}'. Use 'content', 'files_with_matches', or 'count'", output_mode)
                ));
            }
        }

        if literal {
            cmd.arg("--fixed-strings");
        }
        if !case_sensitive {
            cmd.arg("--ignore-case");
        }

        // Handle include patterns
        if let Some(includes) = tool.input.get("include").and_then(|v| v.as_array()) {
            for inc in includes {
                if let Some(glob) = inc.as_str() {
                    cmd.arg("--glob").arg(glob);
                }
            }
        }

        // Handle exclude patterns
        if let Some(excludes) = tool.input.get("exclude").and_then(|v| v.as_array()) {
            for exc in excludes {
                if let Some(glob) = exc.as_str() {
                    cmd.arg("--glob").arg(format!("!{}", glob));
                }
            }
        }

        cmd.arg(pattern)
            .arg(&resolved)
            .current_dir(&self.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let exec_future = async {
            let child = cmd.spawn()?;
            child.wait_with_output().await
        };

        let output = timeout(Duration::from_secs(policy.timeout_secs), exec_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Format output based on mode
        let result_str = match output_mode {
            "content" => {
                // Parse ripgrep JSON output and format nicely
                let mut matches: Vec<serde_json::Value> = Vec::new();

                for line in stdout.lines() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                        if json.get("type").and_then(|t| t.as_str()) == Some("match") {
                            if let Some(data) = json.get("data") {
                                let path = data.get("path").and_then(|p| p.get("text")).and_then(|t| t.as_str()).unwrap_or("");
                                let line_num = data.get("line_number").and_then(|n| n.as_i64()).unwrap_or(0);
                                let text = data.get("lines").and_then(|l| l.get("text")).and_then(|t| t.as_str()).unwrap_or("");

                                matches.push(serde_json::json!({
                                    "path": path,
                                    "line": line_num,
                                    "text": text.trim_end()
                                }));
                            }
                        }
                    }
                }

                let total_matches = matches.len();

                // Apply pagination
                let paginated: Vec<serde_json::Value> = matches
                    .into_iter()
                    .skip(offset)
                    .take(head_limit.unwrap_or(usize::MAX))
                    .collect();

                let result = serde_json::json!({
                    "matches": paginated,
                    "total_matches": total_matches,
                    "offset": offset,
                    "returned": paginated.len(),
                    "truncated": total_matches >= max_results
                });

                serde_json::to_string_pretty(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            "files_with_matches" => {
                // Simple list of file paths
                let files: Vec<&str> = stdout.lines().collect();
                let total_files = files.len();

                // Apply pagination
                let paginated: Vec<&str> = files
                    .into_iter()
                    .skip(offset)
                    .take(head_limit.unwrap_or(usize::MAX))
                    .collect();

                let result = serde_json::json!({
                    "files": paginated,
                    "total_files": total_files,
                    "offset": offset,
                    "returned": paginated.len()
                });

                serde_json::to_string_pretty(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            "count" => {
                // Parse "path:count" format from ripgrep --count
                let mut counts: Vec<serde_json::Value> = Vec::new();
                let mut total_matches = 0usize;

                for line in stdout.lines() {
                    if let Some((path, count_str)) = line.rsplit_once(':') {
                        if let Ok(count) = count_str.parse::<usize>() {
                            counts.push(serde_json::json!({
                                "file": path,
                                "count": count
                            }));
                            total_matches += count;
                        }
                    }
                }

                let total_files = counts.len();

                // Apply pagination
                let paginated: Vec<serde_json::Value> = counts
                    .into_iter()
                    .skip(offset)
                    .take(head_limit.unwrap_or(usize::MAX))
                    .collect();

                let result = serde_json::json!({
                    "counts": paginated,
                    "total_files": total_files,
                    "total_matches": total_matches,
                    "offset": offset,
                    "returned": paginated.len()
                });

                serde_json::to_string_pretty(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            _ => unreachable!() // Already validated above
        };

        if result_str.len() > policy.max_output_bytes {
            Ok(format!(
                "{}...\n\n[TRUNCATED: Output exceeds {} bytes]",
                &result_str[..policy.max_output_bytes],
                policy.max_output_bytes
            ))
        } else {
            Ok(result_str)
        }
    }

    async fn execute_glob(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let pattern = tool.input.get("pattern")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'pattern' parameter".to_string()))?;

        let base_path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .unwrap_or(".");
        let resolved_base = self.resolve_path(base_path);

        if !self.is_path_allowed("glob", &resolved_base) {
            return Err(ToolError::PathNotAllowed(base_path.to_string()));
        }

        let max_results = tool.input.get("max_results")
            .and_then(|v| v.as_i64())
            .unwrap_or(100) as usize;

        // Combine base path with pattern
        let full_pattern = resolved_base.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();

        let glob_future = async {
            let mut files: Vec<serde_json::Value> = Vec::new();
            let mut count = 0;

            let entries = glob::glob(&pattern_str)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;

            for entry in entries {
                if count >= max_results {
                    break;
                }
                if let Ok(path) = entry {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        files.push(serde_json::json!({
                            "path": path.to_string_lossy(),
                            "size": metadata.len(),
                            "is_dir": metadata.is_dir()
                        }));
                        count += 1;
                    }
                }
            }

            Ok::<_, std::io::Error>((files, count >= max_results))
        };

        let (files, truncated) = timeout(Duration::from_secs(policy.timeout_secs), glob_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let result = serde_json::json!({
            "files": files,
            "total_found": files.len(),
            "truncated": truncated
        });

        serde_json::to_string_pretty(&result)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    async fn execute_tree(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let base_path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .unwrap_or(".");
        let resolved = self.resolve_path(base_path);

        if !self.is_path_allowed("tree", &resolved) {
            return Err(ToolError::PathNotAllowed(base_path.to_string()));
        }

        let max_depth = tool.input.get("depth")
            .and_then(|v| v.as_i64())
            .unwrap_or(3) as usize;
        let show_hidden = tool.input.get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let dirs_only = tool.input.get("dirs_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let tree_future = async {
            let mut output = String::new();
            Self::build_tree(&resolved, "", 0, max_depth, show_hidden, dirs_only, &mut output)?;
            Ok::<_, std::io::Error>(output)
        };

        let tree_output = timeout(Duration::from_secs(policy.timeout_secs), tree_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if tree_output.len() > policy.max_output_bytes {
            Ok(format!(
                "{}...\n\n[TRUNCATED: Output exceeds {} bytes]",
                &tree_output[..policy.max_output_bytes],
                policy.max_output_bytes
            ))
        } else {
            Ok(tree_output)
        }
    }

    fn build_tree(
        path: &Path,
        prefix: &str,
        depth: usize,
        max_depth: usize,
        show_hidden: bool,
        dirs_only: bool,
        output: &mut String,
    ) -> std::io::Result<()> {
        if depth > max_depth {
            return Ok(());
        }

        let mut entries: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                show_hidden || !name.starts_with('.')
            })
            .filter(|e| {
                if dirs_only {
                    e.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                } else {
                    true
                }
            })
            .collect();

        entries.sort_by(|a, b| {
            let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });

        let count = entries.len();
        for (i, entry) in entries.into_iter().enumerate() {
            let is_last = i == count - 1;
            let connector = if is_last { "└── " } else { "├── " };
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let suffix = if is_dir { "/" } else { "" };

            output.push_str(&format!("{}{}{}{}\n", prefix, connector, name, suffix));

            if is_dir && depth < max_depth {
                let new_prefix = if is_last {
                    format!("{}    ", prefix)
                } else {
                    format!("{}│   ", prefix)
                };
                let _ = Self::build_tree(
                    &entry.path(),
                    &new_prefix,
                    depth + 1,
                    max_depth,
                    show_hidden,
                    dirs_only,
                    output,
                );
            }
        }

        Ok(())
    }

    async fn execute_find_symbol(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let name = tool.input.get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'name' parameter".to_string()))?;

        let search_path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .unwrap_or(".");
        let resolved = self.resolve_path(search_path);

        if !self.is_path_allowed("find_symbol", &resolved) {
            return Err(ToolError::PathNotAllowed(search_path.to_string()));
        }

        let kind_filter = tool.input.get("kind")
            .and_then(|k| k.as_str())
            .map(|s| s.to_lowercase());
        let exact_match = tool.input.get("exact")
            .and_then(|e| e.as_bool())
            .unwrap_or(false);
        let max_results = tool.input.get("max_results")
            .and_then(|m| m.as_i64())
            .unwrap_or(50) as usize;

        // Build ctags command for on-the-fly parsing
        // Use --output-format=json for structured output
        let mut cmd = Command::new("ctags");
        cmd.arg("--output-format=json")
            .arg("--fields=+nKS")  // line number, kind (long), signature
            .arg("--extras=+q")    // qualified names
            .arg("-R")             // recursive
            .arg("-f").arg("-")    // output to stdout
            .arg(&resolved)
            .current_dir(&self.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let exec_future = async {
            let child = cmd.spawn()?;
            child.wait_with_output().await
        };

        let output = timeout(Duration::from_secs(policy.timeout_secs), exec_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ToolError::ExecutionFailed(
                        "ctags not found. Install with: apt install universal-ctags".to_string()
                    )
                } else {
                    ToolError::ExecutionFailed(e.to_string())
                }
            })?;

        // Parse ctags JSON output and filter by name
        let stdout = String::from_utf8_lossy(&output.stdout);
        let name_lower = name.to_lowercase();
        let mut symbols: Vec<serde_json::Value> = Vec::new();

        for line in stdout.lines() {
            if symbols.len() >= max_results {
                break;
            }

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                // Skip non-tag entries (ctags outputs metadata lines too)
                if json.get("_type").and_then(|t| t.as_str()) != Some("tag") {
                    continue;
                }

                let tag_name = json.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let tag_kind = json.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                
                // Name matching
                let name_matches = if exact_match {
                    tag_name == name
                } else {
                    tag_name.to_lowercase().contains(&name_lower)
                };

                if !name_matches {
                    continue;
                }

                // Kind filtering
                if let Some(ref filter) = kind_filter {
                    let kind_lower = tag_kind.to_lowercase();
                    if !kind_lower.contains(filter) {
                        continue;
                    }
                }

                // Extract relevant fields
                let path = json.get("path").and_then(|p| p.as_str()).unwrap_or("");
                let line_num = json.get("line").and_then(|l| l.as_i64()).unwrap_or(0);
                let scope = json.get("scope").and_then(|s| s.as_str());
                let signature = json.get("signature").and_then(|s| s.as_str());

                let mut symbol = serde_json::json!({
                    "name": tag_name,
                    "kind": tag_kind,
                    "path": path,
                    "line": line_num
                });

                if let Some(s) = scope {
                    symbol["scope"] = serde_json::Value::String(s.to_string());
                }
                if let Some(sig) = signature {
                    symbol["signature"] = serde_json::Value::String(sig.to_string());
                }

                symbols.push(symbol);
            }
        }

        let result = serde_json::json!({
            "symbols": symbols,
            "total_found": symbols.len(),
            "truncated": symbols.len() >= max_results
        });

        let result_str = serde_json::to_string_pretty(&result)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if result_str.len() > policy.max_output_bytes {
            Ok(format!(
                "{}...\n\n[TRUNCATED: Output exceeds {} bytes]",
                &result_str[..policy.max_output_bytes],
                policy.max_output_bytes
            ))
        } else {
            Ok(result_str)
        }
    }

    async fn execute_ast_search(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let pattern = tool.input.get("pattern")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'pattern' parameter".to_string()))?;

        let search_path = tool.input.get("path")
            .and_then(|p| p.as_str())
            .unwrap_or(".");
        let resolved = self.resolve_path(search_path);

        if !self.is_path_allowed("ast_search", &resolved) {
            return Err(ToolError::PathNotAllowed(search_path.to_string()));
        }

        let max_results = tool.input.get("max_results")
            .and_then(|v| v.as_i64())
            .unwrap_or(30) as usize;

        // Build ast-grep command
        // Use 'sg' binary (ast-grep CLI) with --json=stream for newline-delimited JSON
        let mut cmd = Command::new("sg");
        cmd.arg("--pattern").arg(pattern)
            .arg("--json=stream")
            .arg(&resolved)
            .current_dir(&self.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add language filter if specified
        if let Some(lang) = tool.input.get("lang").and_then(|l| l.as_str()) {
            cmd.arg("--lang").arg(lang);
        }

        let exec_future = async {
            let child = cmd.spawn()?;
            child.wait_with_output().await
        };

        let output = timeout(Duration::from_secs(policy.timeout_secs), exec_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ToolError::ExecutionFailed(
                        "ast-grep (sg) not found. Install with: cargo install ast-grep --locked".to_string()
                    )
                } else {
                    ToolError::ExecutionFailed(e.to_string())
                }
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Handle non-zero exit (may just mean no matches)
        if !output.status.success() && stdout.is_empty() {
            // Check stderr for actual errors vs just "no matches"
            if !stderr.is_empty() && !stderr.contains("No files") {
                return Err(ToolError::ExecutionFailed(format!(
                    "ast-grep error: {}", stderr.trim()
                )));
            }
            // No matches found - return empty result
            let result = serde_json::json!({
                "matches": [],
                "total_matches": 0,
                "truncated": false
            });
            return serde_json::to_string_pretty(&result)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()));
        }

        // Parse ast-grep JSON output
        // ast-grep outputs newline-delimited JSON objects
        let mut matches: Vec<serde_json::Value> = Vec::new();

        for line in stdout.lines() {
            if matches.len() >= max_results {
                break;
            }

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                // ast-grep JSON format includes: file, range, text, etc.
                let file = json.get("file").and_then(|f| f.as_str()).unwrap_or("");
                let text = json.get("text").and_then(|t| t.as_str()).unwrap_or("");

                // Extract range info
                let range = json.get("range");
                let start_line = range
                    .and_then(|r| r.get("start"))
                    .and_then(|s| s.get("line"))
                    .and_then(|l| l.as_i64())
                    .map(|l| l + 1) // ast-grep uses 0-based lines
                    .unwrap_or(0);
                let end_line = range
                    .and_then(|r| r.get("end"))
                    .and_then(|s| s.get("line"))
                    .and_then(|l| l.as_i64())
                    .map(|l| l + 1)
                    .unwrap_or(0);

                matches.push(serde_json::json!({
                    "file": file,
                    "line": start_line,
                    "end_line": end_line,
                    "text": text.trim()
                }));
            }
        }

        let total_matches = matches.len();
        let truncated = total_matches >= max_results;

        let result = serde_json::json!({
            "matches": matches,
            "total_matches": total_matches,
            "truncated": truncated
        });

        let result_str = serde_json::to_string_pretty(&result)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if result_str.len() > policy.max_output_bytes {
            Ok(format!(
                "{}...\n\n[TRUNCATED: Output exceeds {} bytes]",
                &result_str[..policy.max_output_bytes],
                policy.max_output_bytes
            ))
        } else {
            Ok(result_str)
        }
    }

    async fn execute_edit(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
        let file_path = tool.input.get("file_path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'file_path' parameter".to_string()))?;

        let old_string = tool.input.get("old_string")
            .and_then(|s| s.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'old_string' parameter".to_string()))?;

        let new_string = tool.input.get("new_string")
            .and_then(|s| s.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'new_string' parameter".to_string()))?;

        let replace_all = tool.input.get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let resolved = self.resolve_path(file_path);

        if !self.is_path_allowed("edit", &resolved) {
            return Err(ToolError::PathNotAllowed(file_path.to_string()));
        }

        // Read the file
        let read_future = tokio::fs::read_to_string(&resolved);
        let content = timeout(Duration::from_secs(policy.timeout_secs), read_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::IoError(format!("Failed to read file: {}", e)))?;

        // Count occurrences
        let count = content.matches(old_string).count();

        if count == 0 {
            return Err(ToolError::ExecutionFailed(format!(
                "String not found in file. The old_string must match exactly (including whitespace):\n{:?}",
                if old_string.len() > 100 {
                    format!("{}...", &old_string[..100])
                } else {
                    old_string.to_string()
                }
            )));
        }

        if count > 1 && !replace_all {
            return Err(ToolError::ExecutionFailed(format!(
                "String occurs {} times in file. Either:\n1. Use replace_all=true to replace all occurrences, or\n2. Provide more context in old_string to make it unique",
                count
            )));
        }

        // Perform replacement
        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // Write the file
        let write_future = tokio::fs::write(&resolved, &new_content);
        timeout(Duration::from_secs(policy.timeout_secs), write_future)
            .await
            .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
            .map_err(|e| ToolError::IoError(format!("Failed to write file: {}", e)))?;

        let replacements = if replace_all { count } else { 1 };
        Ok(format!(
            "Replaced {} occurrence{} in {}\n\nOld ({} chars):\n{}\n\nNew ({} chars):\n{}",
            replacements,
            if replacements == 1 { "" } else { "s" },
            file_path,
            old_string.len(),
            if old_string.len() > 200 {
                format!("{}...", &old_string[..200])
            } else {
                old_string.to_string()
            },
            new_string.len(),
            if new_string.len() > 200 {
                format!("{}...", &new_string[..200])
            } else {
                new_string.to_string()
            }
        ))
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

    // ─────────────────────────────────────────────────────────────────────────
    // Mandrel Cross-Session Memory Tool Execution
    // ─────────────────────────────────────────────────────────────────────────

    /// Get the Mandrel client or return error
    fn get_mandrel_client(&self) -> Result<&Arc<RwLock<MandrelClient>>, ToolError> {
        self.mandrel_client
            .as_ref()
            .ok_or(ToolError::MandrelNotConfigured)
    }

    async fn execute_mandrel_project_switch(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;
        let project = tool.input.get("project")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'project' parameter".to_string()))?;

        let mut client_guard = client.write().await;
        client_guard.project_switch(project).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_project_current(&self, _tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;
        let client_guard = client.read().await;
        client_guard.project_current().await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_context_store(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let content = tool.input.get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'content' parameter".to_string()))?;

        let context_type = tool.input.get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'type' parameter".to_string()))?;

        let tags: Vec<String> = tool.input.get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let client_guard = client.read().await;
        client_guard.context_store(content, context_type, &tags).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_context_get_recent(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let limit = tool.input.get("limit")
            .and_then(|l| l.as_u64())
            .map(|l| l as u32);

        let client_guard = client.read().await;
        client_guard.context_get_recent(limit).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_context_search(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let query = tool.input.get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'query' parameter".to_string()))?;

        let client_guard = client.read().await;
        client_guard.context_search(query).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_task_create(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let title = tool.input.get("title")
            .and_then(|t| t.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'title' parameter".to_string()))?;

        let description = tool.input.get("description").and_then(|d| d.as_str());
        let priority = tool.input.get("priority").and_then(|p| p.as_str());

        let client_guard = client.read().await;
        client_guard.task_create(title, description, priority).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_task_update(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let task_id = tool.input.get("task_id")
            .and_then(|t| t.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'task_id' parameter".to_string()))?;

        let status = tool.input.get("status")
            .and_then(|s| s.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'status' parameter".to_string()))?;

        let client_guard = client.read().await;
        client_guard.task_update(task_id, status).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_task_list(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let status = tool.input.get("status").and_then(|s| s.as_str());

        let client_guard = client.read().await;
        client_guard.task_list(status).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_task_details(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let task_id = tool.input.get("task_id")
            .and_then(|t| t.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'task_id' parameter".to_string()))?;

        let client_guard = client.read().await;
        client_guard.task_details(task_id).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_task_progress_summary(&self, _tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;
        let client_guard = client.read().await;
        client_guard.task_progress_summary().await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    async fn execute_mandrel_smart_search(&self, tool: &ToolUse) -> Result<String, ToolError> {
        let client = self.get_mandrel_client()?;

        let query = tool.input.get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| ToolError::ParseError("Missing 'query' parameter".to_string()))?;

        let client_guard = client.read().await;
        client_guard.smart_search(query).await
            .map_err(|e| ToolError::MandrelError(e.to_string()))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // User Interaction Tools
    // ─────────────────────────────────────────────────────────────────────────

    async fn execute_ask_user(&self, tool: &ToolUse) -> Result<String, ToolError> {
        // Parse the questions from the input
        let questions_value = tool.input.get("questions")
            .ok_or_else(|| ToolError::ParseError("Missing 'questions' parameter".to_string()))?;

        let questions_array = questions_value.as_array()
            .ok_or_else(|| ToolError::ParseError("'questions' must be an array".to_string()))?;

        let mut parsed_questions = Vec::new();

        for q in questions_array {
            let header = q.get("header")
                .and_then(|h| h.as_str())
                .ok_or_else(|| ToolError::ParseError("Question missing 'header'".to_string()))?
                .to_string();

            let question = q.get("question")
                .and_then(|q| q.as_str())
                .ok_or_else(|| ToolError::ParseError("Question missing 'question'".to_string()))?
                .to_string();

            let options_value = q.get("options")
                .ok_or_else(|| ToolError::ParseError("Question missing 'options'".to_string()))?;

            let options_array = options_value.as_array()
                .ok_or_else(|| ToolError::ParseError("'options' must be an array".to_string()))?;

            let mut options = Vec::new();
            for opt in options_array {
                let label = opt.get("label")
                    .and_then(|l| l.as_str())
                    .ok_or_else(|| ToolError::ParseError("Option missing 'label'".to_string()))?
                    .to_string();

                let description = opt.get("description")
                    .and_then(|d| d.as_str())
                    .ok_or_else(|| ToolError::ParseError("Option missing 'description'".to_string()))?
                    .to_string();

                options.push(ParsedOption { label, description });
            }

            let multi_select = q.get("multiSelect")
                .and_then(|m| m.as_bool())
                .unwrap_or(false);

            parsed_questions.push(ParsedQuestion {
                header,
                question,
                options,
                multi_select,
            });
        }

        // Return a special error that carries the parsed questions
        // The App will catch this and show the dialog
        Err(ToolError::WaitingForUserInput {
            tool_use_id: tool.id.clone(),
            questions: parsed_questions,
        })
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
            "file_read" | "file_write" | "list_directory" | "file_delete" | "tree" => {
                self.tool.input.get("path")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| ".".to_string())
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
            "grep" => {
                let pattern = self.tool.input.get("pattern")
                    .and_then(|p| p.as_str())
                    .unwrap_or("<pattern>");
                let path = self.tool.input.get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or(".");
                format!("'{}' in {}", pattern, path)
            }
            "glob" => {
                self.tool.input.get("pattern")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "<pattern>".to_string())
            }
            "find_symbol" => {
                let name = self.tool.input.get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("<symbol>");
                let path = self.tool.input.get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or(".");
                format!("'{}' in {}", name, path)
            }
            "ast_search" => {
                let pattern = self.tool.input.get("pattern")
                    .and_then(|p| p.as_str())
                    .unwrap_or("<pattern>");
                let path = self.tool.input.get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or(".");
                format!("'{}' in {}", pattern, path)
            }
            "edit" => {
                let file_path = self.tool.input.get("file_path")
                    .and_then(|p| p.as_str())
                    .unwrap_or("<file>");
                let old_str = self.tool.input.get("old_string")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                let preview = if old_str.len() > 40 {
                    format!("{}...", &old_str[..40])
                } else {
                    old_str.to_string()
                };
                format!("{}: {:?}", file_path, preview)
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

    #[test]
    fn test_search_tools_registered() {
        let registry = ToolRegistry::new();
        
        // Verify new search tools are registered
        assert!(registry.get_policy("grep").is_some());
        assert!(registry.get_policy("glob").is_some());
        assert!(registry.get_policy("tree").is_some());
        
        // Verify they don't require confirmation (read-only tools)
        assert_eq!(registry.can_execute("grep", false), ToolExecutionCheck::Allowed);
        assert_eq!(registry.can_execute("glob", false), ToolExecutionCheck::Allowed);
        assert_eq!(registry.can_execute("tree", false), ToolExecutionCheck::Allowed);
    }

    #[test]
    fn test_tool_definitions_include_search_tools() {
        let registry = ToolRegistry::new();
        let definitions = registry.get_tool_definitions();
        
        let tool_names: Vec<&str> = definitions.iter().map(|d| d.name.as_str()).collect();
        
        assert!(tool_names.contains(&"grep"));
        assert!(tool_names.contains(&"glob"));
        assert!(tool_names.contains(&"tree"));
        assert!(tool_names.contains(&"file_read"));
    }

    #[test]
    fn test_pending_tool_use_summary() {
        // Test grep summary
        let grep_tool = ToolUse {
            id: "test-1".to_string(),
            name: "grep".to_string(),
            input: serde_json::json!({"pattern": "fn main", "path": "src/"}),
        };
        let pending = PendingToolUse::new(grep_tool, ToolExecutionCheck::Allowed);
        assert_eq!(pending.input_summary(), "'fn main' in src/");
        
        // Test glob summary
        let glob_tool = ToolUse {
            id: "test-2".to_string(),
            name: "glob".to_string(),
            input: serde_json::json!({"pattern": "**/*.rs"}),
        };
        let pending = PendingToolUse::new(glob_tool, ToolExecutionCheck::Allowed);
        assert_eq!(pending.input_summary(), "**/*.rs");
        
        // Test tree summary (uses path, default to ".")
        let tree_tool = ToolUse {
            id: "test-3".to_string(),
            name: "tree".to_string(),
            input: serde_json::json!({}),
        };
        let pending = PendingToolUse::new(tree_tool, ToolExecutionCheck::Allowed);
        assert_eq!(pending.input_summary(), ".");

        // Test find_symbol summary
        let symbol_tool = ToolUse {
            id: "test-4".to_string(),
            name: "find_symbol".to_string(),
            input: serde_json::json!({"name": "ToolExecutor", "path": "src/llm/"}),
        };
        let pending = PendingToolUse::new(symbol_tool, ToolExecutionCheck::Allowed);
        assert_eq!(pending.input_summary(), "'ToolExecutor' in src/llm/");
    }

    #[test]
    fn test_find_symbol_registered() {
        let registry = ToolRegistry::new();
        
        // Verify find_symbol is registered
        assert!(registry.get_policy("find_symbol").is_some());
        
        // Verify it doesn't require confirmation (read-only tool)
        assert_eq!(registry.can_execute("find_symbol", false), ToolExecutionCheck::Allowed);
    }

    #[test]
    fn test_tool_definitions_include_find_symbol() {
        let registry = ToolRegistry::new();
        let definitions = registry.get_tool_definitions();

        let tool_names: Vec<&str> = definitions.iter().map(|d| d.name.as_str()).collect();

        assert!(tool_names.contains(&"find_symbol"));

        // Verify find_symbol definition has required properties
        let find_symbol_def = definitions.iter().find(|d| d.name == "find_symbol").unwrap();
        let schema = &find_symbol_def.input_schema;
        assert!(schema.get("properties").unwrap().get("name").is_some());
        assert!(schema.get("properties").unwrap().get("path").is_some());
        assert!(schema.get("properties").unwrap().get("kind").is_some());
    }

    #[test]
    fn test_ast_search_registered() {
        let registry = ToolRegistry::new();

        // Verify ast_search is registered
        assert!(registry.get_policy("ast_search").is_some());

        // Verify it doesn't require confirmation (read-only tool)
        assert_eq!(registry.can_execute("ast_search", false), ToolExecutionCheck::Allowed);
    }

    #[test]
    fn test_tool_definitions_include_ast_search() {
        let registry = ToolRegistry::new();
        let definitions = registry.get_tool_definitions();

        let tool_names: Vec<&str> = definitions.iter().map(|d| d.name.as_str()).collect();

        assert!(tool_names.contains(&"ast_search"));

        // Verify ast_search definition has required properties
        let ast_search_def = definitions.iter().find(|d| d.name == "ast_search").unwrap();
        let schema = &ast_search_def.input_schema;
        assert!(schema.get("properties").unwrap().get("pattern").is_some());
        assert!(schema.get("properties").unwrap().get("path").is_some());
        assert!(schema.get("properties").unwrap().get("lang").is_some());
        assert!(schema.get("properties").unwrap().get("max_results").is_some());

        // Verify pattern is required
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r.as_str() == Some("pattern")));
    }

    #[test]
    fn test_ast_search_pending_tool_use_summary() {
        // Test ast_search summary
        let ast_tool = ToolUse {
            id: "test-5".to_string(),
            name: "ast_search".to_string(),
            input: serde_json::json!({"pattern": "fn $NAME($$$)", "path": "src/"}),
        };
        let pending = PendingToolUse::new(ast_tool, ToolExecutionCheck::Allowed);
        assert_eq!(pending.input_summary(), "'fn $NAME($$$)' in src/");

        // Test with default path
        let ast_tool_default = ToolUse {
            id: "test-6".to_string(),
            name: "ast_search".to_string(),
            input: serde_json::json!({"pattern": "$EXPR.unwrap()"}),
        };
        let pending_default = PendingToolUse::new(ast_tool_default, ToolExecutionCheck::Allowed);
        assert_eq!(pending_default.input_summary(), "'$EXPR.unwrap()' in .");
    }

    #[test]
    fn test_edit_registered() {
        let registry = ToolRegistry::new();

        // Verify edit is registered
        assert!(registry.get_policy("edit").is_some());

        // Verify it requires confirmation (file modification)
        assert_eq!(registry.can_execute("edit", false), ToolExecutionCheck::RequiresConfirmation);
        assert_eq!(registry.can_execute("edit", true), ToolExecutionCheck::Allowed);
    }

    #[test]
    fn test_tool_definitions_include_edit() {
        let registry = ToolRegistry::new();
        let definitions = registry.get_tool_definitions();

        let tool_names: Vec<&str> = definitions.iter().map(|d| d.name.as_str()).collect();

        assert!(tool_names.contains(&"edit"));

        // Verify edit definition has required properties
        let edit_def = definitions.iter().find(|d| d.name == "edit").unwrap();
        let schema = &edit_def.input_schema;
        assert!(schema.get("properties").unwrap().get("file_path").is_some());
        assert!(schema.get("properties").unwrap().get("old_string").is_some());
        assert!(schema.get("properties").unwrap().get("new_string").is_some());
        assert!(schema.get("properties").unwrap().get("replace_all").is_some());

        // Verify required fields
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r.as_str() == Some("file_path")));
        assert!(required.iter().any(|r| r.as_str() == Some("old_string")));
        assert!(required.iter().any(|r| r.as_str() == Some("new_string")));
    }

    #[test]
    fn test_edit_pending_tool_use_summary() {
        // Test edit summary
        let edit_tool = ToolUse {
            id: "test-7".to_string(),
            name: "edit".to_string(),
            input: serde_json::json!({
                "file_path": "src/main.rs",
                "old_string": "fn main() {",
                "new_string": "fn main() -> Result<()> {"
            }),
        };
        let pending = PendingToolUse::new(edit_tool, ToolExecutionCheck::RequiresConfirmation);
        assert_eq!(pending.input_summary(), "src/main.rs: \"fn main() {\"");

        // Test with long old_string (should truncate)
        let edit_tool_long = ToolUse {
            id: "test-8".to_string(),
            name: "edit".to_string(),
            input: serde_json::json!({
                "file_path": "src/lib.rs",
                "old_string": "This is a very long string that should be truncated in the summary because it exceeds forty characters",
                "new_string": "shorter"
            }),
        };
        let pending_long = PendingToolUse::new(edit_tool_long, ToolExecutionCheck::RequiresConfirmation);
        let summary = pending_long.input_summary();
        assert!(summary.starts_with("src/lib.rs:"));
        assert!(summary.contains("..."));
    }
}
