# Ridge-Control

A terminal-based command center built in Rust using Ratatui. Combines a full PTY terminal emulator with custom TUI widgets for process monitoring, log streaming, LLM interaction, and system orchestration.

## Status

**Planning Phase** - Instance i[0] complete. Architecture proposals documented in Mandrel.

### Instance Progress

| Instance | Phase | Focus | Status |
|----------|-------|-------|--------|
| i[0] | Planning | Research & Architecture Proposals | ✅ Complete |
| i[1] | Planning | Architecture Validation & Component Design | ⏳ Next |
| i[2-9] | Planning | TBD | 🔲 Pending |
| i[10]+ | Building | Implementation | 🔲 Pending |

### Architecture Decision

**Recommended: Component-Local State with Message Bus Bridge**

Three approaches evaluated:
1. **Unified Message Bus** - Simple, proven (GitUI-style)
2. **Actor Model** - Isolated, scalable (Zellij-inspired)  
3. **Component-Local State** - Ratatui-native, recommended ✓

See Mandrel contexts for detailed analysis.

## Features (Planned)

- Full PTY terminal emulator with shell integration
- Built-in Claude API client with tool use (Claude Code-like capabilities)
- Process monitor with CPU/GPU metrics
- Pluggable streaming data sources (WebSocket, REST, Unix/TCP sockets)
- Multi-tab interface with customizable split panes
- Rich visual design with digital braille aesthetic

## Requirements

- Linux (only supported platform)
- Rust (latest stable)
- Nerd Font recommended for icons/glyphs

## Project Structure

```
ridge-control/
├── CONTRACT.md      # Build specification
├── AGENTS.md        # Instance operating instructions
├── THE-MONITOR.md   # Quality oversight role
├── docs/            # Documentation
└── src/             # Source code (coming soon)
```

## The Experiment

This project is being built through a multi-instance AI experiment where sequential LLM instances collaboratively plan and implement the codebase. Each instance reads prior work via Mandrel (a context management system) and contributes incremental progress.

- **i[0] - i[9]**: Planning phase
- **i[10]+**: Building phase

## License

TBD

## Author

Brian @ RidgetopAI
