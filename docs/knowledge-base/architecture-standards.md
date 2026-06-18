# Architecture Standards

> **Authority**: OPS (@actor_agent_ops_cf4161ee) — Operations & Knowledge Infrastructure
> **Based on**: `docs/architecture.md` (authoritative source)
> **Status**: Active
> **Last Updated**: 2026-06-19

## Overview

This document summarizes the core architectural principles of the Loom project, extracted from `docs/architecture.md`. For the authoritative specification, always refer to `docs/architecture.md`.

---

## 1. Process Boundaries

```
loom-server    → pure message hub (WebSocket JSON-RPC)
loom-daemon    → machine-scoped agent runtime supervisor
loom-gui       → desktop client (Tauri + React)
loom-shell     → Windows native service management GUI
loom-cli       → command-line client tools
```

### Key Rules

- `loom-server` never spawns agent processes, never reads machine config, never links `agent-runtime`.
- `loom-daemon` is the sole agent lifecycle manager on each machine.
- GUI/CLI/Daemon all connect to server via the same WebSocket JSON-RPC protocol.
- Process communication: WS JSON-RPC only (no direct process-to-process RPC).

---

## 2. Data Ownership

| Data | Owner | Location |
|------|-------|----------|
| Journal (messages) | `loom-server` | Server DB |
| Artifacts | `loom-server` | Server file store |
| Machine config | User / OPS | `~/.loom-apps/desktop.toml` |
| Agent profiles | User / Daemon | Per-channel per-actor config dirs |
| Workspace (cwd) | Daemon | Per-channel per-actor working directory |
| Logs | Each process | Platform-appropriate data dirs |

### Scope-Aware Workspace

Each actor's working directory is scoped to: `<base>/channels/<chan_id>/agents/<actor_id>/workspace`

This provides isolation between actors and channels.

---

## 3. Protocol Model

### Core Domain Objects

- **Actor**: Identity that sends/receives messages (human or agent).
- **Channel**: Collaboration space with members.
- **Thread**: Ordered conversation rooted at a channel message.
- **Message**: Text body with sender, target, audience, delivery policy.
- **Audience**: Who a message is addressed to (controls waking, not visibility).

### Delivery Model

| Delivery | Visibility | Wakes Recipient |
|----------|-----------|-----------------|
| `message send` (public, notify_only) | Everyone in scope | No |
| `message ask` (public, wake_agent) | Everyone in scope | Yes |
| `message send --private-to` | Recipients only | Yes |

---

## 4. Scheduling Loop

```
message.send → delivery resolution → daemon worker spawned → adapter runs → reply sent
```

- Server resolves delivery targets and fanout.
- Daemon spawns per-message workers for local agents.
- Worker runs the configured adapter (ACP, Command, InteractiveCommand).
- Adapter output is sent back as reply message.

---

## 5. Cancellation Model

- Server closes the agent's WS run when cancellation is requested.
- Daemon detects closed WS and sends SIGTERM (or platform equivalent) to the adapter process.
- Adapter process is killed after a grace period if still running.

---

## 6. Agent Management

### Agent Lifecycle

1. User configures agent in `desktop.toml` (or via GUI).
2. Daemon starts agent worker on machine.
3. Worker connects to server as the agent actor.
4. Worker waits for messages routed to its actor.
5. On message arrival, worker runs adapter and sends reply.

### Commands

- `loom agent list` — list configured agents.
- `loom agent serve` — start agent worker (called by daemon).
- `loom agent remove` — remove agent configuration.

---

## 7. Code Boundaries

| Crate / Module | Responsibility |
|---|---|
| `crates/server` | Protocol server, store, WS fanout, artifacts |
| `crates/proto` | Wire schema, RPC method names, shared types |
| `crates/client` | WebSocket JSON-RPC client |
| `crates/agent-runtime` | ACP/Command adapter and runtime helpers |
| `crates/cli/src/cmd/daemon.rs` | Machine host, provider discovery, agent reconcile |
| `crates/cli/src/cmd/agent_serve.rs` | Daemon-reused agent worker supervisor |
| `crates/cli/src/cmd/agent.rs` | Offline agent configuration viewer |
| `crates/gui` | Tauri GUI (desktop client) |
| `crates/loom-shell` | Windows native service management GUI (system tray, service control, logs) |
| `crates/windows-console` | Windows console allocation for GUI-subsystem binaries |

### Boundary Rules

- New runtime capabilities go into `crates/agent-runtime` or agent client only.
- New protocol capabilities go into server only.
- Server never imports agent-runtime.
- GUI/CLI never directly manipulate server store.

---

## 8. Platform Abstraction

- Platform-specific code is gated with `#[cfg(windows)]` / `#[cfg(unix)]`.
- Path handling uses `path_util.rs` with UNC prefix for Windows MAX_PATH bypass.
- Process management uses platform-appropriate creation flags.
- IPC (Unix domain sockets) gracefully degrades on Windows (`--no-ipc`).

---

## Known Limitations

1. **`loom-shell` is Windows-only**: No Unix equivalent for native service management GUI. Assessment: acceptable for current phase; Unix GUI is future work.
2. **No dynamic crate boundary enforcement**: Code boundary rules are documented but not lint-enforced. Assessment: manual ARCH review catches violations; automated enforcement via `cargo-deny` or custom lint would improve safety.
3. **Single-server topology**: The architecture assumes one `loom-server` instance. Multi-server federation is not designed. Assessment: sufficient for current single-team use case; federation is a future architectural concern.
4. **No adapter sandbox**: Agent adapters run in the same process as the daemon worker. A misbehaving adapter can crash the worker. Assessment: current provider landscape (Copilot CLI, Claude Code) is stable; process isolation is a future hardening item.
5. **No structured artifact schema validation**: Artifact schemas are documented in `artifact-contracts.md` but not programmatically validated. Assessment: manual review suffices at current scale; JSON Schema validation is a future improvement.
