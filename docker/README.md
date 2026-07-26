# Loom Docker stack

One command to run `loom-server` plus a `loom-daemon` that auto-connects to
it, with agent CLIs (Claude Code, Codex, …) baked into the daemon image.

## Quick start

```bash
cp .env.example .env       # choose AGENTS, fill in API keys
docker compose up -d --build
```

Then from the host:

```bash
loom who --server ws://127.0.0.1:7878/rpc   # or point Loom Desktop at 127.0.0.1:7878
```

The daemon registers itself as a runtime host named `docker`; the agents
listed in `AGENTS` show up in its provider inventory.

## How agents are defined

Each agent is a small contract script in `docker/agents/<name>.sh`:

- `AGENT_BIN` — the binary that must end up on `PATH`
- `AGENT_REQUIRED_ENV` — env vars the daemon entrypoint warns about when missing
- `agent_install()` — how to install the CLI (npm package, upstream script, …)
- `agent_configure()` — optional file generation after install

`docker/install-agents.sh <name>...` runs the contract (used at image build
time, and at container start for agents not baked in). Adding a new agent =
dropping a new contract script into `docker/agents/` — no Dockerfile or
compose changes needed.

Notes:

- The daemon container runs as the `node` user (not root): claude-code
  refuses `--dangerously-skip-permissions` as root. Mount config dirs at
  `/home/node/.claude` (not `/root/.claude`).
- Runtime installs from the entrypoint are a fallback and need a writable
  global npm prefix; prefer baking agents at build time (`AGENTS` build arg).
- `zcode` ships as a desktop app only; see `docker/agents/zcode.sh` for the
  `ZCODE_INSTALL_CMD` escape hatch.
- OAuth-style CLIs (e.g. Copilot device flow) need a mounted config dir —
  API-key agents are the smooth path in containers.
- Images download Loom binaries via the release `install.sh`, so building
  requires a published release (pin with `LOOM_VERSION=v0.1.0`).

## Layout

- `docker/Dockerfile.server` — `loom-server` on alpine (musl static binary)
- `docker/Dockerfile.daemon` — `loom` + `loom-daemon` + agent CLIs on node:22-alpine
- `docker/agents/*.sh` — per-agent contract scripts
- `docker/install-agents.sh` — contract runner
- `docker/entrypoint-daemon.sh` — enables/validates `AGENTS`, then execs `loom-daemon`
- `docker-compose.yml`, `.env.example` — the outer layer you actually touch
