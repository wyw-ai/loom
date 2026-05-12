#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_BASE="${JOI_E2E_TMPDIR:-/private/tmp}"
TMP="${TMP_BASE%/}/joi-attachment-e2e.$$"
TRANSPORT="${JOI_E2E_TRANSPORT:-file}"
CLI="$ROOT/target/debug/joi"
SERVER="$ROOT/target/debug/joi-server"
JQ="$(command -v jq || true)"

if [[ -z "$JQ" ]]; then
  echo "jq is required to run this attachment E2E script" >&2
  exit 1
fi

mkdir -p "$TMP/bin" "$TMP/config" "$TMP/daemon-data" "$TMP/server-data"

if [[ "$TRANSPORT" == "file" ]]; then
  SERVER_RPC_DIR="$TMP/file-rpc"
  SERVER_URL="file-rpc://$SERVER_RPC_DIR"
  SERVER_ARGS=(--file-rpc "$SERVER_RPC_DIR")
elif [[ "$TRANSPORT" == "unix" ]]; then
  SERVER_SOCKET="$TMP/server.sock"
  SERVER_URL="unix://$SERVER_SOCKET"
  SERVER_ARGS=(--unix-socket "$SERVER_SOCKET")
elif [[ "$TRANSPORT" == "tcp" ]]; then
  SERVER_PORT="${JOI_E2E_PORT:-17878}"
  SERVER_ADDR="127.0.0.1:${SERVER_PORT}"
  SERVER_URL="ws://${SERVER_ADDR}/rpc"
  SERVER_ARGS=(--bind "$SERVER_ADDR")
else
  echo "unsupported JOI_E2E_TRANSPORT=$TRANSPORT (expected file, unix, or tcp)" >&2
  exit 1
fi

cleanup() {
  if [[ -n "${DAEMON_PID:-}" ]]; then
    kill "$DAEMON_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [[ ! -x "$CLI" || ! -x "$SERVER" ]]; then
  cargo build -p joi-cli -p joi-server
fi

cat > "$TMP/bin/opencode" <<EOF
#!/bin/sh
set -eu
ROOT="$ROOT"
CLI="\$ROOT/target/debug/joi"
JQ="$JQ"
PAYLOAD="\${JOI_AGENT_PROFILE:-/tmp}/agent-e2e-file.txt"
mkdir -p "\$(dirname "\$PAYLOAD")"
printf 'agent file from %s for %s\n' "\${JOI_ACTOR:-actor_agent_e2e}" "\${JOI_SCOPE_ID:-unknown}" > "\$PAYLOAD"
ART_JSON="\$(JOI_NO_DAEMON=1 "\$CLI" --json --server "\$JOI_SERVER" --as "\$JOI_ACTOR" attachment upload --target "\$JOI_E2E_TARGET" --path "\$PAYLOAD")"
ART_ID="\$(printf '%s' "\$ART_JSON" | "\$JQ" -r '.artifact.id')"
JOI_NO_DAEMON=1 "\$CLI" --json --server "\$JOI_SERVER" --as "\$JOI_ACTOR" message send --target "\$JOI_E2E_TARGET" --text "agent sent file \$ART_ID" --attachment-id "\$ART_ID" >/dev/null
printf 'agent uploaded %s\n' "\$ART_ID"
EOF
chmod +x "$TMP/bin/opencode"

cat > "$TMP/config/desktop.toml" <<EOF
active = "ws_e2e"

[account]
provider = "test"
staffId = "e2e"
nickname = "Human E2E"
realName = "Human E2E"
email = "e2e@example.invalid"
actorId = "actor_human_e2e"
avatarUrl = ""

[[workspaces]]
id = "ws_e2e"
name = "E2E"
serverUrl = "$SERVER_URL"
actorId = "actor_human_e2e"
displayName = "Human E2E"

[[machines]]
workspaceId = "ws_e2e"
ownerActorId = "actor_human_e2e"
id = "machine_e2e"
name = "E2E Machine"
kind = "local"
dataRoot = "$TMP/daemon-data"

[[machines.agents]]
providerId = "opencode"
actorId = "actor_agent_e2e"
name = "E2E Agent"
description = "Test agent that uploads an attachment through Joi CLI."
autostart = true
EOF

"$SERVER" "${SERVER_ARGS[@]}" --data-dir "$TMP/server-data" > "$TMP/server.log" 2>&1 &
SERVER_PID=$!

for i in $(seq 1 50); do
  if JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e actor list >"$TMP/server-ready.out" 2>"$TMP/server-ready.err"; then
    break
  fi
  sleep 0.1
  if [[ "$i" == 50 ]]; then
    echo "server RPC did not become reachable; last client error follows" >&2
    cat "$TMP/server-ready.err" >&2
    if grep -q "Operation not permitted" "$TMP/server-ready.err"; then
      echo "local WebSocket loopback is blocked in this environment; rerun outside the network sandbox" >&2
    fi
    echo "server log follows" >&2
    cat "$TMP/server.log" >&2
    if grep -q "Operation not permitted" "$TMP/server.log"; then
      echo "local socket creation is blocked in this environment; rerun outside the sandbox or set JOI_E2E_TRANSPORT=tcp on a host that permits loopback" >&2
    fi
    exit 1
  fi
done

CHAN_JSON=$(JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e channel create --title "attachment e2e")
CHAN=$(printf '%s' "$CHAN_JSON" | "$JQ" -r '.channel.id')
ROOT_JSON=$(JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e message send --target "#$CHAN" --text "root for attachment e2e")
ROOT_EVENT=$(printf '%s' "$ROOT_JSON" | "$JQ" -r '.event.id')
THREAD_JSON=$(JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e thread create --channel "$CHAN" --root-event "$ROOT_EVENT" --title "attachment thread")
THREAD=$(printf '%s' "$THREAD_JSON" | "$JQ" -r '.thread.id')
TARGET="#$CHAN:$ROOT_EVENT"

JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e actor upsert actor_agent_e2e --kind agent --display "E2E Agent" >/dev/null
JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e channel invite "$CHAN" actor_agent_e2e >/dev/null

PATH="$TMP/bin:$PATH" JOI_CONFIG_DIR="$TMP/config" JOI_E2E_TARGET="$TARGET" "$CLI" --server "$SERVER_URL" daemon --machine-id machine_e2e --data-root "$TMP/daemon-data" --no-services --no-ipc > "$TMP/daemon.log" 2>&1 &
DAEMON_PID=$!

for i in $(seq 1 50); do
  if grep -q "ready" "$TMP/daemon.log"; then
    break
  fi
  sleep 0.1
  if [[ "$i" == 50 ]]; then
    echo "daemon did not become ready; log follows" >&2
    cat "$TMP/daemon.log" >&2
    exit 1
  fi
done

for i in $(seq 1 50); do
  if JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e actor list | "$JQ" -e '.actors[] | select(.id == "actor_agent_e2e")' >/dev/null; then
    break
  fi
  sleep 0.2
  if [[ "$i" == 50 ]]; then
    echo "agent was not registered; daemon log follows" >&2
    cat "$TMP/daemon.log" >&2
    exit 1
  fi
done

printf 'human upload payload line 1\nline 2\n' > "$TMP/human.txt"
HUMAN_ART_JSON=$(JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e attachment upload --target "$TARGET" --path "$TMP/human.txt")
HUMAN_ART=$(printf '%s' "$HUMAN_ART_JSON" | "$JQ" -r '.artifact.id')
JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e message send --target "$TARGET" --text "human attached file $HUMAN_ART" --attachment-id "$HUMAN_ART" >/dev/null

OFFSET_JSON=$(JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_agent_e2e artifact read "$HUMAN_ART" --offset 6 --max-bytes 6)
printf '%s' "$OFFSET_JSON" | "$JQ" -e '.offset == 6 and .truncated == true and .nextOffset == 12 and (.content == "upload")' >/dev/null
JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_agent_e2e attachment download --id "$HUMAN_ART" --output "$TMP/human-downloaded.txt" --chunk-bytes 5 >/dev/null
cmp "$TMP/human.txt" "$TMP/human-downloaded.txt"

JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e handoff actor_agent_e2e --in "$THREAD" --message "please send a file" >/dev/null
AGENT_ART=""
for i in $(seq 1 80); do
  EVENTS=$(JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e message read --target "$TARGET" --limit 100)
  AGENT_ART=$(printf '%s' "$EVENTS" | "$JQ" -r '.events[] | select(.actorId == "actor_agent_e2e") | select(.payload.text | strings | startswith("agent sent file")) | .relations[] | select(.kind == "attaches_artifact") | .target.id' | tail -n 1)
  if [[ -n "$AGENT_ART" && "$AGENT_ART" != "null" ]]; then
    break
  fi
  sleep 0.25
  if [[ "$i" == 80 ]]; then
    echo "agent did not send attached file; daemon log follows" >&2
    cat "$TMP/daemon.log" >&2
    exit 1
  fi
done

AGENT_READ=$(JOI_CONFIG_DIR="$TMP/config" JOI_NO_DAEMON=1 "$CLI" --json --server "$SERVER_URL" --as actor_human_e2e artifact read "$AGENT_ART" --max-bytes 200)
printf '%s' "$AGENT_READ" | "$JQ" -e '.content | contains("agent file from actor_agent_e2e")' >/dev/null

printf 'E2E_OK transport=%s tmp=%s channel=%s thread=%s humanArtifact=%s agentArtifact=%s\n' "$TRANSPORT" "$TMP" "$CHAN" "$THREAD" "$HUMAN_ART" "$AGENT_ART"
