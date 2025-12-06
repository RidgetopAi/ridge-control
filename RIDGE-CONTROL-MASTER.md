# Ridge-Control Master Plan

**Version**: 1.0.0  
**Status**: Planning Complete - Ready for Build Phase  
**Consolidated By**: Instance i[8]  
**Source**: Instances i[0] through i[7]  

---

## Executive Summary

Ridge-Control is a terminal-based command center combining a fully functional PTY terminal emulator with custom TUI widgets for process monitoring, log streaming, LLM interaction, and system orchestration. This document consolidates all planning decisions from instances i[0]-i[7] into a formalized implementation roadmap.

**Planning Phase Status**: ✅ COMPLETE  
**Build Phase Ready**: Instance i[10]

---

## Table of Contents

1. [Architecture Decisions](#1-architecture-decisions)
2. [Dependency Selection](#2-dependency-selection)
3. [Type System](#3-type-system)
4. [Configuration System](#4-configuration-system)
5. [Module Structure](#5-module-structure)
6. [Build Phase Roadmap](#6-build-phase-roadmap)
7. [MVP Definition](#7-mvp-definition-i10)
8. [Testing Strategy](#8-testing-strategy)

---

## 1. Architecture Decisions

### 1.1 Overall Pattern

**Decision**: Component-Local State with crossbeam bridge (Ratatui-native + GitUI hybrid)

**Event Flow**:
```
Event → App::handle() → Action → App::dispatch()
```

**Key Characteristics**:
- Tokio async runtime in separate thread
- Bridged to main TUI loop via crossbeam channels
- Component trait for standardized lifecycle
- Action enum for all state mutations

### 1.2 Async Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│                           MAIN EVENT LOOP                               │
├────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
│  │  Crossterm  │  │    PTY      │  │   Streams   │  │   Timers    │   │
│  │   Events    │  │   Output    │  │   (WS/SSE)  │  │   (Tick)    │   │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘   │
│         │                │                │                │          │
│         ▼                ▼                ▼                ▼          │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │                      tokio::select!                              │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│                                    │                                   │
│                                    ▼                                   │
│                            ┌───────────────┐                          │
│                            │ Event → Action│                          │
│                            └───────────────┘                          │
└────────────────────────────────────────────────────────────────────────┘
```

### 1.3 Input Mode State Machine

```rust
enum InputMode {
    PtyRaw,                    // All input → PTY (Ctrl+Esc exits)
    Normal,                    // Vim navigation
    Insert { target },         // Text input for filters/rename
    CommandPalette,            // Fuzzy command search
    Confirm { action },        // Confirmation dialog
}
```

**State Transitions**:
```
Start → Normal ←─────────────────────────────────┐
          │                                       │
    ┌─────┼─────────────┐                        │
    │     │             │                        │
    ▼     ▼             ▼                        │
 PtyRaw  Insert    CommandPalette               │
    │      │             │                       │
    └──────┴─────────────┴── (Esc/Complete) ────┘
```

### 1.4 Focus Management

```rust
enum FocusArea {
    Terminal,       // Left 2/3
    ProcessMonitor, // Top-right
    Menu,           // Bottom-right
    StreamViewer,   // When menu item is active
    ConfigPanel,    // When config is open
}
```

**Focus Ring**: Tab cycles through `[Terminal, ProcessMonitor, Menu]`

---

## 2. Dependency Selection

| Domain | Crate | Rationale |
|--------|-------|-----------|
| TUI | `ratatui` | CONTRACT requirement |
| PTY | `pty-process` | Flexibility, trait-based |
| ANSI Parser | `vte 0.15+` | Proven, Alacritty-based |
| Grid Storage | Ring buffer | Alacritty pattern, O(1) memory |
| HTTP Client | `reqwest` + stream | Async, SSE support |
| WebSocket | `tokio-tungstenite` | Async WebSocket |
| Fuzzy Search | `nucleo` | 6-7x faster than skim, TUI-native |
| Clipboard | `arboard` | Pure Rust, X11/Wayland |
| Secure Keys | `keyring` + encrypted fallback | Linux-native, no plaintext |
| Config | `toml` | Human-readable, Rust-idiomatic |
| Error Handling | `thiserror` + `anyhow` + `color-eyre` | Hybrid pattern like GitUI |
| Process Monitor | `procfs` | Lightweight, iterator-based |
| Testing | `rstest` + `serial_test` + TestBackend | Layered approach |

---

## 3. Type System

### 3.1 Event Types

```rust
pub enum Event {
    Input(crossterm::event::Event),
    Pty(PtyEvent),
    Stream(StreamEvent),
    Tick,
    ConfigChanged(PathBuf),
    ProcessUpdate(Vec<ProcessInfo>),
    Error(AsyncError),
}

pub enum PtyEvent {
    Output(Vec<u8>),
    Exited(i32),
    Error(std::io::Error),
}

pub enum StreamEvent {
    Connected(StreamId),
    Disconnected(StreamId, Option<String>),
    Data(StreamId, StreamData),
    Error(StreamId, String),
}
```

### 3.2 Action Types (~70 variants)

**Grouped Categories**:
- **Lifecycle**: Quit, ForceQuit, Suspend, Reload
- **Mode**: EnterPtyMode, EnterNormalMode, EnterInsertMode, OpenCommandPalette
- **Focus**: FocusNext, FocusPrev, FocusArea(FocusArea)
- **Navigation**: ScrollUp, ScrollDown, ScrollPageUp/Down, ScrollToTop/Bottom
- **PTY**: PtyInput, PtyResize, PtyClear, PtyScrollback
- **Tab**: TabNew, TabClose, TabRename, TabGoto, TabNext, TabPrev
- **Stream**: StreamConnect, StreamDisconnect, StreamToggle, StreamSetFilter
- **Process**: ProcessKill, ProcessSelect, ProcessFilter, ProcessRefresh
- **Menu**: MenuSelect, MenuEnter, MenuBack
- **CommandPalette**: CommandPaletteInput, CommandPaletteExecute
- **LLM**: LlmSendMessage, LlmCancel, LlmSelectModel, LlmSelectProvider
- **Config**: ConfigOpen, ConfigClose, ConfigSave, ConfigSetValue
- **Clipboard**: Copy, Paste
- **Notifications**: Notify, DismissNotification
- **Internal**: Tick, Render, Batch

### 3.3 LLM Types

```rust
pub struct LLMRequest {
    model: String,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    max_tokens: Option<u32>,
    thinking: Option<ThinkingConfig>,
    stream: bool,
}

pub struct Message {
    role: Role,  // User, Assistant
    content: Vec<ContentBlock>,
}

pub enum ContentBlock {
    Text(String),
    Image(ImageContent),
    ToolUse(ToolUse),
    ToolResult(ToolResult),
    Thinking(String),
}

pub enum StreamChunk {
    Start { message_id: String },
    BlockStart { index: usize, block_type: BlockType },
    Delta(StreamDelta),
    BlockStop { index: usize },
    Stop { reason: StopReason, usage: Option<Usage> },
    Error(LLMError),
}
```

### 3.4 Stream Types

```rust
pub trait StreamClient: Send + Sync {
    fn id(&self) -> &StreamId;
    fn name(&self) -> &str;
    fn protocol(&self) -> StreamProtocol;
    fn state(&self) -> ConnectionState;
    async fn connect(&mut self) -> Result<(), StreamError>;
    async fn disconnect(&mut self) -> Result<(), StreamError>;
    async fn send(&mut self, data: &[u8]) -> Result<(), StreamError>;
    fn subscribe(&self) -> mpsc::UnboundedReceiver<StreamEvent>;
}

pub enum StreamProtocol {
    WebSocket, SSE, RestPoll, UnixSocket, Tcp,
}

pub enum ConnectionState {
    Disconnected, Connecting, Connected, Reconnecting { attempt: u32 }, Failed,
}
```

---

## 4. Configuration System

### 4.1 File Locations

| File | Purpose |
|------|---------|
| `~/.config/ridge-control/config.toml` | Main configuration |
| `~/.config/ridge-control/keybindings.toml` | Helix-style keybindings |
| `~/.config/ridge-control/theme.toml` | Colors, icons, focus indicators |
| `~/.config/ridge-control/streams.toml` | Stream endpoint definitions |
| `~/.config/ridge-control/providers.toml` | LLM provider/model config |
| `~/.config/ridge-control/session.toml` | Tab persistence |
| `~/.config/ridge-control/keys/` | Encrypted key fallback storage |

### 4.2 Keybindings Format (Helix-style)

```toml
[keys.normal]
"C-s" = "save"
"Tab" = "focus_next"

[keys.normal.g]
g = "scroll_to_top"
e = "scroll_to_bottom"

[keys.pty]
"C-Esc" = "enter_normal_mode"

[keys.insert]
"Esc" = "cancel"
"Enter" = "confirm"
```

**Modifier Syntax**: `C-` (Ctrl), `A-` (Alt), `S-` (Shift), `M-` (Meta/Super)

### 4.3 Theme Format

```toml
[colors.base]
background = "#1e1e2e"
surface = "#313244"
overlay = "#45475a"

[colors.accent]
primary = "#cba6f7"    # Mauve
secondary = "#89b4fa"  # Blue
tertiary = "#a6e3a1"   # Green

[colors.focus]
active_border = "#f5c2e7"
active_title = "#f5e0dc"

[icons]
terminal = ""
process = ""
stream_connected = "󰌘"
stream_disconnected = "󰌙"
```

### 4.4 Providers Format

```toml
[defaults]
provider = "anthropic"
model = "claude-sonnet-4-20250514"

[[providers]]
name = "anthropic"
type = "anthropic"
base_url = "https://api.anthropic.com/v1"
api_key_ref = "anthropic"  # Reference to keyring, not actual key
enabled = true

[[providers.models]]
id = "claude-sonnet-4-20250514"
name = "Claude Sonnet 4"
context_window = 200000
vision = true
tools = true
thinking = true
```

**API Key Resolution**: Keys stored in system keyring with service `ridge-control`

---

## 5. Module Structure

```
src/
├── main.rs              # Entry point, panic hook, color-eyre
├── app.rs               # App struct, event loop
├── action.rs            # Action enum (~70 variants)
├── event.rs             # Event enum, EventFilter
├── error.rs             # RidgeError with thiserror
├── config/
│   ├── mod.rs           # Config loading
│   ├── theme.rs         # Theme types
│   └── keybindings.rs   # Keybinding types
├── pty/
│   ├── mod.rs           # PTY management
│   ├── grid.rs          # Grid + Cell + ring buffer
│   └── parser.rs        # VTE integration
├── components/
│   ├── mod.rs           # Component trait
│   ├── terminal.rs      # PTY display widget
│   ├── process_monitor.rs
│   ├── menu.rs
│   ├── stream_viewer.rs
│   └── placeholder.rs   # For MVP
├── input/
│   ├── mod.rs
│   ├── mode.rs          # InputMode, InsertTarget
│   └── focus.rs         # FocusManager, FocusArea
├── streams/
│   ├── mod.rs           # StreamManager
│   ├── client.rs        # StreamClient trait
│   ├── websocket.rs
│   ├── sse.rs
│   └── rest.rs
└── llm/
    ├── mod.rs           # LLM coordinator
    ├── types.rs         # Request/Response types
    ├── provider.rs      # Provider trait
    ├── anthropic.rs
    ├── openai_compat.rs # OpenAI/xAI/Groq
    └── gemini.rs
```

---

## 6. Build Phase Roadmap

### Phase 1: Foundation (i[10]-i[12])

| Instance | Focus | Deliverable | Success Criteria |
|----------|-------|-------------|------------------|
| **i[10]** | MVP | Layout + PTY + Focus + Shutdown | `cargo run` shows layout, Tab switches focus, terminal works, Ctrl+C exits |
| **i[11]** | PTY Polish | Scrollback, mouse selection, ANSI colors | Full color support, copy text works |
| **i[12]** | Process Monitor | procfs integration, click-to-kill | Shows processes, clicking kills them |

### Phase 2: Streams & Networking (i[13]-i[15])

| Instance | Focus | Deliverable | Success Criteria |
|----------|-------|-------------|------------------|
| **i[13]** | Menu + Streams | Stream config loading, WebSocket client | Menu shows streams, can connect |
| **i[14]** | LLM Integration | Provider trait, Anthropic impl, streaming | Can send message, see streaming response |
| **i[15]** | Tool Execution | Tool confirmation UI, sandboxed execution | LLM can use tools with user approval |

### Phase 3: User Experience (i[16]-i[18])

| Instance | Focus | Deliverable | Success Criteria |
|----------|-------|-------------|------------------|
| **i[16]** | Command Palette | nucleo integration, action dispatch | `:` opens palette, can execute commands |
| **i[17]** | Config System | Hot-reload, keybindings, theme | Config changes apply without restart |
| **i[18]** | Tab System | Multi-tab, session persistence | Multiple tabs, layout persists |

### Phase 4: Polish (i[19]-i[20])

| Instance | Focus | Deliverable | Success Criteria |
|----------|-------|-------------|------------------|
| **i[19]** | Visual Polish | Animations, spinners, icons | Feels polished and responsive |
| **i[20]** | Testing & CI | Integration tests, CI setup | Tests pass, CI green |

---

## 7. MVP Definition (i[10])

### Must Have

1. **Main layout** - 2/3 left (terminal), 1/3 right (placeholder widgets)
2. **PTY spawn** - bash/zsh with basic input/output
3. **VTE parsing** - Basic ANSI rendering
4. **Focus switching** - Tab between terminal and right pane
5. **Mode switching** - PtyRaw vs Normal
6. **Clean shutdown** - Ctrl+C with terminal restore

### NOT in MVP

- Streams, LLM, process monitor, command palette
- Config hot-reload, tab system, session persistence
- Mouse selection, scrollback history
- Theme customization

### Success Criteria

```bash
cargo run
# Split layout visible
# Tab switches focus (visual indicator)
# Terminal accepts input, runs commands
# Ctrl+C exits cleanly (terminal restored)
```

---

## 8. Testing Strategy

### Layered Approach

| Layer | Tool | Purpose |
|-------|------|---------|
| Unit | TestBackend | Widget rendering tests (fast, deterministic) |
| Async | #[serial] | Stream/async tests (prevent race conditions) |
| Integration | conditional PTY | Real PTY e2e tests (Linux-only) |
| Parametric | rstest | Scenario coverage |

### Test Structure

```
src/
├── pty/mod.rs
│   └── #[cfg(test)] mod tests { ... }

tests/
├── integration/
│   ├── util.rs           # spawn_pty(), fixtures
│   └── pty_e2e.rs        # Real PTY tests (Linux-only)
```

### Key Patterns

- `#[cfg(target_os = "linux")]` for PTY e2e tests
- `#[serial]` for async tests
- TestBackend for widget tests
- Mock providers for LLM tests

---

## Appendix A: Key Decisions Summary

| Area | Decision | Rationale |
|------|----------|-----------|
| Architecture | Component-Local State | Ratatui-native, proven in GitUI |
| PTY | pty-process | Trait-based, flexible |
| Grid | Ring buffer | O(1) memory, Alacritty pattern |
| Async | Tokio + crossbeam bridge | Separate threads, clean separation |
| Fuzzy | nucleo | 6-7x faster than alternatives |
| Config | TOML files | Human-readable, Rust idiomatic |
| Keybindings | Helix-style | Vim-familiar, production-proven |
| LLM | Unified types + Provider trait | Single abstraction for all providers |
| Tool execution | Sandboxed with validation | Defense in depth |
| Token management | Sliding window | Simple, predictable |
| Failover | Configurable with smart defaults | Balance reliability vs pointless retries |

---

## Appendix B: Open Items (None)

All planning questions have been resolved. The planning phase is complete.

---

## Appendix C: Instance Contributions

| Instance | Contribution |
|----------|--------------|
| i[0] | Architecture research, 3 approaches proposed, dependency selection |
| i[1] | Component trait validation |
| i[2] | PTY implementation details, secure key storage |
| i[3] | Config hot-reload, reconnection strategy, process monitor, session persistence |
| i[4] | Fuzzy search, input mode state machine, focus management, testing strategy |
| i[5] | Keybindings config, theme system, Action enum, async event routing |
| i[6] | LLM provider abstraction, unified message types, stream client trait |
| i[7] | Tool execution sandboxing, conversation history, multi-turn tool UI, failover |
| i[8] | Plan formalization and documentation (this document) |

---

*Document created by Instance i[8] • Planning Phase Complete • Build Phase Ready*
