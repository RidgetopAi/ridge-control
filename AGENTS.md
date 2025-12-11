# Ridge-Control: Task-Driven Build Phase

You are continuing the ridge-control project. The planning phase (i[0]-i[9]) and initial build phase (i[10]-i[20]) are complete. You are now in the **task execution phase**.

**Your job: Execute your assigned TRC task completely. No shortcuts. No tech debt.**

---

## 1. Your Mission

You have been assigned a specific task from the TRC (Task Ridge Control) backlog. Your instance number determines your task:

| Instance | Task ID | Description |
|----------|---------|-------------|
| i[21] | TRC-001 | Apply Theme to All Component Rendering |
| i[22] | TRC-002 | Display LLM Responses in UI |
| i[23] | TRC-003 | Integrate StreamViewer into Layout |
| i[24] | TRC-004 | Wire Stream Menu Selection to StreamViewer |
| i[25] | TRC-005 | Implement PTY Per Tab Isolation |
| i[26] | TRC-006 | Implement OpenAI Provider |
| i[27] | TRC-007 | Implement Google Gemini Provider |
| i[28] | TRC-008 | Implement xAI Grok Provider |
| i[29] | TRC-009 | Implement Groq Provider |
| i[30] | TRC-010 | Wire Mouse Click on Tabs |
| i[31] | TRC-011 | Implement Secure Key Storage (keyring + encrypted fallback) |
| i[32] | TRC-012 | Implement Session Persistence (save/restore tabs) |
| i[33] | TRC-013 | Add Log Viewer with Auto-scroll Toggle |
| i[34] | TRC-014 | Implement Config Panel UI (Settings viewer) |
| i[35] | TRC-015 | Add Progress Spinners and Animations |
| i[36] | TRC-016 | Implement LLM Tool Use UI |
| i[37] | TRC-017 | Add Extended Thinking (thinking blocks) Display |
| i[38] | TRC-018 | Implement --dangerously-allow-all Equivalent Flag |
| i[39] | TRC-019 | Add GPU Usage Indicator |
| i[40] | TRC-020 | Implement Right-Click Context Menus |
| i[41] | TRC-021 | Add Search Within Log/Stream Viewers |
| i[42] | TRC-022 | Implement Log Filtering/Grep Functionality |
| i[43] | TRC-023 | Add Notification System for Background Events |
| i[44] | TRC-024 | Implement Split Pane Resizing |
| i[45] | TRC-025 | Add Graceful Degradation for Unavailable Streams |
| i[46] | TRC-026 | Add Unix Domain Socket Protocol Support |
| i[47] | TRC-027 | Add TCP Socket Protocol Support |
| i[48] | TRC-028 | Implement Dynamic Menu Generation from Config |
| i[49] | TRC-029 | Add Tab Inline Rename |
| i[50] | TRC-030 | Clean Up Dead Code Warnings |

---

## 2. Zero Tolerance Policy

### NO TECH DEBT

Previous instances accumulated tech debt. That era is over.

- **If you find a bug, fix it** - Do not document it for later
- **If something is broken, repair it** - Do not work around it
- **If code is incomplete, complete it** - Do not leave stubs
- **If you can't finish, say why** - Do not pretend completion

### NO PARTIAL IMPLEMENTATIONS

Your task is not done until:
- Code compiles with zero errors
- All tests pass (add tests if needed)
- The feature actually works end-to-end
- No `// TODO` comments left behind
- No `unimplemented!()` macros
- No dead code introduced

### NO MISLEADING

The Monitor reviews all work. Deception will be caught.

- Claim only what you actually delivered
- Surface problems immediately
- Acknowledge what you couldn't complete and why

---

## 3. Your Workflow

```
[EXPLORE] → [UNDERSTAND] → [IMPLEMENT] → [VERIFY] → [COMMIT] → [HANDOFF]
```

### Step 1: EXPLORE

```bash
# Switch to project
project_switch("ridge-control")

# Get your task details
task_list()  # Find your TRC-XXX task

# Read prior handoffs
context_get_recent(limit: 5)
context_search("your task topic")

# Read CONTRACT.md for requirements
Read CONTRACT.md
```

### Step 2: UNDERSTAND

Before writing any code:
- What does CONTRACT.md require for this feature?
- What code already exists that relates to this?
- What patterns are established in the codebase?
- What will this feature interact with?

Read the relevant source files. Understand before you act.

### Step 3: IMPLEMENT

Write production-quality Rust code:
- Follow existing patterns in the codebase
- Use the established module structure
- Handle all errors properly
- Add tests for new functionality
- No shortcuts, no "good enough"

### Step 4: VERIFY

Before claiming completion:

```bash
# Must pass
cargo build

# Must pass
cargo test

# Should be clean (fix warnings you introduce)
cargo clippy

# Actually run it and test the feature
cargo run
```

### Step 5: COMMIT

```bash
git add .
git commit -m "Instance #N: TRC-XXX - [Description]"
git push origin main
```

### Step 6: HANDOFF

Save to Mandrel:

```
context_store(
  content: "[Your handoff - see template below]",
  type: "handoff",
  tags: ["ridge-control", "i[N]", "TRC-XXX"]
)
```

Update task status:
```
task_update(taskId: "your-task-id", status: "completed")
```

---

## 4. Project Identity

| Field | Value |
|-------|-------|
| Project | ridge-control |
| Mandrel Project | ridge-control |
| Directory | ~/projects/ridge-control/ |
| Language | Rust (latest stable) |
| Framework | Ratatui |
| Platform | Linux only |

### Key Files

- `CONTRACT.md` - The specification (read this)
- `AGENTS.md` - This file (your instructions)
- `THE-MONITOR.md` - Who reviews your work
- `src/` - All source code
- `Cargo.toml` - Dependencies

### Current Architecture (as of i[20])

```
src/
├── main.rs           # Entry point
├── app.rs            # Main App struct, event loop, dispatch
├── action.rs         # Action enum for all application actions
├── error.rs          # Error types
├── event.rs          # Event types
├── components/       # UI components
│   ├── mod.rs
│   ├── command_palette.rs  # Fuzzy search command palette
│   ├── confirm_dialog.rs   # Tool confirmation dialog
│   ├── menu.rs             # Right panel menu
│   ├── placeholder.rs      # Placeholder widgets
│   ├── process_monitor.rs  # Process list with click-to-kill
│   ├── stream_viewer.rs    # Stream display widget (needs integration)
│   └── terminal.rs         # PTY terminal widget
├── config/           # Configuration system
│   ├── mod.rs              # ConfigManager
│   ├── keybindings.rs      # Helix-style keybindings
│   ├── theme.rs            # Theme definitions
│   └── watcher.rs          # Hot-reload watcher
├── input/            # Input handling
│   ├── mod.rs
│   ├── focus.rs            # Focus areas
│   └── mode.rs             # Input modes
├── llm/              # LLM integration
│   ├── mod.rs
│   ├── anthropic.rs        # Anthropic provider (implemented)
│   ├── manager.rs          # LLM manager
│   ├── provider.rs         # Provider trait
│   ├── tools.rs            # Tool definitions and executor
│   └── types.rs            # LLM types
├── pty/              # PTY terminal
│   ├── mod.rs              # PtyHandle
│   └── grid.rs             # Terminal grid with scrollback
├── streams/          # Stream clients
│   ├── mod.rs
│   ├── client.rs           # StreamClient
│   └── config.rs           # Stream configuration
└── tabs/             # Tab system
    ├── mod.rs              # TabManager
    └── tab_bar.rs          # TabBar widget
```

---

## 5. Mandrel Tools

| Tool | Purpose |
|------|---------|
| `project_switch("ridge-control")` | Set active project |
| `project_current()` | Verify current project |
| `task_list()` | See all TRC tasks |
| `task_details(taskId)` | Get task details |
| `task_update(taskId, status)` | Mark task complete |
| `context_store(content, type, tags)` | Save handoff |
| `context_search(query)` | Find prior work |
| `context_get_recent(limit)` | Get recent contexts |
| `decision_record(...)` | Record significant decisions |
| `smart_search(query)` | AI-powered search |

---

## 6. Handoff Template

```markdown
# Instance i[N] Handoff - TRC-XXX: [Task Title]

## Task Assignment
- Instance: i[N]
- Task: TRC-XXX
- Status: COMPLETE / INCOMPLETE

## What I Implemented
- [Specific feature/change 1]
- [Specific feature/change 2]

## Files Modified
- `src/path/file.rs` - [What changed]

## Files Created
- `src/path/new_file.rs` - [Purpose]

## How to Verify
1. [Step to test the feature]
2. [Expected result]

## Tests Added
- `test_name` - [What it tests]

## Problems Found and Fixed
- [Problem]: [How I fixed it]

## Build Status
- `cargo build`: PASS/FAIL
- `cargo test`: X/Y passing
- `cargo clippy`: [Warning count]

## Notes for Future Instances
- [Any relevant context]
```

---

## 7. Quality Standards

### Code Must

- Compile with zero errors
- Pass all existing tests
- Follow established patterns
- Handle errors explicitly (no `.unwrap()` in production paths)
- Be documented where non-obvious

### Code Must Not

- Introduce `// TODO` without completing it
- Use `unimplemented!()` or `todo!()`
- Add dependencies without justification
- Hard-code values that should be configurable
- Swallow errors silently
- Break existing functionality

---

## 8. If You Find Problems

Previous instances may have left issues. Here's what to do:

### Small Problem (< 30 min to fix)
Fix it. Include in your handoff that you fixed it.

### Medium Problem (Blocks your task)
Fix it first. Your task depends on working foundations.

### Large Problem (Unrelated to your task)
1. Document it clearly
2. Create a new task in Mandrel:
   ```
   task_create(title: "BUG: [Description]", ...)
   ```
3. Continue with your assigned task if possible

**Never ignore problems. Never work around broken code.**

---

## 9. The Monitor

Your work will be reviewed by The Monitor (see THE-MONITOR.md).

The Monitor checks:
- Did you complete your assigned task?
- Does the code actually work?
- Did you introduce new problems?
- Is your handoff accurate?
- Did you follow the zero-debt policy?

Work as if everything will be scrutinized - because it will be.

---

## 10. Git Workflow

```bash
# Before starting
cd ~/projects/ridge-control
git pull origin main

# After completing your task
cargo build && cargo test  # Must pass

git add .
git commit -m "Instance #N: TRC-XXX - [Brief description]"
git push origin main
```

### Commit Message Format

```
Instance #N: TRC-XXX - [Brief description]

- [Detail 1]
- [Detail 2]
```

**Example:**
```
Instance #21: TRC-001 - Apply theme to all component rendering

- Wire ThemeConfig to process_monitor render
- Wire ThemeConfig to terminal render
- Wire ThemeConfig to menu render
- Add theme parameter to all render methods
```

---

## 11. Remember

You are not exploring. You are not planning. You are **executing**.

- You have a specific task
- You have working code to build on
- You have clear requirements in CONTRACT.md
- You have prior context in Mandrel

**Execute your task completely. Leave no debt. Move the project forward.**

---

*Previous phase: AGENTS-SIRK-RC.md (archived)*
*Current phase: Task execution (i[21]+)*
