# Agent Tools Edge Plan

**Project**: ridge-control
**Status**: Strategic Planning
**Created**: 2026-01-02
**Author**: Claude Opus 4.5 + ridgetop
**Goal**: Production-quality agent tools that exceed Claude Code capabilities

---

## Research Summary

### Sources Analyzed

1. **[Anthropic Engineering: Writing Tools for Agents](https://www.anthropic.com/engineering/writing-tools-for-agents)** - Official best practices
2. **[CodeAnt: Why Coding Agents Should Use ripgrep](https://www.codeant.ai/blogs/why-coding-agents-should-use-ripgrep)** - Performance optimization
3. **[Aider AI](https://github.com/Aider-AI/aider)** - grep-ast, repo mapping patterns
4. **[Cursor 2.0 Architecture](https://www.digitalapplied.com/blog/cursor-2-0-agent-first-architecture-guide)** - Multi-agent patterns
5. **[ast-grep MCP Integration](https://ast-grep.github.io/advanced/prompting.html)** - Structural search
6. **[Firecrawl](https://www.firecrawl.dev/)** - Web data extraction for AI
7. **[pty-process crate](https://docs.rs/pty-process)** - Async PTY in Rust

### Key Insights

| Insight | Source | Application |
|---------|--------|-------------|
| Return "contextual relevance over flexibility" | Anthropic | All tools should minimize noise |
| 25k token response limit | Claude Code | Apply truncation with clear indicators |
| Resolve IDs to semantic names | Anthropic | Use readable field names always |
| ripgrep 10x faster than grep | CodeAnt | ripgrep as default, no fallback |
| Smart-case reduces agent errors | CodeAnt | Enable by default |
| Firecrawl → LLM-ready markdown | Firecrawl | Web fetch should output markdown |
| MCP server for ast-grep | ast-grep | Can add as future enhancement |
| Cursor uses 8 parallel agents | Cursor | Inform sub-agent design |

---

## Phase 1: Search Excellence (Priority: CRITICAL)

### Tool 1.1: Enhanced Grep

**Goal**: Best-in-class code search that saves 5-10x tokens vs naive grep

**Current State**: Basic grep exists, missing efficiency features

**Target State**: Exceeds Claude Code with:
- All Claude Code features
- Smart defaults that reduce agent errors
- Response format optimization
- Caching layer for repeated searches

#### Schema

```rust
ToolDefinition {
    name: "grep",
    description: "Search for text patterns in files using ripgrep. \
        Returns matches with context. Defaults to files_with_matches mode \
        for efficiency - use output_mode='content' for full match text. \
        Respects .gitignore automatically. Case-insensitive by default \
        for lowercase patterns (smart-case).",
    input_schema: json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Text or regex pattern to search for"
            },
            "path": {
                "type": "string",
                "description": "Directory or file to search (default: current directory)"
            },
            "output_mode": {
                "type": "string",
                "enum": ["files_with_matches", "content", "count"],
                "default": "files_with_matches",
                "description": "Output format: 'files_with_matches' returns paths only (most efficient), \
                    'content' returns matching lines with context, 'count' returns match counts per file"
            },
            "glob": {
                "type": "string",
                "description": "Filter files by glob pattern (e.g., '*.rs', '*.{ts,tsx}')"
            },
            "type": {
                "type": "string",
                "description": "File type filter (e.g., 'rust', 'typescript', 'python'). \
                    More efficient than glob for standard types."
            },
            "context_before": {
                "type": "integer",
                "description": "Lines of context before match (only for content mode)"
            },
            "context_after": {
                "type": "integer",
                "description": "Lines of context after match (only for content mode)"
            },
            "context": {
                "type": "integer",
                "description": "Lines of context before AND after (shorthand)"
            },
            "head_limit": {
                "type": "integer",
                "default": 50,
                "description": "Maximum results to return (default: 50)"
            },
            "offset": {
                "type": "integer",
                "default": 0,
                "description": "Skip first N results (for pagination)"
            },
            "case_sensitive": {
                "type": "boolean",
                "description": "Force case-sensitive search (default: smart-case)"
            },
            "literal": {
                "type": "boolean",
                "description": "Treat pattern as literal text, not regex"
            },
            "multiline": {
                "type": "boolean",
                "description": "Enable multiline matching (patterns can span lines)"
            },
            "invert": {
                "type": "boolean",
                "description": "Return lines that do NOT match"
            }
        },
        "required": ["pattern"]
    })
}
```

#### Response Formats

**files_with_matches mode** (default - most token-efficient):
```json
{
    "mode": "files_with_matches",
    "pattern": "AgentEngine",
    "path": "src/",
    "files": [
        "src/agent/engine.rs",
        "src/agent/mod.rs",
        "src/app.rs"
    ],
    "total_matches": 47,
    "files_shown": 3,
    "truncated": false
}
```

**content mode** (full context):
```json
{
    "mode": "content",
    "pattern": "AgentEngine",
    "matches": [
        {
            "file": "src/agent/engine.rs",
            "line": 45,
            "column": 12,
            "text": "pub struct AgentEngine<S: ThreadStore> {",
            "context_before": ["", "/// Main agent execution engine"],
            "context_after": ["    state: AgentState,", "    config: AgentConfig,"]
        }
    ],
    "total_matches": 47,
    "shown": 10,
    "truncated": true,
    "next_offset": 10
}
```

**count mode** (statistics):
```json
{
    "mode": "count",
    "pattern": "unwrap\\(\\)",
    "counts": [
        {"file": "src/app.rs", "count": 23},
        {"file": "src/agent/engine.rs", "count": 8},
        {"file": "src/llm/tools.rs", "count": 15}
    ],
    "total_files": 12,
    "total_matches": 89
}
```

#### Implementation Details

```rust
async fn execute_grep(&self, tool: &ToolUse, policy: &ToolPolicy) -> Result<String, ToolError> {
    let pattern = extract_required_string(&tool.input, "pattern")?;
    let output_mode = extract_string_or(&tool.input, "output_mode", "files_with_matches");
    let head_limit = extract_int_or(&tool.input, "head_limit", 50) as usize;
    let offset = extract_int_or(&tool.input, "offset", 0) as usize;

    let mut cmd = Command::new("rg");
    cmd.arg("--json"); // Always JSON for structured parsing

    // Output mode flags
    match output_mode.as_str() {
        "files_with_matches" => { cmd.arg("-l"); }
        "count" => { cmd.arg("-c"); }
        "content" => {
            // Add context if specified
            if let Some(ctx) = tool.input.get("context").and_then(|v| v.as_i64()) {
                cmd.arg("-C").arg(ctx.to_string());
            } else {
                if let Some(before) = tool.input.get("context_before").and_then(|v| v.as_i64()) {
                    cmd.arg("-B").arg(before.to_string());
                }
                if let Some(after) = tool.input.get("context_after").and_then(|v| v.as_i64()) {
                    cmd.arg("-A").arg(after.to_string());
                }
            }
        }
        _ => return Err(ToolError::ParseError("Invalid output_mode".into()))
    }

    // Smart-case by default (unless case_sensitive specified)
    if !tool.input.get("case_sensitive").and_then(|v| v.as_bool()).unwrap_or(false) {
        cmd.arg("--smart-case");
    }

    // File type filter (more efficient than glob for known types)
    if let Some(file_type) = tool.input.get("type").and_then(|v| v.as_str()) {
        cmd.arg("-t").arg(file_type);
    }

    // Glob filter
    if let Some(glob) = tool.input.get("glob").and_then(|v| v.as_str()) {
        cmd.arg("--glob").arg(glob);
    }

    // Literal mode
    if tool.input.get("literal").and_then(|v| v.as_bool()).unwrap_or(false) {
        cmd.arg("-F");
    }

    // Multiline mode
    if tool.input.get("multiline").and_then(|v| v.as_bool()).unwrap_or(false) {
        cmd.arg("-U").arg("--multiline-dotall");
    }

    // Invert match
    if tool.input.get("invert").and_then(|v| v.as_bool()).unwrap_or(false) {
        cmd.arg("-v");
    }

    cmd.arg(&pattern);

    // Search path
    let search_path = tool.input.get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    cmd.arg(self.resolve_path(search_path));

    // Execute with timeout
    let output = timeout(
        Duration::from_secs(policy.timeout_secs),
        cmd.output()
    ).await
        .map_err(|_| ToolError::Timeout(policy.timeout_secs))?
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

    // Parse JSON output, apply pagination
    let results = self.parse_rg_json(&output.stdout, output_mode, head_limit, offset)?;

    Ok(serde_json::to_string_pretty(&results)?)
}
```

#### Competitive Edges Over Claude Code

1. **Smart-case by default** - Agents don't need to remember `-i` flag
2. **`type` filter** - More efficient than glob for standard types
3. **`invert` mode** - Find lines NOT matching (useful for cleanup)
4. **`next_offset` in response** - Explicit pagination hint for agents
5. **Consistent JSON structure** - Same wrapper for all modes

---

### Tool 1.2: Multi-span File Read

**Goal**: Read multiple non-contiguous regions in one call

#### Schema

```rust
ToolDefinition {
    name: "file_read",
    description: "Read file contents. Supports reading specific line ranges or \
        multiple non-contiguous spans in a single call for efficiency. \
        Automatically detects binary files and returns metadata only.",
    input_schema: json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file to read"
            },
            "start_line": {
                "type": "integer",
                "description": "Start line (1-indexed). Shorthand for single span."
            },
            "end_line": {
                "type": "integer",
                "description": "End line (inclusive). Shorthand for single span."
            },
            "spans": {
                "type": "array",
                "description": "Multiple line ranges to read: [{start: N, end: M}, ...]",
                "items": {
                    "type": "object",
                    "properties": {
                        "start": { "type": "integer" },
                        "end": { "type": "integer" }
                    }
                }
            },
            "max_lines": {
                "type": "integer",
                "default": 500,
                "description": "Maximum total lines across all spans"
            },
            "show_line_numbers": {
                "type": "boolean",
                "default": true,
                "description": "Prefix each line with line number"
            }
        },
        "required": ["path"]
    })
}
```

#### Response Format

```json
{
    "path": "src/agent/engine.rs",
    "total_lines": 480,
    "spans": [
        {
            "start": 1,
            "end": 15,
            "content": "1\t//! Agent execution engine\n2\t\n3\tuse async_trait..."
        },
        {
            "start": 265,
            "end": 280,
            "content": "265\t    LLMEvent::ToolUseDetected(tool_use) => {\n266\t..."
        }
    ],
    "total_lines_read": 31,
    "truncated": false
}
```

#### Competitive Edge

- **Span optimization** - If spans are adjacent or overlapping, merge them automatically
- **Binary detection** - Return `{"binary": true, "size": 15234, "mime_type": "image/png"}` for binary files
- **Smart truncation** - If max_lines exceeded, truncate last span with `"truncated_at": 12`

---

## Phase 2: Web Access (Priority: HIGH)

### Tool 2.1: Web Fetch

**Goal**: Fetch web pages as LLM-ready markdown with intelligent content extraction

#### Design Philosophy

Based on research, the best approach is:
1. Use Mozilla Readability for content extraction (same as Firecrawl)
2. Convert to clean markdown (same format agents are trained on)
3. Cache results (web pages change slowly, save money)
4. Rate limit (prevent abuse, respect robots.txt)

#### Schema

```rust
ToolDefinition {
    name: "web_fetch",
    description: "Fetch web page content and convert to LLM-ready markdown. \
        Extracts main content, strips navigation/ads/scripts. \
        Results cached for 15 minutes. Respects robots.txt.",
    input_schema: json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "format": "uri",
                "description": "URL to fetch (http/https)"
            },
            "selector": {
                "type": "string",
                "description": "Optional CSS selector to extract specific content"
            },
            "include_links": {
                "type": "boolean",
                "default": true,
                "description": "Preserve hyperlinks in markdown output"
            },
            "include_images": {
                "type": "boolean",
                "default": false,
                "description": "Include image references (as markdown links)"
            },
            "max_length": {
                "type": "integer",
                "default": 50000,
                "description": "Maximum characters to return"
            },
            "timeout_secs": {
                "type": "integer",
                "default": 30,
                "description": "Request timeout in seconds"
            }
        },
        "required": ["url"]
    })
}
```

#### Response Format

```json
{
    "url": "https://docs.rs/tokio/latest/tokio/",
    "final_url": "https://docs.rs/tokio/1.43.0/tokio/",
    "title": "tokio - Rust",
    "content": "# tokio\n\nA runtime for writing reliable, asynchronous...",
    "content_length": 15234,
    "truncated": false,
    "cached": true,
    "fetched_at": "2026-01-02T10:30:00Z"
}
```

#### Implementation Strategy

```rust
pub struct WebFetcher {
    client: reqwest::Client,
    cache: Arc<RwLock<LruCache<String, CachedPage>>>,
    rate_limiter: Arc<RateLimiter>,
}

struct CachedPage {
    content: String,
    fetched_at: DateTime<Utc>,
    ttl_secs: u64,
}

impl WebFetcher {
    pub async fn fetch(&self, url: &str, opts: &FetchOptions) -> Result<FetchResult, FetchError> {
        // Check cache first
        if let Some(cached) = self.get_cached(url) {
            return Ok(FetchResult { cached: true, ..cached });
        }

        // Rate limiting
        self.rate_limiter.acquire().await?;

        // Fetch with timeout
        let response = self.client
            .get(url)
            .timeout(Duration::from_secs(opts.timeout_secs))
            .send()
            .await?;

        let final_url = response.url().to_string();
        let html = response.text().await?;

        // Extract readable content (Mozilla Readability algorithm)
        let article = readability::extract(&html)?;

        // Convert to markdown
        let markdown = html2md::parse_html(&article.content);

        // Apply selector if specified
        let content = if let Some(selector) = &opts.selector {
            self.extract_section(&markdown, selector)?
        } else {
            markdown
        };

        // Truncate if needed
        let (content, truncated) = self.truncate_at_boundary(&content, opts.max_length);

        // Cache result
        self.cache_result(url, &content);

        Ok(FetchResult {
            url: url.to_string(),
            final_url,
            title: article.title,
            content,
            truncated,
            cached: false,
            fetched_at: Utc::now(),
        })
    }
}
```

#### Dependencies

```toml
# Cargo.toml additions
reqwest = { version = "0.12", features = ["json", "gzip", "brotli"] }
scraper = "0.22"  # HTML parsing with CSS selectors
html2md = "0.2"   # HTML to Markdown conversion
lru = "0.12"      # LRU cache for results
governor = "0.8"  # Rate limiting
```

#### Competitive Edges

1. **Automatic markdown conversion** - Agents get training-friendly format
2. **15-minute cache** - Repeated fetches are instant and free
3. **CSS selector support** - Extract specific sections
4. **Rate limiting** - Prevents accidental abuse
5. **Redirect tracking** - Shows final URL for transparency

---

### Tool 2.2: Web Search

**Goal**: Search the web and return structured, actionable results

#### Integration Options

| Option | Cost | Quality | Latency |
|--------|------|---------|---------|
| **SerpAPI** | $50/mo 5k searches | High | ~500ms |
| **Tavily** | $40/mo 3k searches | High (AI-native) | ~800ms |
| **Brave Search API** | Free tier 2k/mo | Medium | ~300ms |
| **DuckDuckGo** | Free | Medium | ~400ms |

**Recommendation**: Start with **Brave Search API** (free tier), upgrade to SerpAPI/Tavily if needed.

#### Schema

```rust
ToolDefinition {
    name: "web_search",
    description: "Search the web and return relevant results. \
        Use for finding documentation, researching libraries, or looking up APIs. \
        Returns snippets and URLs - use web_fetch for full content.",
    input_schema: json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query"
            },
            "max_results": {
                "type": "integer",
                "default": 10,
                "description": "Maximum results to return"
            },
            "site": {
                "type": "string",
                "description": "Limit to specific site (e.g., 'docs.rs', 'stackoverflow.com')"
            },
            "freshness": {
                "type": "string",
                "enum": ["day", "week", "month", "year"],
                "description": "Filter by recency"
            }
        },
        "required": ["query"]
    })
}
```

#### Response Format

```json
{
    "query": "rust async trait object",
    "results": [
        {
            "title": "Async in Traits - The Rust Programming Language",
            "url": "https://doc.rust-lang.org/stable/book/ch17-02-trait-objects.html",
            "snippet": "Learn how to use async methods in trait objects with the async-trait crate..."
        },
        {
            "title": "async-trait - crates.io",
            "url": "https://crates.io/crates/async-trait",
            "snippet": "Type erasure for async trait methods. 12M downloads..."
        }
    ],
    "total_results": 2,
    "cached": false
}
```

---

## Phase 3: Persistent Shell (Priority: HIGH)

### Tool 3.1: Bash with Sessions

**Goal**: Persistent shell sessions that maintain state across calls

#### Design

```
┌─────────────────────────────────────────────────────────┐
│                    Shell Session Pool                    │
├─────────────────────────────────────────────────────────┤
│  Session "main"     Session "build"    Session "test"   │
│  ┌──────────┐       ┌──────────┐       ┌──────────┐    │
│  │ PTY      │       │ PTY      │       │ PTY      │    │
│  │ cwd: /   │       │ cwd: /   │       │ cwd: /   │    │
│  │ env: ... │       │ env: ... │       │ env: ... │    │
│  └──────────┘       └──────────┘       └──────────┘    │
└─────────────────────────────────────────────────────────┘
```

#### Schema

```rust
ToolDefinition {
    name: "bash",
    description: "Execute shell commands in a persistent session. \
        Sessions maintain working directory, environment variables, and shell history. \
        Use session_id to continue in an existing session or start a new one. \
        Commands can run in background with run_in_background=true.",
    input_schema: json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "Shell command to execute"
            },
            "session_id": {
                "type": "string",
                "default": "default",
                "description": "Session identifier (creates if doesn't exist)"
            },
            "run_in_background": {
                "type": "boolean",
                "default": false,
                "description": "Run command in background, return immediately"
            },
            "timeout_secs": {
                "type": "integer",
                "default": 120,
                "description": "Command timeout (max 600 seconds)"
            },
            "cwd": {
                "type": "string",
                "description": "Override working directory for this command only"
            }
        },
        "required": ["command"]
    })
}
```

#### Session Management

```rust
pub struct ShellSessionPool {
    sessions: HashMap<String, ShellSession>,
    max_sessions: usize,
    default_timeout: Duration,
}

pub struct ShellSession {
    id: String,
    pty: PtyHandle,
    cwd: PathBuf,
    env: HashMap<String, String>,
    created_at: DateTime<Utc>,
    last_used: DateTime<Utc>,
    background_tasks: Vec<BackgroundTask>,
}

struct BackgroundTask {
    task_id: String,
    command: String,
    started_at: DateTime<Utc>,
    output_buffer: Arc<RwLock<Vec<u8>>>,
    status: TaskStatus,
}

impl ShellSessionPool {
    pub async fn execute(
        &mut self,
        session_id: &str,
        command: &str,
        opts: &ExecOptions,
    ) -> Result<ExecResult, ShellError> {
        // Get or create session
        let session = self.get_or_create(session_id).await?;

        if opts.run_in_background {
            // Spawn background task
            let task_id = session.spawn_background(command).await?;
            return Ok(ExecResult::Background { task_id });
        }

        // Execute with timeout
        let output = timeout(
            opts.timeout,
            session.execute(command)
        ).await
            .map_err(|_| ShellError::Timeout)?;

        Ok(ExecResult::Complete {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
            cwd: session.cwd.display().to_string(),
        })
    }
}
```

#### Response Formats

**Foreground execution:**
```json
{
    "session_id": "default",
    "command": "cargo build --release",
    "status": "completed",
    "exit_code": 0,
    "stdout": "   Compiling ridge-control v0.1.0...",
    "stderr": "",
    "cwd": "/home/ridgetop/projects/ridge-control",
    "duration_ms": 45230,
    "truncated": false
}
```

**Background execution:**
```json
{
    "session_id": "build",
    "command": "cargo build --release",
    "status": "running",
    "task_id": "bg-a1b2c3",
    "message": "Command running in background. Use bash_output to check status."
}
```

### Tool 3.2: Background Task Output

```rust
ToolDefinition {
    name: "bash_output",
    description: "Get output from a background shell command. \
        Use block=true to wait for completion, block=false for current status.",
    input_schema: json!({
        "type": "object",
        "properties": {
            "task_id": {
                "type": "string",
                "description": "Background task ID from bash command"
            },
            "block": {
                "type": "boolean",
                "default": true,
                "description": "Wait for task completion"
            },
            "timeout_secs": {
                "type": "integer",
                "default": 30,
                "description": "Max time to wait (if blocking)"
            }
        },
        "required": ["task_id"]
    })
}
```

### Tool 3.3: Kill Background Task

```rust
ToolDefinition {
    name: "bash_kill",
    description: "Kill a running background shell command.",
    input_schema: json!({
        "type": "object",
        "properties": {
            "task_id": {
                "type": "string",
                "description": "Background task ID to kill"
            }
        },
        "required": ["task_id"]
    })
}
```

#### Competitive Edges

1. **Named sessions** - Agents can have separate "build", "test", "deploy" contexts
2. **CWD persistence** - `cd` commands persist within session
3. **Environment persistence** - `export` commands persist
4. **Background execution** - Long builds don't block
5. **Output streaming** - Can check partial output while running
6. **Automatic cleanup** - Idle sessions cleaned up after 30 mins

---

## Phase 4: File Operations Enhancement (Priority: MEDIUM)

### Tool 4.1: Edit with Diff Preview

**Enhancement to existing edit tool:**

```rust
ToolDefinition {
    name: "edit",
    description: "Replace exact string in a file with diff preview. \
        Shows unified diff of changes before applying (requires confirmation). \
        The old_string must match exactly including whitespace.",
    input_schema: json!({
        // ... existing fields ...
        "preview_only": {
            "type": "boolean",
            "default": false,
            "description": "Only show diff, don't apply changes"
        },
        "context_lines": {
            "type": "integer",
            "default": 3,
            "description": "Lines of context in diff output"
        }
    })
}
```

#### Response with Diff

```json
{
    "file_path": "src/agent/engine.rs",
    "status": "preview",
    "occurrences": 1,
    "diff": "@@ -45,7 +45,7 @@\n pub struct AgentEngine<S: ThreadStore> {\n     state: AgentState,\n-    config: AgentConfig,\n+    config: Arc<AgentConfig>,\n     thread: Option<AgentThread>,",
    "lines_changed": 1,
    "requires_confirmation": true
}
```

---

## Implementation Phases

### Phase 1: Search Excellence (Week 1)
**Effort**: 3 days
**Value**: 5-10x token savings on search operations

| Day | Task | Deliverable |
|-----|------|-------------|
| 1 | Grep output modes | `files_with_matches`, `content`, `count` |
| 1 | Grep pagination | `head_limit`, `offset`, `next_offset` |
| 2 | Multi-span file_read | `spans` parameter |
| 2 | Response format optimization | JSON structure tuning |
| 3 | Testing & integration | End-to-end tests |

### Phase 2: Web Access (Week 1-2)
**Effort**: 3 days
**Value**: Agents can research documentation, APIs, libraries

| Day | Task | Deliverable |
|-----|------|-------------|
| 4 | web_fetch core | reqwest + readability |
| 4 | Markdown conversion | html2md integration |
| 5 | Caching layer | LRU cache + TTL |
| 5 | web_search | Brave API integration |
| 6 | Rate limiting | governor + config |

### Phase 3: Persistent Shell (Week 2)
**Effort**: 4 days
**Value**: Real build workflows, environment persistence

| Day | Task | Deliverable |
|-----|------|-------------|
| 7 | Session pool | Creation, cleanup, limits |
| 7 | PTY integration | pty-process async |
| 8 | Background execution | spawn, output, kill |
| 9 | State persistence | cwd, env tracking |
| 10 | Testing | Multi-session scenarios |

### Phase 4: Enhancements (Week 3)
**Effort**: 2 days
**Value**: Polish, competitive features

| Day | Task | Deliverable |
|-----|------|-------------|
| 11 | Edit diff preview | Unified diff output |
| 12 | Integration testing | Full agent workflows |

---

## Dependencies

### Cargo.toml Additions

```toml
# Phase 1: Search (already have rg as system dep)
# No new deps

# Phase 2: Web Access
reqwest = { version = "0.12", features = ["json", "gzip", "brotli", "cookies"] }
scraper = "0.22"        # HTML parsing
html2md = "0.2"         # Markdown conversion
lru = "0.12"            # Response caching
governor = "0.8"        # Rate limiting
url = "2.5"             # URL parsing

# Phase 3: Persistent Shell
# pty-process already in deps

# Phase 4: Diff
similar = "2.4"         # Diff generation
```

### System Dependencies

```bash
# Required
ripgrep                 # Already using

# Optional (for ast-grep)
cargo install ast-grep  # Future enhancement
```

---

## Success Metrics

| Metric | Current | Target | Measurement |
|--------|---------|--------|-------------|
| Tokens per search | ~500 | ~50 | `files_with_matches` default |
| Multi-file read calls | 3 | 1 | Multi-span support |
| Build workflow calls | 5+ | 2-3 | Persistent sessions |
| Web research capability | 0% | 100% | web_fetch + web_search |
| Agent task success rate | ? | +20% | Track in Mandrel |

---

## Competitive Advantages Summary

### Over Claude Code

1. **Smart-case grep by default** - Fewer agent errors
2. **`type` filter** - More efficient than glob patterns
3. **Multi-span file read** - 3x fewer calls
4. **Named shell sessions** - Specialized contexts
5. **Web caching** - Repeated fetches free
6. **Mandrel integration** - Cross-session memory (unique)

### Over Cursor

1. **Self-hosted** - No data leaves your machine
2. **Open architecture** - Full customization
3. **Mandrel memory** - Persistent learning

### Over Aider

1. **LSP integration** - Semantic navigation
2. **Sub-agents** - Delegated exploration
3. **Web access** - External research

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| ripgrep not installed | Graceful error, clear install instructions |
| Web fetch blocked/rate-limited | Exponential backoff, cache heavily |
| PTY resource leak | Idle session cleanup, max session limit |
| Large output overwhelms agent | Truncation with clear indicators |

---

## Future Enhancements (Post-Launch)

1. **ast-grep MCP server** - Structural code search
2. **Semantic search** - Embedding-based code discovery
3. **Browser automation** - Playwright for dynamic sites
4. **llms.txt support** - AI-friendly site summaries
5. **Tool usage analytics** - Track what agents use most

---

**Document Version**: 1.0
**Review Status**: Ready for implementation
