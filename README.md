# joi-apps

A no-auth reference server for the [Open Multi-Actor Collaboration Protocol v0](docs/protocol/open-multi-actor-collaboration-protocol-v0.md), with:

- a Rust **server** (`joi-server`) speaking JSON-RPC 2.0 over WebSocket,
- a Rust **CLI** (`joi`) for humans to drive conversations from the terminal,
- pluggable **ACP-protocol agent runtimes** declared as `agents/*.json`.

The point of v0 is small: a human opens a conversation in a CLI, drops in one or more configured ACP agents via `handoff`, and watches them stream back as protocol events. No auth, no GUI, no database.

## Layout

```
crates/proto    shared protocol types + JSON-RPC envelopes
crates/server   joi-server binary (WebSocket + ACP runtime host)
crates/cli      joi binary (human terminal client)
agents/         agent JSON specs
data/           journal + artifacts (created at runtime)
docs/           protocol spec
```

## Quickstart

```sh
cargo build

# Terminal 1: server
cargo run -p joi-server -- --bind 127.0.0.1:7878 --data-dir ./data --agents-dir ./agents

# Terminal 2: install an agent from the bundled marketplace, then chat
cargo run -p joi-cli -- agent install claude-acp --actor-id actor_claude --name "Claude"
cargo run -p joi-cli -- space create --title "Demo"
cargo run -p joi-cli -- conv create --space <space_id> --title "Kickoff"
cargo run -p joi-cli -- chat --in <conv_id>
```

In the chat TUI:

| keys                          | what happens                                                              |
| ----------------------------- | ------------------------------------------------------------------------- |
| any plain text + `Enter`      | `event/append content.add` to the conversation                            |
| `/` (start of input)          | inline slash-command dropdown above the input box                         |
| `Tab` / `↑` / `↓`             | navigate the dropdown                                                     |
| `Enter` (with dropdown open)  | populate input with the chosen command (e.g. `/handoff `) — does not send |
| `/handoff` + `Enter`          | open modal target picker (lists agents + humans), pick to send the offer  |
| `/action` + `Enter`           | open modal picker over pending `action.request` events                    |
| `/agents` + `Enter`           | print registered agents in the history pane                               |
| `/quit` + `Enter` or `Ctrl-C` | leave the TUI                                                             |
| `PgUp` / `PgDn` / `End`       | scroll history                                                            |

Outgoing messages render with `⏳` until the server echoes them through the stream, then flip to `✓`.

## Configuring an ACP agent

Three ways to add an agent — all of them write a JSON spec into `agents/` that the server reloads on the next `agent/list`:

1. **From the bundled marketplace** (recommended):
   ```sh
   joi agent marketplace                                        # list bundled entries
   joi agent install claude-acp --actor-id actor_claude --name "Claude"
   ```
   Resolves `npx`/`uvx`/binary on PATH (no downloads); writes the spec.

2. **Interactively**, for a custom command:
   ```sh
   joi agent add
   ```

3. **Hand-rolled**, for power users:
   ```json
   {
     "actor": {
       "id": "actor_my_agent",
       "displayName": "My Agent",
       "kind": "agent",
       "capabilities": {}
     },
     "transport": {
       "kind": "acp_stdio",
       "command": "my-acp-binary",
       "args": [],
       "env": {},
       "cwd": "{agent.workspace}",
       "authMethod": null
     },
     "autostart": false
   }
   ```
   Save as `agents/<actor-id>.json` or register at runtime: `joi agent register <path>`.

The server scans `agents/` at boot and registers each as an `Actor { kind: agent }`. The runtime is started lazily when the agent first becomes the target of a `handoff/create` or `targets` relation, unless `"autostart": true`.

Template variables in `cwd` / `env` values:

- `{agent.workspace}` → `data/agents/<actor-id>/workspace`
- `{agent.cache}`     → `data/agents/<actor-id>/cache`
- `{agent.logs}`      → `data/agents/<actor-id>/logs`
- `{agent.root}`      → `data/agents/<actor-id>`

## Reading state from the CLI (for humans and agents)

Read-only RPCs are wrapped as subcommands so an ACP child process (or any shell)
can introspect the server. Add `--json` (or `JOI_JSON=1`) to any output-producing
command to get a single-line JSON document instead of the human-friendly text.

```sh
joi --json space list
joi --json conv list --space <space_id>
joi --json actor list
joi --json event list --in <conv_id> --limit 200          # scope/read on a conversation
joi --json event list --in <space_id> --space             # scope/read on a space
joi --json event list --in <conv_id> --before <event_id>  # paginate older
joi --json agent list
```

When the server spawns an ACP child it injects two environment variables (only
if the agent's spec doesn't already set them):

- `JOI_SERVER` → the WebSocket URL the server is bound to (e.g. `ws://127.0.0.1:7878/rpc`)
- `JOI_ACTOR`  → the agent's own actor id

so the child can run `joi --json event list --in <conv_id>` etc. without any
extra flags. The very first prompt of each session is also prefixed with a
short auto-generated manifest telling the model who it is, what scope it is in,
and which read-only commands are available; subsequent prompts are clean.

## What's not in v0

- no auth, no RBAC
- no HTTP/SSE transport (WebSocket only)
- artifact ingress is `inline_text` only
- empty `mcpServers` is passed to ACP children — bring your own
- no GUI, no federation, no SQLite, no automated test suite
