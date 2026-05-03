#!/usr/bin/env bash
# scripts/e2e/local-up.sh — bring up a local joi stack for e2e dev.
#
# Layout (under $JOI_E2E_ROOT, default /tmp/joi-e2e):
#   server-data/        joi-server's --data-dir
#   agent-specs/        per-actor AgentSpec files (linked from data/agents/)
#   service-specs/      per-service ServiceSpec files (linked from data/services/)
#   agent-data/         JOI_AGENT_DATA_ROOT — agents/<id>/profile/claude/settings.json
#                       lives here (linked to ~/.claude/settings-glm.json by default)
#   service-data/       JOI_SERVICE_HOST_DATA
#   pids/               PID files for joi-server / joi agent serve / joi service serve
#   logs/               stdout/stderr capture
#
# Usage:
#   scripts/e2e/local-up.sh build    # cargo build --release
#   scripts/e2e/local-up.sh start    # boot server + hosts
#   scripts/e2e/local-up.sh status
#   scripts/e2e/local-up.sh stop
#   scripts/e2e/local-up.sh nuke     # stop + wipe $JOI_E2E_ROOT
#
# After `start`, set:
#   export JOI_SERVER=ws://127.0.0.1:${JOI_E2E_PORT:-7900}/rpc
#   export JOI_AGENT_DATA_ROOT=$JOI_E2E_ROOT/agent-data
#   export JOI_SERVICE_HOST_DATA=$JOI_E2E_ROOT/service-data

set -euo pipefail

ROOT="${JOI_E2E_ROOT:-/tmp/joi-e2e}"
PORT="${JOI_E2E_PORT:-7900}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_JOI="$REPO_ROOT/target/release/joi"
BIN_SERVER="$REPO_ROOT/target/release/joi-server"

usage() { sed -n '2,21p' "${BASH_SOURCE[0]}"; exit 1; }

cmd_build() {
  cd "$REPO_ROOT"
  cargo build --release -p joi-server -p joi-cli
  echo "built: $BIN_JOI / $BIN_SERVER"
}

prepare_dirs() {
  mkdir -p "$ROOT"/{server-data,agent-specs,service-specs,agent-data,service-data,pids,logs,fixtures}
  # Pre-bake a bare repo for mount projection. Re-runs are no-ops.
  "$REPO_ROOT/tests/e2e/fixtures/make-bare-repo.sh" "$ROOT/fixtures/repo.git" >/dev/null
}

link_specs() {
  # Mirror data/agents/*/spec.json into $ROOT/agent-specs/<id>/spec.json so
  # the nested loader picks them up. Same for services.
  for d in "$REPO_ROOT"/data/agents/*/; do
    [ -d "$d" ] || continue
    id="$(basename "$d")"
    [ -f "$d/spec.json" ] || continue
    mkdir -p "$ROOT/agent-specs/$id"
    cp -f "$d/spec.json" "$ROOT/agent-specs/$id/spec.json"
    if [ -d "$d/bundle" ]; then
      rm -rf "$ROOT/agent-specs/$id/bundle"
      cp -R "$d/bundle" "$ROOT/agent-specs/$id/bundle"
    fi
  done
  for d in "$REPO_ROOT"/data/services/*/; do
    [ -d "$d" ] || continue
    id="$(basename "$d")"
    [ -f "$d/spec.json" ] || continue
    mkdir -p "$ROOT/service-specs/$id"
    cp -f "$d/spec.json" "$ROOT/service-specs/$id/spec.json"
    if [ -d "$d/bundle" ]; then
      rm -rf "$ROOT/service-specs/$id/bundle"
      cp -R "$d/bundle" "$ROOT/service-specs/$id/bundle"
    fi
  done
}

prepare_profiles() {
  # AgentPaths::profile = $JOI_AGENT_DATA_ROOT/agents/<actor_id>/profile.
  # We pre-create that path and link claude/settings.json so actor_profile
  # provider mode finds it on first turn.
  src="${JOI_E2E_CLAUDE_SETTINGS:-$HOME/.claude/settings-glm.json}"
  if [ ! -f "$src" ]; then
    echo "WARN: $src not found; agents using actor_profile mode will fail." >&2
  fi
  for d in "$ROOT"/agent-specs/*/; do
    [ -d "$d" ] || continue
    id="$(basename "$d")"
    actor="$(awk -F'"' '/"id"/ {print $4; exit}' "$d/spec.json")"
    [ -n "$actor" ] || actor="$id"
    pdir="$ROOT/agent-data/agents/$actor/profile/claude"
    mkdir -p "$pdir"
    if [ -f "$src" ] && [ ! -e "$pdir/settings.json" ]; then
      ln -sf "$src" "$pdir/settings.json"
    fi
  done
}

start_server() {
  if pgrep -f "joi-server.*--bind 127.0.0.1:$PORT" >/dev/null 2>&1; then
    echo "joi-server already running on $PORT"; return
  fi
  nohup env PATH="$REPO_ROOT/target/release:$PATH" \
    "$BIN_SERVER" --bind "127.0.0.1:$PORT" --data-dir "$ROOT/server-data" \
    >"$ROOT/logs/server.log" 2>&1 &
  echo $! > "$ROOT/pids/server.pid"
  sleep 1
  if ! kill -0 "$(cat "$ROOT/pids/server.pid")" 2>/dev/null; then
    echo "joi-server failed to start; tail $ROOT/logs/server.log" >&2; exit 1
  fi
  echo "joi-server pid=$(cat "$ROOT/pids/server.pid")  ws://127.0.0.1:$PORT/rpc"
}

start_agent_host() {
  if [ -f "$ROOT/pids/agent-host.pid" ] && kill -0 "$(cat "$ROOT/pids/agent-host.pid")" 2>/dev/null; then
    echo "joi agent serve already running"; return
  fi
  nohup env \
    PATH="$REPO_ROOT/target/release:$PATH" \
    JOI_SERVER="ws://127.0.0.1:$PORT/rpc" \
    JOI_AGENT_DATA_ROOT="$ROOT/agent-data" \
    "$BIN_JOI" agent serve --specs "$ROOT/agent-specs" \
    >"$ROOT/logs/agent-host.log" 2>&1 &
  echo $! > "$ROOT/pids/agent-host.pid"
  sleep 1
  if ! kill -0 "$(cat "$ROOT/pids/agent-host.pid")" 2>/dev/null; then
    echo "agent host failed to start; tail $ROOT/logs/agent-host.log" >&2; exit 1
  fi
  echo "joi agent serve pid=$(cat "$ROOT/pids/agent-host.pid")"
}

start_service_host() {
  if [ -f "$ROOT/pids/service-host.pid" ] && kill -0 "$(cat "$ROOT/pids/service-host.pid")" 2>/dev/null; then
    echo "joi service serve already running"; return
  fi
  # Default mr-detector to the merged fixture so the local "thread close"
  # path exercises end-to-end without a real MR backend. Driver scripts
  # may override before invoking poll directly.
  : "${MR_DETECTOR_FETCH_CMD:=$REPO_ROOT/tests/e2e/fixtures/mr-fetch-merged.sh}"
  nohup env \
    PATH="$REPO_ROOT/target/release:$PATH" \
    JOI_SERVER="ws://127.0.0.1:$PORT/rpc" \
    JOI_SERVICE_HOST_DATA="$ROOT/service-data" \
    MR_DETECTOR_FETCH_CMD="$MR_DETECTOR_FETCH_CMD" \
    "$BIN_JOI" service serve --specs "$ROOT/service-specs" \
    >"$ROOT/logs/service-host.log" 2>&1 &
  echo $! > "$ROOT/pids/service-host.pid"
  sleep 1
  if ! kill -0 "$(cat "$ROOT/pids/service-host.pid")" 2>/dev/null; then
    echo "service host failed to start; tail $ROOT/logs/service-host.log" >&2; exit 1
  fi
  echo "joi service serve pid=$(cat "$ROOT/pids/service-host.pid")"
}

cmd_start() {
  prepare_dirs
  link_specs
  prepare_profiles
  start_server
  start_agent_host
  start_service_host
  cat <<EOF
ready.
  export JOI_SERVER=ws://127.0.0.1:$PORT/rpc
  export JOI_AGENT_DATA_ROOT=$ROOT/agent-data
  export JOI_SERVICE_HOST_DATA=$ROOT/service-data
  export JOI_AGENT_SPECS=$ROOT/agent-specs
  export JOI_SERVICE_SPECS=$ROOT/service-specs
specs:  $ROOT/agent-specs  $ROOT/service-specs
fixtures: $ROOT/fixtures/repo.git (bare); MR_DETECTOR_FETCH_CMD=$MR_DETECTOR_FETCH_CMD
logs:   $ROOT/logs/
EOF
}

stop_pid_file() {
  local file="$1" name="$2"
  [ -f "$file" ] || return 0
  local pid
  pid="$(cat "$file")"
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    sleep 1
    if kill -0 "$pid" 2>/dev/null; then
      kill -9 "$pid" 2>/dev/null || true
    fi
    echo "stopped $name pid=$pid"
  fi
  rm -f "$file"
}

cmd_stop() {
  stop_pid_file "$ROOT/pids/service-host.pid" "service host"
  stop_pid_file "$ROOT/pids/agent-host.pid" "agent host"
  stop_pid_file "$ROOT/pids/server.pid" "joi-server"
}

cmd_status() {
  for n in server agent-host service-host; do
    f="$ROOT/pids/$n.pid"
    if [ -f "$f" ] && kill -0 "$(cat "$f")" 2>/dev/null; then
      echo "$n: running pid=$(cat "$f")"
    else
      echo "$n: stopped"
    fi
  done
}

cmd_nuke() {
  cmd_stop
  rm -rf "$ROOT"
  echo "wiped $ROOT"
}

case "${1:-}" in
  build)  cmd_build ;;
  start)  cmd_start ;;
  stop)   cmd_stop ;;
  status) cmd_status ;;
  nuke)   cmd_nuke ;;
  *)      usage ;;
esac
