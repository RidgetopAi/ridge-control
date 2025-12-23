# Ridge-Control Search Tools Design

## Research Summary

Analyzed search implementations from:
- **Anthropic Claude Code CLI** - Built-in Grep, Glob, Read, LSP tools
- **Sourcegraph Amp** - finder, Grep, glob tools (currently using)
- **Oracle Analysis** - LLM-centric design principles

---

## Core Insight: LLM Search is Different

Traditional search optimizes for humans who can scan results visually. LLM search must optimize for:

| Concern | Human Search | LLM Search |
|---------|-------------|------------|
| **Context limits** | Unlimited scroll | 200k tokens max |
| **Pattern recognition** | Visual scanning | Sequential text processing |
| **Iteration cost** | Cheap (click around) | Expensive (each call uses tokens) |
| **Decision-making** | Intuitive | Needs explicit metadata |

---

## Recommended Tool Set (Priority Order)

### Tier 1: Essential (Implement First)

#### 1. `grep` - Exact Text Search
The bread-and-butter tool. Wraps ripgrep with structured output.

```rust
ToolDefinition {
    name: "grep",
    description: "Search for exact text or regex patterns in files. Returns matches with surrounding context.",
    input_schema: {
        "pattern": "string - The text or regex to search for",
        "path": "string (optional) - Directory or file to search in",
        "include": "[string] (optional) - Glob patterns to include (e.g. ['*.rs', '*.toml'])",
        "exclude": "[string] (optional) - Glob patterns to exclude (e.g. ['target/**'])",
        "literal": "bool (optional) - Treat pattern as literal text, not regex",
        "case_sensitive": "bool (optional) - Case sensitive matching",
        "context_lines": "int (optional, default 2) - Lines of context before/after match",
        "max_results": "int (optional, default 50) - Maximum matches to return"
    }
}
```

**Output Structure:**
```json
{
  "matches": [
    {
      "path": "src/agent/engine.rs",
      "line": 270,
      "column": 12,
      "preview": [
        {"line": 268, "text": "            self.transition(AgentState::Error);"},
        {"line": 269, "text": "        }"},
        {"line": 270, "text": "        LLMEvent::ToolUseDetected(tool_use) => {", "match": true},
        {"line": 271, "text": "            self.pending_tools.push(tool_use.clone());"},
        {"line": 272, "text": "            // Add ToolUse to current_response"}
      ],
      "match_range": [12, 32]
    }
  ],
  "total_matches": 5,
  "truncated": false
}
```

**Why this design:**
- `path` scope prevents searching entire filesystem
- `include/exclude` lets LLM narrow scope incrementally  
- `context_lines` controllable = token-efficient
- `max_results` with `truncated` flag = LLM knows if it's missing results
- Structured `preview` = parseable, not just raw text

---

#### 2. `glob` - File Pattern Search
Find files by name/pattern without reading contents.

```rust
ToolDefinition {
    name: "glob",
    description: "Find files matching a glob pattern. Returns paths only, no content.",
    input_schema: {
        "pattern": "string - Glob pattern (e.g. 'src/**/*.rs', '**/test_*.py')",
        "max_results": "int (optional, default 100) - Maximum files to return",
        "sort_by": "string (optional) - 'path' | 'mtime' | 'size'"
    }
}
```

**Output:**
```json
{
  "files": [
    {"path": "src/agent/engine.rs", "size": 15234, "modified": "2025-12-23T10:00:00Z"},
    {"path": "src/agent/context.rs", "size": 8921, "modified": "2025-12-22T15:30:00Z"}
  ],
  "total_found": 47,
  "truncated": false
}
```

**Why essential:**
- LLM needs to discover file structure before searching content
- Cheap operation (no file reads) = fast iteration
- Metadata helps LLM prioritize (recently modified = more relevant)

---

#### 3. `read_file` (Enhanced) - Multi-Span Reading
Upgrade existing `file_read` to support multiple line ranges.

```rust
ToolDefinition {
    name: "read_file",
    description: "Read specific line ranges from a file. Supports multiple spans in one call.",
    input_schema: {
        "path": "string - Path to file",
        "spans": "[{start: int, end: int}] (optional) - Line ranges to read",
        "max_lines": "int (optional, default 500) - Maximum total lines"
    }
}
```

**Output:**
```json
{
  "path": "src/agent/engine.rs",
  "total_lines": 480,
  "chunks": [
    {"start": 1, "end": 50, "content": "// File header...\n..."},
    {"start": 265, "end": 280, "content": "LLMEvent::ToolUseDetected..."}
  ],
  "truncated": false
}
```

**Why multi-span matters:**
- After grep finds 3 matches in one file, LLM can read all 3 areas in ONE call
- Saves 2 round-trips = faster, fewer tokens wasted on tool call overhead

---

### Tier 2: High Value (Implement Second)

#### 4. `find_symbol` - Structural Search
Jump to definitions without grepping.

```rust
ToolDefinition {
    name: "find_symbol",
    description: "Find function, struct, trait, or type definitions by name.",
    input_schema: {
        "name": "string - Symbol name (exact or partial)",
        "kind": "string (optional) - 'function' | 'struct' | 'trait' | 'type' | 'const' | 'any'",
        "path": "string (optional) - Scope search to directory",
        "max_results": "int (optional, default 20)"
    }
}
```

**Output:**
```json
{
  "symbols": [
    {
      "name": "AgentEngine",
      "kind": "struct",
      "path": "src/agent/engine.rs",
      "line": 45,
      "signature": "pub struct AgentEngine<S: ThreadStore>",
      "doc": "/// Main agent execution engine"
    }
  ]
}
```

**Implementation:** Use tree-sitter or ctags for fast parsing.

---

#### 5. `find_references` - Usage Search
Find where a symbol is used.

```rust
ToolDefinition {
    name: "find_references",
    description: "Find all usages of a symbol (function calls, type references, imports).",
    input_schema: {
        "path": "string - File containing the symbol",
        "line": "int - Line number of symbol definition",
        "max_results": "int (optional, default 50)"
    }
}
```

---

#### 6. `tree` - Directory Structure
Show directory tree with controllable depth.

```rust
ToolDefinition {
    name: "tree",
    description: "Show directory structure as a tree.",
    input_schema: {
        "path": "string (optional, default '.') - Root directory",
        "depth": "int (optional, default 3) - Maximum depth",
        "include": "[string] (optional) - Glob patterns to include",
        "show_hidden": "bool (optional, default false)"
    }
}
```

---

### Tier 3: Advanced (Future)

#### 7. `semantic_search` - AI-Powered Search
"Find code that handles authentication" - conceptual search.

```rust
ToolDefinition {
    name: "semantic_search",
    description: "Search for code by concept or behavior, not exact text.",
    input_schema: {
        "query": "string - Natural language description of what you're looking for",
        "path": "string (optional) - Scope to directory",
        "top_k": "int (optional, default 10) - Number of results"
    }
}
```

**Implementation:** Requires embedding index (expensive to build, powerful once done).

---

## Design Principles

### 1. Always Include Truncation Info
```json
{
  "results": [...],
  "total_found": 150,
  "returned": 50,
  "truncated": true  // LLM knows to narrow search
}
```

### 2. Metadata Over Raw Content
Instead of dumping file contents, include:
- Line numbers
- File paths (always absolute or workspace-relative)
- Match ranges (column positions)
- Symbol kinds (function, struct, etc.)
- Timestamps (for relevance)

### 3. Controllable Scope
Every search tool should support:
- Path restriction (directory or file)
- Include/exclude patterns
- Result limits with explicit truncation

### 4. Composable Results
Output from one tool should feed into another:
```
glob("**/*.rs") → grep(pattern, path=results[0]) → read_file(path, spans=[...])
```

---

## Implementation Priority

| Priority | Tool | Effort | Value |
|----------|------|--------|-------|
| P0 | `grep` | Medium | Critical - basic search |
| P0 | `glob` | Small | Critical - file discovery |
| P1 | `read_file` (multi-span) | Small | High - token efficiency |
| P1 | `tree` | Small | High - orientation |
| P2 | `find_symbol` | Medium | High - precision search |
| P2 | `find_references` | Medium | Medium - usage tracking |
| P3 | `semantic_search` | Large | High but complex |

---

## Comparison: Current vs Proposed

| Capability | Current | Proposed |
|------------|---------|----------|
| Find text in files | `bash_execute("rg ...")` | `grep` with structured output |
| Find files by name | `bash_execute("find ...")` | `glob` with metadata |
| Read file portions | `file_read` (whole file) | `read_file` with spans |
| Understand structure | None | `tree`, `find_symbol` |
| Conceptual search | None | `semantic_search` (future) |

---

## Next Steps

1. **Implement `grep`** - Wrap ripgrep, add structured JSON output
2. **Implement `glob`** - Use Rust glob crate, add metadata
3. **Enhance `read_file`** - Add multi-span support
4. **Add `tree`** - Simple directory walker with depth limit
5. **Test end-to-end** - Verify LLM can chain tools effectively

---

## References

- [Anthropic Claude Code](https://github.com/anthropics/claude-code) - Grep, Glob, Read, LSP tools
- [Sourcegraph Amp](https://ampcode.com/manual) - finder, Grep, glob
- Oracle analysis on LLM search primitives
