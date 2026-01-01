# Agent Harness Upgrade Plan

**Project**: ridge-control
**Status**: Active Development
**Created**: 2026-01-01
**Last Updated**: 2026-01-01
**Mandrel Project**: ridge-control

---

## Overview

This plan upgrades the ridge-control agent harness from ~70% to ~95% feature parity with Claude Code's agent model. Focus areas:

1. **Search Excellence** - Fast, accurate code discovery
2. **Agent Intelligence** - Sub-agent delegation, cross-session memory
3. **Code Intelligence** - LSP integration for semantic navigation

### Success Metrics

- [ ] Agent can find code 3x faster (fewer tool calls per search)
- [ ] Token usage reduced 40%+ via output_mode and Edit tool
- [ ] Sub-agents handle exploration on cheaper models
- [ ] Cross-session context via Mandrel integration

---

## Current State

### What We Have (Solid)
| Component | File | Status |
|-----------|------|--------|
| Agent state machine | `src/agent/engine.rs` | ✅ Complete |
| Context management | `src/agent/context.rs` | ✅ Complete |
| Token counting | `src/agent/tokens.rs` | ✅ Complete |
| Tool registry | `src/llm/tools.rs` | ✅ Complete |
| Thread persistence | `src/agent/thread.rs` | ✅ Complete |
| 9 tools | `src/llm/tools.rs` | ✅ Complete |

### Current Tools
```
file_read, file_write, file_delete, bash_execute,
list_directory, grep, glob, tree, find_symbol
```

---

## Phase 1: Search Excellence

**Goal**: Make agents find code faster with fewer tokens
**Estimated Effort**: 2-3 days
**Priority**: CRITICAL

### Task 1.1: Grep Output Modes

**File**: `src/llm/tools.rs`

Add `output_mode` parameter to grep tool:

```rust
pub enum GrepOutputMode {
    /// Return full matching lines with context (current behavior)
    Content,
    /// Return only file paths that contain matches
    FilesWithMatches,
    /// Return match count per file
    Count,
}
```

**Implementation Details**:

1. Update `ToolDefinition` for grep:
```rust
"output_mode": {
    "type": "string",
    "enum": ["content", "files_with_matches", "count"],
    "default": "files_with_matches",
    "description": "Output format: 'content' for full lines, 'files_with_matches' for paths only, 'count' for match counts"
}
```

2. Update `execute_grep()` to handle modes:
```rust
// For files_with_matches mode:
cmd.arg("--files-with-matches");

// For count mode:
cmd.arg("--count");
```

3. Update JSON output format per mode

**Test Cases**:
- [ ] `output_mode: files_with_matches` returns only paths
- [ ] `output_mode: count` returns `{"file": "path", "count": N}`
- [ ] `output_mode: content` preserves current behavior
- [ ] Default behavior is `files_with_matches` (most efficient)

**Success Criteria**: Agent asking "what files contain X" uses ~50 tokens instead of ~500

---

### Task 1.2: Grep Pagination

**File**: `src/llm/tools.rs`

Add `head_limit` and `offset` parameters:

```rust
"head_limit": {
    "type": "integer",
    "description": "Limit results to first N entries (default: 50)"
},
"offset": {
    "type": "integer",
    "description": "Skip first N entries for pagination (default: 0)"
}
```

**Implementation**: Apply after ripgrep returns, slice the results vector.

**Test Cases**:
- [ ] `head_limit: 10` returns only 10 results
- [ ] `offset: 10, head_limit: 10` returns results 11-20
- [ ] Works with all output modes

---

### Task 1.3: Add ast-grep Tool

**File**: `src/llm/tools.rs`

New tool for structural code search via ast-grep:

```rust
ToolDefinition {
    name: "ast_search".to_string(),
    description: "Search code using structural patterns (AST-aware). \
        More accurate than text search for finding function definitions, \
        method calls, and code patterns. Uses tree-sitter for parsing.".to_string(),
    input_schema: serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "AST pattern to search for. Examples: \
                    '$FUNC($ARGS)' matches function calls, \
                    'fn $NAME($PARAMS) { $BODY }' matches function definitions, \
                    '$EXPR.unwrap()' matches unwrap calls"
            },
            "path": {
                "type": "string",
                "description": "Directory to search in (default: current directory)"
            },
            "lang": {
                "type": "string",
                "enum": ["rust", "typescript", "javascript", "python", "go", "c", "cpp"],
                "description": "Language to parse as (auto-detected if not specified)"
            },
            "max_results": {
                "type": "integer",
                "description": "Maximum matches to return (default: 30)"
            }
        },
        "required": ["pattern"]
    }),
}
```

**Implementation**:

```rust
async fn execute_ast_search(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
    let pattern = tool.input.get("pattern")
        .and_then(|p| p.as_str())
        .ok_or_else(|| ToolError::ParseError("Missing 'pattern'".to_string()))?;

    let search_path = tool.input.get("path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");

    let mut cmd = Command::new("ast-grep");
    cmd.arg("--pattern").arg(pattern)
       .arg("--json")
       .arg(&self.resolve_path(search_path));

    if let Some(lang) = tool.input.get("lang").and_then(|l| l.as_str()) {
        cmd.arg("--lang").arg(lang);
    }

    // Execute and parse JSON output...
}
```

**Dependencies**:
- Requires `ast-grep` binary (install: `cargo install ast-grep` or `npm i -g @ast-grep/cli`)
- Document in README

**Test Cases**:
- [ ] Find all function definitions: `fn $NAME($$$)`
- [ ] Find all .unwrap() calls: `$EXPR.unwrap()`
- [ ] Find struct definitions: `struct $NAME { $$$ }`
- [ ] Language auto-detection works
- [ ] Graceful error if ast-grep not installed

---

### Task 1.4: Add Edit Tool

**File**: `src/llm/tools.rs`

Surgical string replacement instead of full file rewrites:

```rust
ToolDefinition {
    name: "edit".to_string(),
    description: "Replace exact string in a file. More efficient than \
        rewriting entire file. The old_string must match exactly \
        (including whitespace). Use replace_all to change all occurrences.".to_string(),
    input_schema: serde_json::json!({
        "type": "object",
        "properties": {
            "file_path": {
                "type": "string",
                "description": "Path to the file to edit"
            },
            "old_string": {
                "type": "string",
                "description": "Exact string to find and replace (must be unique unless replace_all)"
            },
            "new_string": {
                "type": "string",
                "description": "Replacement string"
            },
            "replace_all": {
                "type": "boolean",
                "default": false,
                "description": "Replace all occurrences (default: false, requires unique match)"
            }
        },
        "required": ["file_path", "old_string", "new_string"]
    }),
}
```

**Implementation**:

```rust
async fn execute_edit(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
    let file_path = extract_string(&tool.input, "file_path")?;
    let old_string = extract_string(&tool.input, "old_string")?;
    let new_string = extract_string(&tool.input, "new_string")?;
    let replace_all = tool.input.get("replace_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let resolved = self.resolve_path(&file_path);
    let content = tokio::fs::read_to_string(&resolved).await
        .map_err(|e| ToolError::IoError(e.to_string()))?;

    // Count occurrences
    let count = content.matches(&old_string).count();

    if count == 0 {
        return Err(ToolError::ExecutionFailed(
            format!("String not found in file: {:?}", old_string)
        ));
    }

    if count > 1 && !replace_all {
        return Err(ToolError::ExecutionFailed(
            format!("String occurs {} times. Use replace_all=true or provide more context.", count)
        ));
    }

    let new_content = if replace_all {
        content.replace(&old_string, &new_string)
    } else {
        content.replacen(&old_string, &new_string, 1)
    };

    tokio::fs::write(&resolved, &new_content).await
        .map_err(|e| ToolError::IoError(e.to_string()))?;

    Ok(format!("Replaced {} occurrence(s) in {}",
        if replace_all { count } else { 1 },
        file_path))
}
```

**Policy**: Requires confirmation (same as file_write)

**Test Cases**:
- [ ] Single replacement works
- [ ] replace_all replaces all occurrences
- [ ] Error if old_string not found
- [ ] Error if multiple matches without replace_all
- [ ] Preserves file permissions

---

## Phase 2: Agent Intelligence

**Goal**: Enable delegation and cross-session memory
**Estimated Effort**: 1 week
**Priority**: HIGH

### Task 2.1: Sub-agent Configuration

**File**: `src/config/mod.rs` (new struct)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentConfig {
    /// Model to use for this agent type
    pub model: String,
    /// Provider (anthropic, openai, gemini, grok, groq)
    pub provider: String,
    /// Maximum tokens for sub-agent context
    pub max_context_tokens: Option<u32>,
    /// Tools available to this sub-agent (empty = all read-only tools)
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentsConfig {
    /// Agent for exploration/research tasks
    pub explore: SubagentConfig,
    /// Agent for planning/architecture
    pub plan: SubagentConfig,
    /// Agent for code review
    pub review: SubagentConfig,
    /// Default for unspecified types
    pub default: SubagentConfig,
}

impl Default for SubagentsConfig {
    fn default() -> Self {
        Self {
            explore: SubagentConfig {
                model: "claude-3-5-haiku-20241022".to_string(),
                provider: "anthropic".to_string(),
                max_context_tokens: Some(50_000),
                allowed_tools: vec![
                    "file_read", "grep", "glob", "tree",
                    "find_symbol", "ast_search"
                ].into_iter().map(String::from).collect(),
            },
            plan: SubagentConfig {
                model: "claude-sonnet-4-20250514".to_string(),
                provider: "anthropic".to_string(),
                max_context_tokens: Some(100_000),
                allowed_tools: vec![], // All tools
            },
            review: SubagentConfig {
                model: "gpt-4o-mini".to_string(),
                provider: "openai".to_string(),
                max_context_tokens: Some(30_000),
                allowed_tools: vec![
                    "file_read", "grep", "ast_search"
                ].into_iter().map(String::from).collect(),
            },
            default: SubagentConfig {
                model: "claude-3-5-haiku-20241022".to_string(),
                provider: "anthropic".to_string(),
                max_context_tokens: Some(50_000),
                allowed_tools: vec![],
            },
        }
    }
}
```

**Config file** (`~/.config/ridge-control/config.toml`):
```toml
[subagents.explore]
model = "claude-3-5-haiku-20241022"
provider = "anthropic"
max_context_tokens = 50000
allowed_tools = ["file_read", "grep", "glob", "tree", "find_symbol", "ast_search"]

[subagents.plan]
model = "claude-sonnet-4-20250514"
provider = "anthropic"

[subagents.review]
model = "gpt-4o-mini"
provider = "openai"
```

---

### Task 2.1b: Command Palette Subagent Model Selection

**Goal**: Allow runtime configuration of subagent models via command palette (matching existing provider/model selection UX)

**Files**:
- `src/action.rs` - New action variants
- `src/components/command_palette.rs` - New registry methods
- `src/app.rs` - Handle actions, update command palette

**New Action Variants** (`src/action.rs`):
```rust
/// Select model for a specific subagent type
SubagentSelectModel { agent_type: String, model: String },
/// Select provider for a specific subagent type
SubagentSelectProvider { agent_type: String, provider: String },
```

**CommandRegistry Methods** (`src/components/command_palette.rs`):
```rust
/// Set available models for each subagent type
pub fn set_subagent_models(&mut self, subagent_configs: &SubagentsConfig, available_models: &HashMap<String, Vec<String>>) {
    self.remove_commands_with_prefix("subagent:");

    for (agent_type, config) in [
        ("explore", &subagent_configs.explore),
        ("plan", &subagent_configs.plan),
        ("review", &subagent_configs.review),
    ] {
        // Get models for this agent's provider
        if let Some(models) = available_models.get(&config.provider) {
            for model in models {
                let is_current = *model == config.model;
                let name = if is_current {
                    format!("Subagent {}: {} ✓", agent_type, model)
                } else {
                    format!("Subagent {}: {}", agent_type, model)
                };

                self.commands.push(Command::new(
                    format!("subagent:{}:{}", agent_type, model),
                    name,
                    format!("Use {} for {} subagent", model, agent_type),
                    Action::SubagentSelectModel {
                        agent_type: agent_type.to_string(),
                        model: model.to_string(),
                    },
                ));
            }
        }
    }
}
```

**App Integration** (`src/app.rs`):
```rust
// In handle_action match:
Action::SubagentSelectModel { agent_type, model } => {
    match agent_type.as_str() {
        "explore" => self.subagent_config.explore.model = model,
        "plan" => self.subagent_config.plan.model = model,
        "review" => self.subagent_config.review.model = model,
        _ => {}
    }
    // Refresh command palette to show updated checkmarks
    self.refresh_subagent_commands();
}
```

**UX**:
- Commands appear as: `Subagent explore: claude-haiku ✓`
- Typing "subagent" or "explore" filters to relevant options
- Current model shows checkmark (✓)
- Changes take effect immediately for next subagent spawn

**Test Cases**:
- [ ] Subagent commands appear in palette
- [ ] Checkmark shows current model per agent type
- [ ] Selection updates config and refreshes palette
- [ ] Works with all configured providers

---

### Task 2.2: Task Tool (Sub-agent Spawning)

**File**: `src/llm/tools.rs`

```rust
ToolDefinition {
    name: "task".to_string(),
    description: "Spawn a sub-agent to handle complex tasks autonomously. \
        Use 'explore' for codebase research, 'plan' for architecture decisions, \
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
}
```

**New module**: `src/agent/subagent.rs`

```rust
pub struct SubagentManager {
    config: SubagentsConfig,
    provider_registry: Arc<ProviderRegistry>,
    running_tasks: HashMap<String, JoinHandle<SubagentResult>>,
}

pub struct SubagentResult {
    pub task_id: String,
    pub agent_type: String,
    pub result: String,
    pub tokens_used: u32,
    pub duration_ms: u64,
}

impl SubagentManager {
    pub async fn spawn(
        &mut self,
        agent_type: &str,
        prompt: &str,
        background: bool,
    ) -> Result<SubagentResult, SubagentError> {
        let config = self.get_config(agent_type);
        let provider = self.provider_registry.get(&config.provider)?;

        // Build sub-agent with limited tools
        let tools = self.filter_tools(&config.allowed_tools);

        // Create isolated context (no parent conversation)
        let request = self.build_request(prompt, &config, tools);

        if background {
            let task_id = uuid::Uuid::new_v4().to_string();
            let handle = tokio::spawn(async move {
                // Execute and return result
            });
            self.running_tasks.insert(task_id.clone(), handle);
            Ok(SubagentResult { task_id, /* pending */ })
        } else {
            // Execute synchronously
            self.execute_sync(request, provider).await
        }
    }
}
```

**Integration with AgentEngine**:
- Add `SubagentManager` to `AgentEngine` struct
- Handle `task` tool execution in tool executor
- Return sub-agent results as tool output

---

### Task 2.3: Mandrel Integration

**File**: `src/agent/mandrel.rs` (new)

```rust
use reqwest::Client;

pub struct MandrelClient {
    client: Client,
    base_url: String,
    project: String,
}

impl MandrelClient {
    pub fn new(base_url: &str, project: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            project: project.to_string(),
        }
    }

    /// Store context for future retrieval
    pub async fn store_context(
        &self,
        content: &str,
        context_type: &str,
        tags: &[&str],
    ) -> Result<String, MandrelError> {
        let body = serde_json::json!({
            "arguments": {
                "content": content,
                "type": context_type,
                "tags": tags
            }
        });

        let resp = self.client
            .post(&format!("{}/mcp/tools/context_store", self.base_url))
            .json(&body)
            .send()
            .await?;

        // Parse response, return context ID
        Ok(resp.json::<ContextStoreResponse>().await?.id)
    }

    /// Search contexts semantically
    pub async fn search_context(&self, query: &str) -> Result<Vec<Context>, MandrelError> {
        let body = serde_json::json!({
            "arguments": {
                "query": query
            }
        });

        let resp = self.client
            .post(&format!("{}/mcp/tools/context_search", self.base_url))
            .json(&body)
            .send()
            .await?;

        Ok(resp.json::<ContextSearchResponse>().await?.contexts)
    }

    /// Get recent contexts for session continuity
    pub async fn get_recent(&self, limit: usize) -> Result<Vec<Context>, MandrelError> {
        // ...
    }
}
```

**Tools to add**:

```rust
ToolDefinition {
    name: "context_store".to_string(),
    description: "Store context in Mandrel for cross-session memory. \
        Use for important findings, decisions, or handoff notes.".to_string(),
    // ...
},
ToolDefinition {
    name: "context_search".to_string(),
    description: "Search stored contexts semantically. Use before \
        re-investigating something that may have been explored before.".to_string(),
    // ...
}
```

---

### Task 2.4: Ask User Tool

**File**: `src/llm/tools.rs`

```rust
ToolDefinition {
    name: "ask_user".to_string(),
    description: "Ask the user a question with structured options. \
        Use when you need clarification or a decision.".to_string(),
    input_schema: serde_json::json!({
        "type": "object",
        "properties": {
            "question": {
                "type": "string",
                "description": "The question to ask"
            },
            "options": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "description": { "type": "string" }
                    }
                },
                "description": "2-4 options for the user to choose from"
            },
            "allow_other": {
                "type": "boolean",
                "default": true,
                "description": "Allow user to provide custom answer"
            }
        },
        "required": ["question", "options"]
    }),
}
```

**UI Integration**:
- New component `src/components/ask_user_dialog.rs`
- Shows question, options as selectable list
- Returns selected option or custom text

---

## Phase 3: Code Intelligence

**Goal**: LSP integration for semantic code navigation
**Estimated Effort**: 1-2 weeks
**Priority**: MEDIUM

### Task 3.1: LSP Client Infrastructure

**File**: `src/lsp/client.rs` (new module)

```rust
use tower_lsp::lsp_types::*;
use tokio::process::{Command, Child};

pub struct LspClient {
    process: Child,
    reader: BufReader<ChildStdout>,
    writer: BufWriter<ChildStdin>,
    request_id: i64,
    capabilities: ServerCapabilities,
}

impl LspClient {
    pub async fn start(command: &str, args: &[&str], root: &Path) -> Result<Self, LspError> {
        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut client = Self {
            process,
            reader: BufReader::new(process.stdout.take().unwrap()),
            writer: BufWriter::new(process.stdin.take().unwrap()),
            request_id: 0,
            capabilities: Default::default(),
        };

        // Initialize LSP
        client.initialize(root).await?;
        Ok(client)
    }

    pub async fn go_to_definition(
        &mut self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, LspError> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path(file).unwrap(),
                },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        self.request("textDocument/definition", params).await
    }

    pub async fn find_references(
        &mut self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, LspError> {
        // Similar to go_to_definition
    }

    pub async fn document_symbols(&mut self, file: &Path) -> Result<Vec<DocumentSymbol>, LspError> {
        // ...
    }
}
```

### Task 3.2: LSP Manager (Multi-language)

**File**: `src/lsp/manager.rs`

```rust
pub struct LspManager {
    clients: HashMap<String, LspClient>,
    config: LspConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    pub rust: Option<LspServerConfig>,
    pub typescript: Option<LspServerConfig>,
    pub python: Option<LspServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            rust: Some(LspServerConfig {
                command: "rust-analyzer".to_string(),
                args: vec![],
            }),
            typescript: Some(LspServerConfig {
                command: "typescript-language-server".to_string(),
                args: vec!["--stdio".to_string()],
            }),
            python: Some(LspServerConfig {
                command: "pylsp".to_string(),
                args: vec![],
            }),
        }
    }
}

impl LspManager {
    pub async fn get_client(&mut self, language: &str, root: &Path) -> Result<&mut LspClient, LspError> {
        if !self.clients.contains_key(language) {
            let config = self.get_server_config(language)?;
            let client = LspClient::start(&config.command, &config.args, root).await?;
            self.clients.insert(language.to_string(), client);
        }
        Ok(self.clients.get_mut(language).unwrap())
    }

    fn detect_language(&self, file: &Path) -> Option<&str> {
        match file.extension()?.to_str()? {
            "rs" => Some("rust"),
            "ts" | "tsx" => Some("typescript"),
            "js" | "jsx" => Some("javascript"),
            "py" => Some("python"),
            "go" => Some("go"),
            _ => None,
        }
    }
}
```

### Task 3.3: LSP Tools

**File**: `src/llm/tools.rs`

```rust
// lsp_definition
ToolDefinition {
    name: "lsp_definition".to_string(),
    description: "Go to the definition of a symbol at the given position. \
        Returns file path and line number of the definition.".to_string(),
    input_schema: serde_json::json!({
        "type": "object",
        "properties": {
            "file_path": { "type": "string" },
            "line": { "type": "integer", "description": "1-based line number" },
            "character": { "type": "integer", "description": "1-based column" }
        },
        "required": ["file_path", "line", "character"]
    }),
}

// lsp_references
ToolDefinition {
    name: "lsp_references".to_string(),
    description: "Find all references to the symbol at the given position.".to_string(),
    // Similar schema
}

// lsp_hover
ToolDefinition {
    name: "lsp_hover".to_string(),
    description: "Get type information and documentation for symbol at position.".to_string(),
    // Similar schema
}

// lsp_symbols
ToolDefinition {
    name: "lsp_symbols".to_string(),
    description: "List all symbols (functions, classes, etc.) in a file or workspace.".to_string(),
    input_schema: serde_json::json!({
        "type": "object",
        "properties": {
            "file_path": {
                "type": "string",
                "description": "File to get symbols from (omit for workspace search)"
            },
            "query": {
                "type": "string",
                "description": "Filter symbols by name (for workspace search)"
            }
        }
    }),
}
```

---

## Implementation Order

### Sprint 1 (Days 1-3): Phase 1 - Search Excellence
1. Task 1.1: grep output_mode
2. Task 1.2: grep pagination
3. Task 1.4: Edit tool
4. Task 1.3: ast_search tool

### Sprint 2 (Days 4-7): Phase 2a - Sub-agents
1. Task 2.1: SubagentConfig
2. Task 2.1b: Command palette subagent model selection
3. Task 2.2: Task tool + SubagentManager
4. Task 2.4: ask_user tool

### Sprint 3 (Days 8-10): Phase 2b - Mandrel
1. Task 2.3: MandrelClient
2. Integration with agent loop
3. Auto-store on handoff

### Sprint 4 (Days 11-17): Phase 3 - LSP
1. Task 3.1: LspClient
2. Task 3.2: LspManager
3. Task 3.3: LSP tools

---

## Testing Strategy

### Unit Tests
- Each tool in isolation
- Mock LLM responses for sub-agents
- Mock Mandrel for context operations

### Integration Tests
- Full agent loop with tool execution
- Sub-agent spawning and result handling
- LSP client with real language servers

### Manual Testing
- Docker environment with all tools
- Multi-turn conversations
- Cross-session continuity

---

## Rollback Plan

Each phase is independently deployable. If issues arise:

1. **Phase 1**: Revert to existing grep/file_write
2. **Phase 2**: Disable sub-agents, keep main agent
3. **Phase 3**: Graceful fallback to grep/ctags when LSP unavailable

---

## Dependencies

### External Binaries (Docker)
```bash
# Phase 1
cargo install ast-grep
# OR
npm install -g @ast-grep/cli

# Phase 3
rustup component add rust-analyzer
npm install -g typescript-language-server
pip install python-lsp-server
```

### Rust Crates
```toml
# Cargo.toml additions
tower-lsp = "0.20"  # LSP client
reqwest = { version = "0.11", features = ["json"] }  # Mandrel HTTP
```

---

## Mandrel Task Tracking

Tasks will be created in Mandrel as work progresses:

```
task_create: "P1-T1.1: Add grep output_mode"
task_create: "P1-T1.2: Add grep pagination"
task_create: "P1-T1.3: Add ast_search tool"
task_create: "P1-T1.4: Add Edit tool"
...
```

Update status as work completes:
```
task_update: taskId, status: "in_progress"
task_update: taskId, status: "completed"
```

---

## Handoff Protocol

When ending a session:

1. **Commit work** with descriptive message
2. **Store context** in Mandrel:
   - What was completed
   - What's in progress
   - Any blockers or decisions needed
3. **Update task status** in Mandrel
4. **Note next steps** clearly

---

## References

- Claude Code tool definitions: System prompt analysis
- ast-grep documentation: https://ast-grep.github.io/
- Tower LSP: https://github.com/ebkalderon/tower-lsp
- Mandrel MCP API: See CLAUDE.md in ~/projects

---

**Document Version**: 1.0
**Author**: Claude + ridgetop
**Review Status**: Initial draft
