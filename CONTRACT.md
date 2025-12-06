# Ridge-Control Contract Specification

**Version**: 1.0.0
**Status**: Active
**Project**: ridge-control
**Language**: Rust
**Framework**: Ratatui
**Target Platform**: Linux only

---

## 1. Project Vision

Ridge-Control is a terminal-based command center that combines a fully functional PTY terminal emulator with custom TUI widgets for process monitoring, log streaming, LLM interaction, and system orchestration. It serves as a central hub for development workflows, AI-assisted coding, and experimental tooling.

**This is not a simple project.** It combines terminal emulation, REST API clients, streaming protocols, and rich TUI design. The complexity is intentional.

---

## 2. Iteration Protocol

### Planning Phase: i[0] through i[9]

Each instance follows this workflow **without exception**:

```
[EXPLORE] → [THINK/PLAN] → [SAVE REASONING]
```

**[EXPLORE]**
- Research relevant technologies, patterns, and prior art
- Read previous instance work thoroughly
- Understand the full context before proceeding
- Use Mandrel `context_search` and `smart_search` to find relevant prior work

**[THINK/PLAN]**
- Instance i[0]: Research and propose **at least 3 approaches** for i[1] to evaluate
- Subsequent instances: Evaluate previous work, refine, expand, or pivot with justification
- Think big picture while maintaining focus on assigned scope
- Avoid tunnel vision - consider how your piece fits the whole

**[SAVE REASONING]**
- Use Mandrel `context_store` with type `planning` or `decision`
- Document your reasoning, not just conclusions
- Include alternatives considered and why rejected

### Building Phase: i[10] and beyond (requires approval)

```
[EXPLORE] → [THINK/PLAN] → [SAVE REASONING] → [BUILD] → [COMMIT/PUSH]
```

**[BUILD]**
- Write production-quality code
- No shortcuts, no "good enough"
- Test your work before claiming completion

**[COMMIT/PUSH]**
- Atomic, logical commits
- Clear commit messages explaining the "why"

### Core Principles (ALL INSTANCES)

1. **No time pressure** - Use tokens, not time. Never rush.
2. **No shortcuts** - Thorough analysis every time
3. **No settling for easy** - Challenge yourself to find the best solution
4. **Pride in craft** - Build something you'd be proud to show
5. **Big picture awareness** - Your piece must fit the whole
6. **Honesty above all** - Never mislead about completion status

---

## 3. Verification & Accountability

### Self-Verification (MANDATORY)

Before declaring any work complete, you MUST:

1. **Review your own work critically** - Would this pass code review?
2. **Verify claims are accurate** - Don't say "complete" if it's partial
3. **Test what you built** - If it's code, does it compile? Does it run?
4. **Check for regressions** - Did you break anything that worked before?

### The Monitor

Your work WILL be checked by **The Monitor**. This is not a suggestion - it is a guarantee.

- Misleading claims will be identified
- Incomplete work marked as complete will be flagged
- Shortcuts will be caught

There is **no reward for deception**. If your part is not complete, say so clearly. Partial progress honestly reported is infinitely more valuable than false completion claims.

### Problem Inheritance

If you discover a problem from a previous instance:

1. **STOP** - Do not continue with your planned work
2. **DOCUMENT** - Record the issue clearly
3. **FIX** - Resolve the problem before proceeding
4. **NOTIFY** - Include in your handoff that you fixed inherited issues

You are responsible for the integrity of everything you touch.

### Tech Debt Protocol

Technical debt is acceptable when:
- It's a conscious trade-off, not laziness
- It's clearly documented
- There's a path to resolution

**All tech debt MUST be documented via Mandrel:**

```
context_store(
  content: "TECH DEBT: [description of debt, why it exists, and remediation path]",
  type: "handoff",
  tags: ["tech-debt", "ridge-control"]
)
```

### Test & Mock Data

Any test data, mock data, placeholder values, or stub implementations MUST be:

1. **Clearly marked** in code comments: `// TEST DATA - REMOVE BEFORE PRODUCTION`
2. **Documented** in handoff context
3. **Isolated** - easy to identify and remove
4. **Never presented as real functionality**

---

## 4. Hard Requirements (Non-Negotiable)

### 4.1 Architecture Overview

```
┌─────────────────────────────────────┬─────────────────────────┐
│                                     │    Process Monitor      │
│                                     │    - Running processes  │
│         PTY Terminal                │    - Click to kill      │
│         (Full shell)                │    - CPU/GPU metrics    │
│                                     ├─────────────────────────┤
│         + LLM Integration           │    Interactive Menu     │
│         (Claude API)                │    - Logs               │
│                                     │    - Spindles           │
│                                     │    - Forge              │
│                                     │    - Config             │
│                                     │    - [Dynamic streams]  │
└─────────────────────────────────────┴─────────────────────────┘
              Left 2/3                        Right 1/3
                              MAIN TAB
```

### 4.2 Core Components

#### PTY Terminal (Left 2/3)
- Full PTY implementation - must run bash, zsh, or user's shell
- Must be able to run `claude` CLI (Claude Code)
- Must be able to run any arbitrary command
- Full ANSI escape sequence support
- Scrollback buffer
- Mouse selection for copy/paste

#### LLM Integration (Built into terminal pane)
- Direct REST API client to Claude API (Anthropic)
- Tool use capability matching Claude Code patterns:
  - File read/write/edit
  - Bash command execution
  - Search (glob/grep equivalent)
  - Web fetch (if applicable)
- Support for `--dangerously-allow-all` equivalent flag
- Streaming responses
- Extended thinking (thinking blocks) support

#### Process Monitor (Top Right)
- Display running processes (filterable)
- Click-to-kill functionality (mouse required)
- CPU usage indicator
- GPU usage indicator (Linux: nvidia-smi/rocm-smi parsing)
- Visual strain indicators (how stressed is the system)

#### Interactive Menu (Bottom Right)
- Dynamic menu items from configuration
- State machine per menu item:
  - Menu view (default)
  - Viewer mode (on Enter)
  - Escape returns to menu

#### Built-in Viewers
- **Logs**: Streaming log viewer with auto-scroll
- **Spindles**: Thinking block stream viewer (from spindles-proxy or configured endpoint)
- **Forge**: Orchestration run viewer (from Forge system or configured endpoint)
- **Config**: Settings panel (see 4.4)

### 4.3 Tab System

- Multiple tabs supported
- Main tab: "Ridge-Control" with layout above
- Additional tabs: User-configurable split panes (tmux-style)
- Tab creation, closing, renaming
- Keyboard navigation between tabs

### 4.4 Configuration System

All configuration via files - NEVER hard-coded values.

**Required config options:**
- API keys (secure storage - keyring or encrypted, NEVER plaintext in config)
- Model provider selection
- Add/remove model providers
- Model selection per provider
- Add/remove models
- Stream endpoint definitions (see 4.5)
- Keybindings (customizable)
- Color theme (customizable)
- Any other runtime options

**Config locations:**
- `~/.config/ridge-control/config.toml` (or yaml - builder's choice)
- `~/.config/ridge-control/streams.toml` (stream definitions)
- `~/.config/ridge-control/keys/` (secure key storage)

### 4.5 Pluggable Stream Architecture (CRITICAL)

Instead of hard-coding data sources, implement a configurable stream system:

```toml
# Example stream configuration
[[streams]]
name = "forge-logs"
protocol = "websocket"
endpoint = "ws://localhost:8081/forge/stream"
viewer = "log"
auto_connect = false

[[streams]]
name = "spindles"
protocol = "websocket"
endpoint = "ws://localhost:8082/spindles"
viewer = "log"
auto_connect = true

[[streams]]
name = "mandrel-search"
protocol = "rest"
endpoint = "http://localhost:8080/mcp/tools/context_search"
viewer = "list"
poll_interval_ms = 5000

[[streams]]
name = "custom-metrics"
protocol = "unix_socket"
path = "/tmp/my-app.sock"
viewer = "log"
```

**Required protocol support:**
- WebSocket
- REST (HTTP/HTTPS)
- Unix domain socket
- TCP socket

**Viewer types:**
- `log` - Streaming log viewer (scrollable, auto-scroll toggle)
- `list` - List/table viewer (for structured data)
- Additional types at builder discretion

**Menu generation:**
The interactive menu SHALL dynamically render available streams from configuration. Hard-coded menu items are only: Config (always present).

### 4.6 Model Providers

Support these providers via REST API:
- Anthropic (Claude models)
- OpenAI (GPT models)
- xAI (Grok models)
- Google (Gemini models)
- Groq (as inference provider)

Provider configuration is additive - users can add custom providers following a standard schema.

### 4.7 Input & Interaction

- **Keyboard**: Full keyboard navigation, vim-style bindings encouraged
- **Mouse**: Required
  - Click to focus panes
  - Click to select menu items
  - Click to kill processes
  - Click and drag to select text for copy
  - Right-click context menus (optional but encouraged)
- **Copy/Paste**: Must work via mouse selection

### 4.8 Visual Design

- **Rich colors**: No muted palettes, no "safe" choices. Bold, vibrant, purposeful.
- **Digital braille aesthetic**: Clean, technical, precise
- **Animations**:
  - Progress indicators with character-based animation
  - Spinners with digital/braille-style frames
  - Smooth transitions where possible
- **Nerd Font glyphs**: Use freely for icons and indicators
- **No generic "TUI gray"**: This should look distinctive

---

## 5. Soft Requirements (Desired but Flexible)

These are goals, not mandates. Builders have discretion.

- Startup time under 100ms
- Memory-efficient scrollback (don't load entire history into RAM)
- Responsive UI even during heavy streaming
- Graceful degradation if a stream endpoint is unavailable
- Session persistence (restore tabs/layout on restart)
- Command palette (fuzzy search for actions)
- Notification system for background events
- Split pane resizing via keyboard or mouse drag
- Search within log viewers
- Log filtering/grep

---

## 6. Explicitly Open (Builder Decides)

These decisions are left to the builders. Document your choices.

- Internal state management architecture
- Async runtime choice (tokio, async-std, etc.)
- Configuration file format (TOML, YAML, etc.)
- Exact keybinding defaults
- Color palette specifics (within "rich and bold" constraint)
- Animation frame sequences
- Error handling patterns
- Logging framework
- Directory structure within `src/`
- Module organization
- Testing strategy
- How secure key storage is implemented (keyring API, encrypted file, etc.)

---

## 7. Technical Constraints

### Must Use
- **Language**: Rust (latest stable)
- **TUI Framework**: Ratatui
- **Platform**: Linux only (no Windows/macOS compatibility required)

### Must Not
- Hard-code API keys anywhere
- Hard-code endpoint URLs (use config)
- Use unsafe Rust without clear justification and documentation
- Introduce dependencies without justification
- Ignore errors (handle or propagate, never swallow)

---

## 8. Project Structure

```
ridge-control/
├── CONTRACT.md          # This file (do not modify)
├── README.md            # Project readme (builders maintain)
├── Cargo.toml           # Rust manifest
├── Cargo.lock           # Dependency lock
├── docs/                # Documentation
│   └── [builder docs]   # Architecture decisions, API docs, etc.
├── src/                 # Source code
│   └── [builder structure]
├── tests/               # Integration tests
└── examples/            # Usage examples (if applicable)
```

**Keep it tidy:**
- No excessive markdown files
- No orphaned files
- Clear naming conventions
- Logical organization

---

## 9. Mandrel Integration

Use Mandrel for:

### Context Storage
```
context_store(content, type, tags)
```
- `type: "planning"` - For plans and architectural thinking
- `type: "decision"` - For technical decisions with rationale
- `type: "handoff"` - For end-of-instance handoffs
- `type: "error"` - For issues encountered
- `tags: ["ridge-control", ...]` - Always include project tag

### Decision Recording
```
decision_record(decisionType, title, description, rationale, impactLevel, alternativesConsidered)
```
Use for significant architectural or technical decisions.

### Task Tracking (if needed)
```
task_create(title, description, type, priority)
task_update(taskId, status)
```

### Searching Prior Work
```
context_search(query, tags: ["ridge-control"])
smart_search(query, projectId)
```

---

## 10. Handoff Protocol

At the end of each instance, you MUST save a handoff context:

```
context_store(
  content: "[Structured handoff - see template below]",
  type: "handoff",
  tags: ["ridge-control", "i[N]", "handoff"]
)
```

**Handoff Template:**

```markdown
# Instance i[N] Handoff

## What I Accomplished
[Bullet points of concrete deliverables]

## Key Decisions Made
[Decisions with brief rationale]

## Open Questions
[Questions that need resolution]

## Known Issues
[Any bugs, concerns, or problems identified]

## Tech Debt Introduced
[Any shortcuts taken with remediation path]

## Recommendations for Next Instance
[What should i[N+1] focus on]

## Files Modified/Created
[List of files touched]
```

---

## 11. Definition of Done

An instance's work is complete when:

1. All claimed deliverables are actually delivered
2. Code compiles without errors (if code was written)
3. No known breaking issues left undocumented
4. Handoff context saved to Mandrel
5. Any inherited problems from previous instances are fixed or documented
6. Tech debt is documented if introduced
7. Self-review completed honestly

**Remember: Partial completion honestly reported > False completion claims**

---

## 12. Reference Materials

### External Resources (for research)
- Ratatui documentation: https://ratatui.rs/
- Ratatui GitHub: https://github.com/ratatui/ratatui
- Anthropic API documentation: https://docs.anthropic.com/
- PTY handling in Rust: Research `portable-pty`, `pty-process` crates
- Terminal emulation: Research VT100/ANSI escape sequences

### Internal Resources
- Mandrel MCP Server: Available via tools
- Spindles proxy: `~/mandrel/spindles-proxy` (existing logic reference)
- Forge system: Research existing `forge` directory structure

### Claude Code Reference
- Study Claude Code's tool use patterns
- Understand file operations, bash execution, search capabilities
- The goal is feature parity where applicable

---

## 13. Final Notes

This project is an experiment in AI-driven development. You are part of a chain of instances building something ambitious.

**Your constraints:**
- This contract (non-negotiable requirements)
- The laws of physics and computing
- Honesty

**Your freedoms:**
- Everything else

Build something remarkable. Take pride in your work. The next instance is counting on you.

---

*Contract Version 1.0.0 - Do not modify without explicit approval*
