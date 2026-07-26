---
name: local-launch
description: Bring up a working local Loom environment (server + daemon + agents) and verify it actually works end to end. Use this skill whenever the user says things like "start loom locally", "bring up the loom environment", "run a demo", "spin up server/daemon", "test loom locally", or mentions docker compose, loom-server, loom-daemon, or agent smoke testing — even if they never say the words "local launch".
---

# Local Loom Launch

Goal: get from zero to "an agent replies in a channel" — a working state, not
just running processes.

## Two paths — pick one first

**Path A: Docker (recommended)** — one command brings up server + daemon with
agent CLIs baked into the image; clean and reproducible. Requires a working
Docker setup and a published release (images download Loom binaries via the
release `install.sh`).

**Path B: build from source** — for iterating on the code itself. No Docker
needed, but requires a Rust toolchain (and GNU make on Windows).

If unsure, ask the user one question: "run it quickly to use it" (A) or
"verify local code changes" (B).

## Path A: Docker

1. Preconditions: `docker version` and `docker compose version` must work. If
   not, stop and tell the user.
2. Configure: `cp .env.example .env`, then fill in:
   - `AGENTS` (comma-separated, e.g. `claude,codex`) — whatever you list here
     becomes available as commands inside the daemon.
   - The matching API keys (e.g. `ANTHROPIC_API_KEY`). Agents that use a
     mounted config (e.g. a claude `settings.json`) can skip keys — see
     "Config injection" below.
3. Build and start: `docker compose up -d --build`.
4. Release download is controlled by build args: `LOOM_REPO` (default
   `wyw-ai/loom`) and `LOOM_VERSION` (default `latest`). **Anonymous downloads
   404 on private repos.** In that case, fetch the assets with credentials
   first (e.g. `gh release download`) — `install.sh`, the platform `tar.gz`,
   and `SHA256SUMS` — put them in the build context, and temporarily change
   the Dockerfile's download step to `COPY` + `install.sh --package-dir
   <assets dir>`. Checksum verification is unchanged, so asset integrity is
   still enforced.

## Path B: build from source

```bash
make build
export PATH="$PWD/target/debug:$PATH"
loom-server --bind 127.0.0.1:7878   # terminal 1
loom who                            # terminal 2, first run writes ~/.loom/cli.toml
loom channel create --title general
loom chat
```

On Windows without make, build with `cargo` directly — see
`docs/windows-build-guide.md`.

## Verification (mandatory — do not skip)

"Processes are up" is not "it works". Verify in order:

1. `loom who --server ws://127.0.0.1:7878/rpc` — server answers.
2. `loom provider list` — the target provider shows `detected` (not
   `missing`).
3. Create an agent:
   `loom machine agent create --machine local --provider claude --name <name> --model <model> --instructions "..."`
4. Create a channel and invite the agent:
   `loom channel create --title t` → `loom channel invite <chan> <actor_id>`
5. Ask something:
   `loom message ask @<actor_id> --target "#<chan>" --text "What is 2+2?"`
6. **Wait 60–90 seconds** (adapter cold start is 30–60s, plus model latency),
   then **read the thread**:
   `loom message read --target "#<chan>:<msg_id>"`

Note: **agent replies land in the thread, not the channel timeline.** That is
by design, not a bug — you must read `#chan:msg_id` to see the reply.

## Known pitfalls (all verified in live testing, ordered by hit rate)

1. **Port already in use**: a dev `loom-server` often owns host port 7878.
   Don't kill the user's process — remap the host port in compose (e.g.
   `127.0.0.1:17878:7878`). Compose merges `ports` across override files by
   appending; use `!override` to replace.
2. **claude refuses root**: `--dangerously-skip-permissions cannot be used
   with root`. The daemon image runs as the `node` user (uid 1000); a mounted
   claude config goes to `/home/node/.claude`, not `/root/.claude`.
3. **Missing onboarding file**: claude-code without `~/.claude.json` prints a
   warning instead of working. `claude.sh`'s `agent_configure()` writes
   `{"hasCompletedOnboarding":true}` when absent, and the entrypoint runs
   `agent_configure` on every start.
4. **Stale volume ownership**: if a named volume was previously written by a
   root-era container, workers fail with `Permission denied (os error 13)`.
   Fix: `docker run --rm -v <volume>:/data alpine chown -R 1000:1000 /data`.
5. **Polluted mount dir**: a config dir written by a root container ends up
   with root-owned `backups/`, `projects/`, `sessions/` that the node
   container cannot write. Fix: `sudo chown -R 1000:1000 <dir>`. Prefer
   mounting a **copy** of `~/.claude`, not the live one.
6. **Agent specs are not persistent**: specs live in container-local
   `/home/node/.loom/agents`; recreating the container wipes them
   (`machine list` shows `agents=0`). Just recreate the agent.
7. **Two credential classes**: API-key agents go through `.env`;
   OAuth/config-file agents (settings.json) go through a mount. An invalid
   token (401) or exhausted quota (429) surfaces as "Agent run failed" in the
   reply — reproduce with a bare `claude -p "hi"` inside the container first
   to tell a config problem from a loom problem.

## Config injection (example: claude with a third-party endpoint)

The `env` block of `settings.json` (`ANTHROPIC_BASE_URL`,
`ANTHROPIC_AUTH_TOKEN`, model overrides) is read by claude-code. Both
injection styles are verified:

- **Mount**: add `- /path/to/config-copy:/home/node/.claude` to the daemon
  service (a compose override file works well).
- **Env vars**: convert the `env` block of `settings.json` to `KEY=VALUE`
  lines in `.env.claude` and add it to `env_file`.

## Cleanup

When testing is done, ask the user before tearing down:
`docker compose down -v` (removes volumes too) and delete the test directory.
Never clean up by default — the environment may still be in use.
