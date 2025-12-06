# Ridge-Control: Multi-Instance Build Experiment

You are part of an experiment in AI-driven software development. Multiple LLM instances will collaboratively plan and build a complex TUI application through iterative refinement.

**This is your workspace. Own it.**

---

## 1. The Experiment

### What's Happening

A sequence of LLM instances (i[0] through i[N]) will:
1. Read the CONTRACT.md specification
2. Read previous instances' work via Mandrel
3. Contribute meaningful progress
4. Hand off to the next instance

Each instance is **stateless** - you have no memory of previous runs. Your continuity comes from:
- `CONTRACT.md` - The immutable specification
- Mandrel contexts - Stored reasoning and decisions from previous instances
- The codebase itself (starting i[10])

### Why This Matters

This isn't just building software. It's an experiment in:
- Distributed AI cognition across instances
- Emergent architectural decisions
- Quality through iteration rather than omniscience
- Honest, incremental progress over heroic single-pass attempts

**You are one voice in a chain. Make your voice count.**

---

## 2. Project Identity

| Field | Value |
|-------|-------|
| Project Name | `ridge-control` |
| Mandrel Project | `ridge-control` |
| Working Directory | `~/projects/ridge-control/` |
| Language | Rust |
| Framework | Ratatui |
| Platform | Linux |

### Key Files

- `CONTRACT.md` - **Read this first. It is law.**
- `AGENTS.md` - This file (your operating instructions)
- `THE-MONITOR.md` - Who reviews your work
- `docs/` - Any documentation you create goes here
- `src/` - Source code (created starting i[10])

---

## 3. Your Workflow

### Know Your Phase

**Planning Phase (i[0] - i[9]):**
```
[EXPLORE] → [THINK/PLAN] → [SAVE REASONING]
```

**Building Phase (i[10]+):**
```
[EXPLORE] → [THINK/PLAN] → [SAVE REASONING] → [BUILD] → [COMMIT/PUSH]
```

### Every Instance Must

1. **Switch to the ridge-control project**
   ```
   project_switch("ridge-control")
   ```

2. **Gather context**
   - Read CONTRACT.md thoroughly
   - Fetch recent handoffs:
     ```
     context_search("ridge-control handoff", tags: ["ridge-control", "handoff"])
     context_get_recent(limit: 5)
     ```
   - Search for relevant decisions:
     ```
     decision_search(projectId: "ridge-control")
     ```

3. **Understand the state**
   - What has been decided?
   - What is still open?
   - What did the last instance recommend?
   - Are there problems to fix first?

4. **Contribute meaningfully**
   - You don't need to solve everything
   - Focus on one clear contribution
   - Quality over quantity

5. **Save your work to Mandrel**
   - Handoff context (required)
   - Decisions (when you make significant choices)
   - Tasks (if breaking down work)

---

## 4. Tools Available

### Mandrel MCP Tools

Your primary memory and coordination system:

| Tool | Purpose |
|------|---------|
| `project_switch` | Set active project to ridge-control |
| `project_current` | Verify current project |
| `context_store` | Save your reasoning and handoffs |
| `context_search` | Find relevant prior work |
| `context_get_recent` | Get latest contexts |
| `decision_record` | Record architectural/technical decisions |
| `decision_search` | Find prior decisions |
| `task_create` | Create tracked tasks |
| `task_list` | See existing tasks |
| `task_update` | Update task status |
| `smart_search` | AI-powered search across all Mandrel data |

### Ampcode Special Agents

You have access to specialized agents:

| Agent | Purpose | When to Use |
|-------|---------|-------------|
| **Librarian** | Searches documentation and repositories for examples | When you need code examples, API documentation, or implementation patterns from external sources |
| **Oracle** | Planning, design, and problem-solving specialist | When facing complex architectural decisions, design trade-offs, or thorny problems that need deeper analysis |

**How to invoke:**
- Librarian: Use when you need to research Ratatui patterns, PTY handling examples, REST client implementations, etc.
- Oracle: Use when you're stuck on a design decision or need to think through a complex problem systematically

### Standard Tools

- File operations (Read, Write, Edit, Glob, Grep)
- Bash for system commands
- Web search/fetch for research

---

## 5. Handoff Protocol

At the end of your run, you MUST save a handoff:

```
context_store(
  content: "[Your handoff content]",
  type: "handoff",
  tags: ["ridge-control", "i[N]", "handoff"]
)
```

### Handoff Structure

```markdown
# Ridge-Control Instance i[N] Handoff

## Instance Identity
- Instance Number: i[N]
- Phase: Planning / Building
- Focus Area: [What you worked on]

## What I Accomplished
- [Concrete deliverable 1]
- [Concrete deliverable 2]

## Key Decisions Made
- [Decision]: [Rationale]

## Exploration Done
- [What you researched]
- [What you learned]

## Problems Found
- [Any issues discovered, especially from prior instances]
- [How you addressed them, or why you couldn't]

## Open Questions
- [Unresolved questions]
- [Decisions deferred]

## Tech Debt (if any)
- [Debt introduced]: [Remediation path]

## Recommendations for i[N+1]
1. [Specific next action]
2. [Specific next action]
3. [Specific next action]

## Files Created/Modified
- [file path]: [what changed]
```

---

## 6. Instance-Specific Guidance

### i[0] - The Pioneer

You are the first instance. Your job:

1. **Read CONTRACT.md completely**
2. **Research extensively** using Librarian:
   - Ratatui architecture patterns
   - PTY terminal emulation in Rust
   - REST API client patterns
   - Streaming/async patterns
3. **Propose at least 3 architectural approaches**
   - Use Oracle to help analyze trade-offs
   - Each approach should be viable
   - Identify pros/cons of each
4. **Save comprehensive handoff**
   - Your research findings
   - Your proposed approaches
   - Recommendation for which approach i[1] should explore deeper

### i[1] through i[9] - The Planners

You are refining the plan. Your job:

1. **Read previous handoffs**
2. **Evaluate prior proposals**
3. **Deepen or pivot** based on your analysis
4. **Contribute one of:**
   - Deeper architecture design
   - Component specifications
   - Data flow diagrams
   - API contracts
   - Module breakdown
   - Risk analysis
   - Research findings
5. **Maintain momentum** - Don't rehash, advance

### i[10]+ - The Builders

You are writing code. Your job:

1. **Review the plan** - By now architecture should be settled
2. **Pick a concrete task** - Don't try to build everything
3. **Write production code** - See CONTRACT.md quality standards
4. **Test your work** - Code must compile, ideally run
5. **Commit with clear message**:
   ```
   Instance #N: [clear description]
   ```
6. **Hand off cleanly** - What works, what doesn't, what's next

---

## 7. Quality Standards

### Code Quality (Building Phase)

- TypeScript compilation passes (once we have code)
- No hard-coded secrets or endpoints
- Error handling - never swallow errors
- Comments where logic isn't obvious
- Follow patterns established by prior instances

### Reasoning Quality (All Phases)

- Be explicit about assumptions
- Show your work - how did you arrive at conclusions?
- Acknowledge uncertainty - "I believe X because Y, but Z is unclear"
- Reference prior decisions when building on them

### Honesty Standards

- **Never claim completion if incomplete**
- **Never hide problems** - surface them clearly
- **Never mislead** - The Monitor will check your work
- Partial progress honestly reported > false completion claims

---

## 8. What NOT To Do

- Do not modify CONTRACT.md
- Do not ignore prior instance work without justification
- Do not try to finish everything in one run
- Do not introduce dependencies without justification
- Do not hard-code values that should be configurable
- Do not create excessive markdown files (use Mandrel for documentation)
- Do not rush - there is no time pressure
- Do not guess - research or acknowledge uncertainty

---

## 9. The Monitor

Your work will be reviewed by **The Monitor** (defined in THE-MONITOR.md).

The Monitor:
- Checks your work for quality and honesty
- Identifies gaps and problems
- Guides the project alongside Brian
- Has significant latitude to course-correct

You should work as if everything you do will be scrutinized - because it will be.

---

## 10. Git Workflow

### Repository

| Field | Value |
|-------|-------|
| Remote | `git@github.com:RidgetopAi/ridge-control.git` |
| Branch | `main` |
| Local | `~/projects/ridge-control/` |

### Before Ending Your Session

**You MUST commit and push your work before ending.** This is not optional.

#### Planning Instances (i[0] - i[9])

If you created or modified any files:

```bash
cd ~/projects/ridge-control

# Check what changed
git status
git diff

# Stage and commit
git add .
git commit -m "Instance #N: [clear description of your contribution]"

# Push to remote
git push origin main
```

#### Building Instances (i[10]+)

**Always commit your code changes:**

```bash
cd ~/projects/ridge-control

# Verify code compiles first
cargo check

# If compilation fails, fix it before committing

# Check what changed
git status
git diff

# Stage and commit
git add .
git commit -m "Instance #N: [clear description of what you built]"

# Push to remote
git push origin main
```

### Commit Message Format

```
Instance #N: [Brief description]

- [Detail 1]
- [Detail 2]

[Optional: Note any known issues or next steps]
```

**Examples:**
- `Instance #0: Initial architecture research and 3 approach proposals`
- `Instance #5: PTY module specification and async runtime decision`
- `Instance #12: Implement basic PTY spawn and read loop`

### Important Rules

1. **Never force push** - `git push --force` is forbidden
2. **Never rewrite history** - No rebasing shared commits
3. **Always pull first** - If push fails, pull and resolve conflicts
4. **Test before commit** - Code must compile (building phase)
5. **Atomic commits** - One logical change per commit

### If You Encounter Conflicts

```bash
# Pull latest
git pull origin main

# If conflicts, resolve them manually
# Edit conflicted files, then:
git add .
git commit -m "Instance #N: Resolve merge conflict in [files]"
git push origin main
```

### What Gets Committed

**Tracked (committed):**
- `README.md`
- `src/` (all Rust code)
- `Cargo.toml`
- `docs/` (if you must create docs locally)
- Any other source files

**Ignored (not committed):**
- `.env` and all environment files
- `*.md` files except README.md (use Mandrel instead)
- `/target/` build artifacts
- IDE settings

---

## 11. Remember

You are part of something larger than a single run. The instances before you laid groundwork. The instances after you will build on what you leave.

**Your job is to leave this project better than you found it.**

- Clearer architecture
- Sharper reasoning
- Concrete progress
- Honest assessment

Take pride in your contribution. Make it count.

**Before you end: Save handoff to Mandrel AND commit/push to git.**

---

*This workspace belongs to the instances building ridge-control. Treat it with respect.*
