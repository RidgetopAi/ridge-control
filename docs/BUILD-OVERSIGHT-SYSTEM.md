# Ridge-Control Build Phase Oversight System

**Designed By**: Instance i[9]  
**Purpose**: Keep BUILD phase (i[10]-i[20]) on track, maintain quality, prevent drift  
**Status**: Ready for Implementation

---

## Table of Contents

1. [Overview](#1-overview)
2. [Self-Verification Protocol](#2-self-verification-protocol)
3. [Cross-Instance Consistency](#3-cross-instance-consistency)
4. [Specialized QA Agents](#4-specialized-qa-agents)
5. [Project Manager Agent](#5-project-manager-agent)
6. [Mandrel Usage Protocol](#6-mandrel-usage-protocol)
7. [Status Dashboard Concept](#7-status-dashboard-concept)
8. [Implementation Notes](#8-implementation-notes)

---

## 1. Overview

### The Problem

The BUILD phase spans 10+ stateless LLM instances. Without oversight:
- **Drift**: Gradual deviation from the master plan
- **Quality erosion**: Technical debt accumulates unchecked
- **Inconsistency**: Naming conventions, patterns diverge
- **Integration failures**: Modules don't fit together
- **Invisible progress**: The Monitor lacks visibility

### The Solution

A layered oversight system using Mandrel as the central nervous system:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          BUILD PHASE OVERSIGHT                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   LAYER 1: Self-Verification (Every Instance)                              │
│   ├── Pre-Build Checklist                                                   │
│   ├── During-Build Conventions                                              │
│   └── Post-Build Verification                                               │
│                                                                              │
│   LAYER 2: Specialized QA Agents (Triggered)                                │
│   ├── Pre-Build Agent: Project state check                                  │
│   ├── Post-Build Agent: Success criteria verification                       │
│   ├── Integration Agent: Module boundary checks                             │
│   └── Regression Agent: Test runner                                         │
│                                                                              │
│   LAYER 3: Project Manager Agent (Periodic)                                 │
│   ├── Status synthesis                                                      │
│   ├── Gap identification                                                    │
│   └── Monitor briefing                                                      │
│                                                                              │
│   FOUNDATION: Mandrel (Persistent Memory)                                   │
│   ├── Contexts: Handoffs, decisions, errors, completions                   │
│   ├── Tasks: Tracked deliverables with status                              │
│   └── Search: Semantic lookup across all stored data                       │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Self-Verification Protocol

### 2.1 Pre-Build Checklist (MANDATORY for every build instance)

Before writing ANY code, each instance MUST:

```markdown
## Pre-Build Verification Checklist

□ 1. PROJECT CONTEXT
   □ Ran `project_switch("ridge-control")`
   □ Read RIDGE-CONTROL-MASTER.md completely
   □ Searched recent handoffs: `context_search("handoff", tags: ["ridge-control"])`
   □ Understand what i[N-1] accomplished and recommended

□ 2. SUCCESS CRITERIA CLARITY
   □ Know MY specific deliverables from the roadmap table
   □ Know MY success criteria (what must work when I'm done)
   □ Identified any blockers from previous instances

□ 3. CONVENTION AWARENESS
   □ Reviewed existing code patterns (if src/ exists)
   □ Know the Component trait signature
   □ Know the Action/Event dispatch pattern
   □ Understand error handling (thiserror + anyhow + color-eyre)

□ 4. NO PRIOR PROBLEMS
   □ `cargo check` passes (or no code yet for i[10])
   □ No blocking issues in Mandrel flagged by previous instance
   □ If problems found, STOP and document before proceeding
```

### 2.2 During-Build Conventions

While coding, instances MUST follow:

**Naming Conventions**:
```rust
// Modules: snake_case
mod process_monitor;

// Types: PascalCase, descriptive
pub struct ProcessMonitor { ... }
pub enum FocusArea { Terminal, ProcessMonitor, ... }
pub enum Action { FocusNext, PtyInput(Vec<u8>), ... }

// Functions: snake_case, verb_noun
pub fn handle_input(&mut self, event: Event) -> Option<Action> { ... }
pub fn dispatch_action(&mut self, action: Action) { ... }

// Constants: SCREAMING_SNAKE_CASE
const MAX_SCROLLBACK: usize = 10_000;

// Files: Match module name exactly
// pty/mod.rs, pty/grid.rs, components/terminal.rs
```

**Pattern Adherence**:
```rust
// Component trait (ALL components must implement)
pub trait Component {
    fn handle_event(&mut self, event: &Event) -> Option<Action>;
    fn update(&mut self, action: &Action);
    fn render(&self, frame: &mut Frame, area: Rect);
}

// Event → Action flow (NEVER bypass)
// Event arrives → App::handle() → returns Action → App::dispatch() → mutates state

// Error handling (NEVER swallow errors)
fn load_config() -> Result<Config, RidgeError> { ... }  // Domain errors
fn main() -> anyhow::Result<()> { ... }                  // App layer
```

**Code Quality Gates**:
```bash
# MUST pass before claiming completion
cargo check      # Type check
cargo clippy     # Lints (no warnings allowed)
cargo test       # Unit tests

# If time permits
cargo fmt --check  # Formatting
```

### 2.3 Post-Build Verification Checklist (MANDATORY before handoff)

Before creating handoff, each instance MUST verify:

```markdown
## Post-Build Verification Checklist

□ 1. BUILD VERIFICATION
   □ `cargo check` passes with no errors
   □ `cargo clippy` has no warnings (or warnings documented)
   □ `cargo test` passes (or no tests yet)
   □ New code follows established patterns

□ 2. SUCCESS CRITERIA MET
   □ My specific deliverables from roadmap are DONE
   □ Success criteria can be demonstrated (documented how)
   □ If NOT all complete, clearly documented what's missing

□ 3. NO REGRESSION
   □ Existing functionality still works
   □ No breaking changes to public APIs without documentation
   □ Terminal still starts/stops cleanly

□ 4. DOCUMENTATION COMPLETE
   □ Module-level doc comments on new files
   □ Complex logic has inline comments
   □ Any deviations from master plan documented

□ 5. MANDREL SAVES DONE
   □ Handoff context stored with proper tags
   □ Any decisions recorded via decision_record
   □ Tech debt documented (if any)
   □ Tasks updated (if using task tracking)
```

### 2.4 Instance Verification Mandrel Query

Each instance should run these Mandrel queries as self-verification:

```javascript
// Check for conflicting decisions
decision_search({ query: "[your focus area]", projectId: "ridge-control" })
// Look for decisions that might conflict with what you're about to do

// Check for warnings from previous instances
context_search({ query: "warning blocker problem error", tags: ["ridge-control"] })
// Look for red flags that need addressing first

// Verify your focus area isn't duplicated
task_list({ projectId: "ridge-control", status: "in_progress" })
// Make sure no one else is working on the same thing
```

---

## 3. Cross-Instance Consistency

### 3.1 Naming Convention Registry

Store a naming registry in Mandrel to catch inconsistencies:

```javascript
// When introducing a new type/module/constant
context_store({
  content: `
NAMING REGISTRY ENTRY:
- Name: ProcessMonitor
- Type: struct
- Module: src/components/process_monitor.rs
- Purpose: Displays running processes with CPU/GPU metrics
- Related: ProcessInfo, ProcessUpdate, Action::ProcessKill
`,
  type: "code",
  tags: ["ridge-control", "naming", "component"]
})
```

**Verification Query** (each instance should run):
```javascript
// Check if name already exists
context_search({ 
  query: "NAMING REGISTRY [proposed name]", 
  tags: ["ridge-control", "naming"] 
})
```

### 3.2 Pattern Adherence Checks

Each instance should verify against these locked patterns:

**Pattern 1: Component Trait**
```rust
// CANONICAL: From RIDGE-CONTROL-MASTER.md
pub trait Component {
    fn handle_event(&mut self, event: &Event) -> Option<Action>;
    fn update(&mut self, action: &Action);
    fn render(&self, frame: &mut Frame, area: Rect);
}
```
- [ ] Every new component implements this trait
- [ ] No deviation without decision_record justification

**Pattern 2: Event-Action Flow**
```rust
// CANONICAL: Event arrives → handle → Action → dispatch
loop {
    let event = event_rx.recv()?;
    if let Some(action) = app.handle(&event) {
        app.dispatch(action);
    }
    app.render(&mut terminal)?;
}
```
- [ ] State mutations ONLY in dispatch()
- [ ] handle() is pure (no side effects)

**Pattern 3: Error Types**
```rust
// CANONICAL: Domain errors use thiserror
#[derive(thiserror::Error, Debug)]
pub enum RidgeError {
    #[error("...")]
    Variant { ... }
}

// CANONICAL: App layer uses anyhow
fn main() -> anyhow::Result<()> { ... }
```
- [ ] New error variants added to RidgeError
- [ ] Never use unwrap() in production code

### 3.3 Decision Conflict Detection

Before making ANY significant decision, query existing decisions:

```javascript
// Check for conflicts
decision_search({ 
  decisionType: "[architecture|library|pattern]",
  query: "[your topic]"
})

// If you MUST deviate from a prior decision, record superseding decision
decision_record({
  decisionType: "architecture",
  title: "Supersede: [previous decision]",
  description: "Why the prior decision is being changed",
  rationale: "New information that justifies the change",
  impactLevel: "high",
  tags: ["ridge-control", "supersede"]
})
```

---

## 4. Specialized QA Agents

### 4.1 Pre-Build Agent

**Trigger**: Before each build instance starts  
**Runner**: The Monitor OR automated script  
**Purpose**: Surface blockers before instance begins

**Checks**:
```bash
#!/bin/bash
# pre-build-check.sh

echo "=== Ridge-Control Pre-Build Check ==="

# 1. Git state
echo "Checking git status..."
cd ~/projects/ridge-control
git status --short
git log -1 --oneline

# 2. Build state
echo "Checking build..."
cargo check 2>&1 | tail -20

# 3. Mandrel state (requires MCP call)
echo "Checking Mandrel..."
# Query: recent errors, blockers, incomplete handoffs

# 4. Output summary for next instance
echo "=== Pre-Build Summary ==="
# - Last instance: i[N-1]
# - Build status: Pass/Fail
# - Blockers: None / [list]
# - Recommended focus: [from roadmap]
```

**Report Location**: Stored in Mandrel as context type "planning"

### 4.2 Post-Build Agent

**Trigger**: After each build instance commits  
**Runner**: The Monitor OR automated script  
**Purpose**: Verify success criteria met

**Checks**:
```bash
#!/bin/bash
# post-build-check.sh

echo "=== Ridge-Control Post-Build Check ==="

# 1. Build passes
cargo check
cargo clippy -- -D warnings
cargo test

# 2. Instance-specific criteria
case $INSTANCE in
  10) 
    # MVP: Layout + PTY + Focus + Shutdown
    cargo run &
    PID=$!
    sleep 2
    # Check: Window visible
    # Check: Tab switches focus (manual or screenshot)
    # Check: Ctrl+C exits
    kill $PID 2>/dev/null
    ;;
  11)
    # PTY Polish: Scrollback, mouse selection, colors
    # Check: ANSI sequences render
    # Check: Scrollback works
    ;;
  # ... more cases
esac

# 3. No regressions (previous criteria still pass)
# Re-run earlier instance checks

# 4. Store result in Mandrel
```

**Report Location**: Stored in Mandrel as context type "completion" or "error"

### 4.3 Integration Agent

**Trigger**: Every 3 instances (i[12], i[15], i[18])  
**Runner**: The Monitor  
**Purpose**: Verify modules integrate correctly

**Checks**:
```markdown
## Integration Checklist

□ Module Boundaries
  □ No circular dependencies between modules
  □ Public APIs are minimal and well-defined
  □ Internal types stay internal

□ Type Consistency
  □ Action enum has all new variants from recent instances
  □ Event enum has all new variants
  □ No duplicate enum variants with different semantics

□ Async Consistency
  □ All async code runs in tokio thread
  □ crossbeam channels bridge to main thread correctly
  □ No blocking in async context

□ Config Consistency
  □ New config fields added to schema
  □ Default values provided
  □ Config loading handles new fields
```

### 4.4 Regression Agent

**Trigger**: Every instance, automated  
**Runner**: GitHub Actions or local script  
**Purpose**: Catch regressions early

**Implementation**:
```yaml
# .github/workflows/regression.yml
name: Regression Check

on: [push]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      
      - name: Check
        run: cargo check
      
      - name: Clippy
        run: cargo clippy -- -D warnings
      
      - name: Test
        run: cargo test
      
      - name: Functional Test
        run: |
          timeout 5 cargo run || true
          # Basic smoke test - does it start?
```

---

## 5. Project Manager Agent

### 5.1 Purpose

A periodic meta-agent that synthesizes project state and briefs The Monitor. NOT a replacement for The Monitor - an assistant.

### 5.2 When It Runs

- After every 2-3 build instances
- On-demand when The Monitor requests
- Before major milestones (end of Phase 1, 2, 3, 4)

### 5.3 What It Checks

```markdown
## Project Manager Synthesis

### 1. Progress Against Roadmap
For each instance i[10]-i[N]:
- [ ] Instance completed? (handoff exists)
- [ ] Success criteria met? (from roadmap table)
- [ ] On schedule? (no unexpected blockers)

### 2. Quality Metrics
- [ ] Build passing?
- [ ] Test coverage trend (if measurable)
- [ ] Clippy warnings count
- [ ] Tech debt items outstanding

### 3. Consistency Analysis
- [ ] Naming convention violations?
- [ ] Pattern deviations?
- [ ] Conflicting decisions?

### 4. Risk Identification
- [ ] Incomplete handoffs?
- [ ] Repeated errors across instances?
- [ ] Drift from master plan?
- [ ] Integration concerns?

### 5. Recommendations
- Next instance focus
- Issues for Monitor attention
- Suggested course corrections
```

### 5.4 Output Format

The Project Manager Agent produces a brief for The Monitor:

```markdown
# Ridge-Control Status Brief
Generated: [timestamp]
Period: i[10] through i[N]

## Executive Summary
[2-3 sentences on overall health]

## Progress
| Instance | Status | Deliverable | Issues |
|----------|--------|-------------|--------|
| i[10]    | ✅     | MVP layout  | None   |
| i[11]    | ✅     | PTY polish  | Minor  |
| i[12]    | 🔄     | In progress | -      |

## Health Indicators
- Build: ✅ Passing
- Tests: ⚠️ 2 failing
- Clippy: ✅ Clean
- Debt: 2 items

## Issues Requiring Attention
1. [Issue description, severity, recommended action]

## Recommendations for Next Instance
1. [Specific guidance]
```

### 5.5 Mandrel Queries for PM Agent

```javascript
// Get all build handoffs
context_search({ 
  query: "handoff build", 
  tags: ["ridge-control"],
  type: "handoff"
})

// Get errors and blockers
context_search({ 
  query: "error blocker problem", 
  tags: ["ridge-control"] 
})

// Get tech debt
context_search({ 
  query: "tech debt", 
  tags: ["ridge-control", "tech-debt"] 
})

// Get task status
task_list({ projectId: "ridge-control" })

// Get decision timeline
decision_search({ projectId: "ridge-control" })
```

---

## 6. Mandrel Usage Protocol

### 6.1 Required Context Types for BUILD Phase

| Type | When to Use | Required Tags |
|------|-------------|---------------|
| `handoff` | End of every instance | `["ridge-control", "i[N]", "handoff"]` |
| `code` | New modules, significant code | `["ridge-control", "code", "[module]"]` |
| `decision` | Any deviation from master plan | `["ridge-control", "decision", "[area]"]` |
| `error` | Problems discovered | `["ridge-control", "error", "i[N]"]` |
| `completion` | Deliverable finished | `["ridge-control", "completion", "[feature]"]` |
| `milestone` | Major checkpoint | `["ridge-control", "milestone"]` |

### 6.2 Required Tags

**Every context MUST include**:
- `ridge-control` (project identifier)
- `i[N]` (instance number)

**Recommended additional tags**:
- Module name: `pty`, `components`, `llm`, `streams`, `config`
- Feature name: `terminal`, `process-monitor`, `menu`
- Status: `blocker`, `tech-debt`, `resolved`

### 6.3 Handoff Template (BUILD Phase)

```markdown
# Instance i[N] Build Handoff

## Instance Identity
- Instance Number: i[N]
- Phase: Build
- Focus Area: [From roadmap]

## Deliverables
| Deliverable | Status | Notes |
|-------------|--------|-------|
| [Item 1]    | ✅/⚠️/❌ | [Details] |
| [Item 2]    | ✅/⚠️/❌ | [Details] |

## Success Criteria Verification
```bash
# How to verify my work
cargo run
# Expected: [description]
# Actual: [result]
```

## Code Changes
| File | Change Type | Description |
|------|-------------|-------------|
| src/pty/mod.rs | New | PTY spawn and read |
| src/app.rs | Modified | Added PTY integration |

## Patterns Used
- [ ] Component trait implemented correctly
- [ ] Event → Action flow followed
- [ ] Error handling with RidgeError

## Known Issues
- [Issue]: [Description], [Severity: Low/Medium/High]

## Tech Debt Introduced
- [Debt]: [Description], [Remediation: path to fix]

## Blockers for Next Instance
- [None / List blockers]

## Recommendations for i[N+1]
1. [Specific next step]
2. [Watch out for X]

## Verification Commands
```bash
cargo check    # [Pass/Fail]
cargo clippy   # [Warnings: N]
cargo test     # [Pass: N, Fail: M]
```
```

### 6.4 Task Tracking Protocol

For granular tracking, use Mandrel tasks:

```javascript
// At instance START, create tasks for deliverables
task_create({
  title: "Implement PTY spawn",
  description: "Create pty-process based PTY spawning",
  type: "feature",
  priority: "high",
  tags: ["ridge-control", "i10", "pty"]
})

// During work, update status
task_update({
  taskId: "[id]",
  status: "in_progress"
})

// At completion
task_update({
  taskId: "[id]",
  status: "completed"
})
```

### 6.5 Decision Recording Protocol

When deviating from or extending the master plan:

```javascript
decision_record({
  decisionType: "pattern",  // architecture, library, pattern, api_design
  title: "PTY read buffer size",
  description: "Using 4KB buffer instead of 8KB default",
  rationale: "Lower latency for interactive shells, benchmarked",
  impactLevel: "low",  // low, medium, high, critical
  alternativesConsidered: [
    { name: "8KB", pros: ["Higher throughput"], cons: ["Higher latency"], reasonRejected: "Interactive feel matters more" }
  ],
  affectedComponents: ["pty/mod.rs"],
  tags: ["ridge-control", "pty", "performance"]
})
```

---

## 7. Status Dashboard Concept

### 7.1 Purpose

A quick-glance view of project health for The Monitor.

### 7.2 Data Sources

All data comes from Mandrel queries:

```javascript
// Build status (from last handoff)
context_get_recent({ limit: 1, type: "handoff" })

// Progress (count completions)
context_search({ query: "completion", tags: ["ridge-control"], type: "completion" })

// Issues (count errors not resolved)
context_search({ query: "error blocker", tags: ["ridge-control", "error"] })

// Tasks
task_progress_summary({ projectId: "ridge-control", groupBy: "status" })
```

### 7.3 Dashboard Layout

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     RIDGE-CONTROL BUILD DASHBOARD                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  CURRENT STATE                          HEALTH INDICATORS                   │
│  ├─ Instance: i[12]                     ├─ Build: ✅ Pass                   │
│  ├─ Phase: 1 (Foundation)               ├─ Tests: ⚠️ 2/15 fail             │
│  ├─ Focus: Process Monitor              ├─ Clippy: ✅ Clean                 │
│  └─ Last Handoff: 2h ago                └─ Debt: 3 items                    │
│                                                                              │
│  ROADMAP PROGRESS                                                           │
│  Phase 1: Foundation     [████████░░] 80% (i10 ✅, i11 ✅, i12 🔄)         │
│  Phase 2: Networking     [░░░░░░░░░░]  0% (i13-i15)                         │
│  Phase 3: UX             [░░░░░░░░░░]  0% (i16-i18)                         │
│  Phase 4: Polish         [░░░░░░░░░░]  0% (i19-i20)                         │
│                                                                              │
│  RECENT ISSUES                          UPCOMING                            │
│  ├─ [⚠️] VTE parser edge case           ├─ i[13]: Menu + Streams           │
│  └─ [ℹ️] Scrollback memory tuning       ├─ i[14]: LLM Integration          │
│                                         └─ i[15]: Tool Execution            │
│                                                                              │
│  TECH DEBT                                                                  │
│  1. Hardcoded terminal size (i10)                                           │
│  2. No config hot-reload yet                                                │
│  3. Placeholder process data                                                │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.4 Implementation

Could be implemented as:
1. **Markdown file** generated by PM Agent → committed to docs/
2. **Mandrel context** with special type "dashboard"
3. **Script output** that queries Mandrel and formats

Recommended: **Option 1** - Markdown file at `docs/BUILD-STATUS.md`, updated by PM Agent after each instance.

---

## 8. Implementation Notes

### 8.1 What Happens When

```
Instance i[N] Lifecycle:
─────────────────────────────────────────────────────────────────────────────

1. PRE-BUILD (5-10 minutes)
   └─ Instance reads RIDGE-CONTROL-MASTER.md
   └─ Instance runs Pre-Build Checklist
   └─ Instance queries Mandrel for context
   └─ Instance confirms no blockers

2. BUILD (bulk of time)
   └─ Instance codes against roadmap deliverables
   └─ Instance follows naming/pattern conventions
   └─ Instance stores interim contexts if helpful
   └─ Instance runs cargo check/clippy/test

3. POST-BUILD (10-15 minutes)
   └─ Instance runs Post-Build Verification Checklist
   └─ Instance creates Mandrel handoff
   └─ Instance commits to git with clear message
   └─ Instance pushes to remote

4. BETWEEN INSTANCES (async)
   └─ Post-Build Agent runs (if configured)
   └─ Regression Agent runs (CI)
   └─ PM Agent runs (every 2-3 instances)
```

### 8.2 Failure Modes and Mitigations

| Failure | Detection | Mitigation |
|---------|-----------|------------|
| Instance skips verification | Missing handoff fields | PM Agent flags incomplete handoffs |
| Naming inconsistency | Search shows duplicates | Pre-build query for existing names |
| Pattern deviation | Code review by Monitor | Post-build agent checks signatures |
| Accumulating tech debt | Debt count in dashboard | PM Agent surfaces when > 5 items |
| Instance works on wrong thing | Handoff doesn't match roadmap | Pre-build confirms focus area |
| Build broken for next instance | cargo check fails | Pre-build agent catches before start |

### 8.3 The Monitor's Workflow

With this system, The Monitor can:

1. **Review dashboard** before spawning next instance
2. **Read PM Agent brief** for synthesized status
3. **Scan handoffs** only when issues flagged
4. **Intervene** with specific guidance when needed
5. **Trust the process** for routine instances

### 8.4 Overhead Assessment

| Activity | Time per Instance | Impact |
|----------|------------------|--------|
| Pre-Build Checklist | 5-10 min | Low overhead, high value |
| Post-Build Verification | 10-15 min | Catches issues early |
| Mandrel saves | 5 min | Essential for continuity |
| Naming registry | 2 min | Prevents future conflicts |
| PM Agent (every 3 instances) | N/A (automated) | No instance overhead |

**Total overhead**: ~20-30 minutes per instance, which is acceptable for quality assurance.

---

## Appendix A: Quick Reference Card

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    BUILD INSTANCE QUICK REFERENCE                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  START                                   END                                 │
│  ├─ project_switch("ridge-control")     ├─ cargo check ✓                   │
│  ├─ Read RIDGE-CONTROL-MASTER.md        ├─ cargo clippy ✓                  │
│  ├─ context_search("handoff i[N-1]")    ├─ cargo test ✓                    │
│  └─ Confirm no blockers                 └─ context_store(handoff)          │
│                                                                              │
│  MANDREL SAVES                                                              │
│  ├─ Handoff (required): type="handoff", tags=["ridge-control","i[N]"]      │
│  ├─ Decisions (if any): decision_record(...)                               │
│  ├─ Errors (if any): type="error", tags=["ridge-control","error"]          │
│  └─ Completions: type="completion", tags=["ridge-control","completion"]    │
│                                                                              │
│  PATTERNS TO FOLLOW                                                         │
│  ├─ Component trait: handle_event → update → render                        │
│  ├─ Event flow: Event → handle() → Action → dispatch()                     │
│  ├─ Errors: RidgeError (domain), anyhow (app)                              │
│  └─ Naming: PascalCase types, snake_case functions                         │
│                                                                              │
│  GIT                                                                        │
│  └─ git commit -m "Instance #N: [clear description]"                        │
│  └─ git push origin main                                                   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

*Document created by Instance i[9] • Build Phase Oversight System Ready*
