# Streams Module - Infrastructure Complete

**Status**: ✅ Fully wired to App, ready for use

This module provides external stream connectivity for monitoring real-time data feeds.
The infrastructure is complete and connected to the UI - just needs practical endpoints.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ App                                                         │
│  ├── stream_manager: StreamManager     ← Initialized        │
│  ├── stream_rx: mpsc::Receiver         ← Event polling      │
│  └── handle_stream_event()             ← Event dispatch     │
├─────────────────────────────────────────────────────────────┤
│ UI Integration                                              │
│  ├── Menu shows available streams                           │
│  ├── StreamViewer displays connected stream data            │
│  └── Actions: Connect/Disconnect/Toggle/Retry/Refresh       │
├─────────────────────────────────────────────────────────────┤
│ Supported Protocols (TRC-026, TRC-027)                      │
│  ├── WebSocket (wss://, ws://)                              │
│  ├── SSE (Server-Sent Events)                               │
│  ├── REST (polling)                                         │
│  ├── Unix Domain Socket (/var/run/*.sock)                   │
│  └── TCP Socket (host:port)                                 │
└─────────────────────────────────────────────────────────────┘
```

## What's Implemented

| Component | Status | Location |
|-----------|--------|----------|
| `StreamManager` | ✅ Complete | `client.rs` |
| `StreamClient` | ✅ Complete | `client.rs` |
| `StreamsConfig` | ✅ Complete | `config.rs` |
| `ConnectionHealth` | ✅ Complete | `client.rs` |
| App integration | ✅ Wired | `app.rs` lines 70, 186-190, 590-601 |
| Event handling | ✅ Wired | `app.rs` `handle_stream_event()` |
| Auto-reconnect | ✅ Complete | `client.rs` with exponential backoff |
| Hot-reload config | ✅ Complete | TRC-028 |

## Configuration

Create `~/.config/ridge-control/streams.toml`:

```toml
[[streams]]
id = "local-logs"
name = "Application Logs"
protocol = "unix"
url = "/var/run/myapp/log.sock"
auto_connect = true
reconnect = true
reconnect_delay_ms = 5000

[[streams]]
id = "metrics"
name = "Metrics Feed"
protocol = "websocket"
url = "wss://metrics.internal.example.com/ws"
auto_connect = false
reconnect = true
reconnect_delay_ms = 10000

[[streams]]
id = "alerts"
name = "Alert System"
protocol = "tcp"
url = "10.0.0.50:9090"
auto_connect = true
reconnect = true
reconnect_delay_ms = 3000
```

## Default Configuration

Without `streams.toml`, defaults to a demo echo server:
```toml
[[streams]]
id = "example-ws"
name = "Example WebSocket"
protocol = "websocket"
url = "wss://echo.websocket.org"
auto_connect = false
```

## Usage

1. **Menu Navigation**: Focus menu panel, see available streams
2. **Connect**: Select stream, press Enter or click to toggle connection
3. **View Data**: Connected stream data appears in StreamViewer
4. **Disconnect**: Toggle again or use Action::StreamDisconnect
5. **Refresh Config**: `:stream_refresh` or press `r` in menu (hot-reloads `streams.toml`)

## Example Use Cases

- **Log Aggregation**: Connect to centralized logging via Unix socket
- **Metrics Monitoring**: WebSocket feed from Prometheus/Grafana
- **Alert Systems**: TCP connection to alerting infrastructure
- **Dev Server Logs**: SSE from local development servers

## Tests

```bash
cargo test streams::  # Run stream module tests
```

Test coverage includes:
- Connection health tracking
- Reconnection logic with backoff
- Multi-protocol stream loading
- Client state management

## Related Tasks

- TRC-026: Unix Domain Socket Protocol ✅
- TRC-027: TCP Socket Protocol ✅
- TRC-028: Dynamic Menu from Config ✅
- TRC-025: Graceful Degradation ✅
