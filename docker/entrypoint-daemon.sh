#!/bin/sh
# entrypoint-daemon.sh — container entrypoint for loom-daemon.
#
# Reads AGENTS (comma/space separated) from the environment, makes sure each
# requested agent CLI is installed (installing on the fly if the image did
# not bake it), warns about missing credentials declared via each contract's
# AGENT_REQUIRED_ENV, then execs loom-daemon.
#
# Relevant env vars:
#   AGENTS               e.g. "claude,codex". Empty means: skip agent setup.
#   LOOM_SERVER          server RPC URL. Default ws://127.0.0.1:7878/rpc.
#   LOOM_MACHINE_NAME    runtime host name. Default "docker".
#   LOOM_DATA_ROOT       daemon data dir. Default "/data".
#   LOOM_AGENT_STRICT_ENV  "1" turns missing AGENT_REQUIRED_ENV into a fatal
#                        error; default only warns.
set -eu

AGENTS_DIR="${AGENTS_DIR:-/opt/loom/agents}"
INSTALL_AGENTS="${INSTALL_AGENTS:-/usr/local/bin/loom-install-agents}"
AGENTS="${AGENTS:-}"

warn() { printf 'entrypoint-daemon: %s\n' "$*" >&2; }

if [ -n "$AGENTS" ]; then
  for name in $(printf '%s\n' "$AGENTS" | tr ',' ' '); do
    script="$AGENTS_DIR/$name.sh"
    if [ ! -f "$script" ]; then
      warn "unknown agent '$name' (no contract at $script); skipping"
      continue
    fi

    AGENT_BIN=""
    AGENT_REQUIRED_ENV=""
    agent_install() { :; }
    agent_configure() { :; }
    # shellcheck disable=SC1090
    . "$script"

    if ! command -v "$AGENT_BIN" >/dev/null 2>&1; then
      warn "$name: '$AGENT_BIN' not baked into the image; installing at runtime"
      if ! "$INSTALL_AGENTS" "$name"; then
        warn "$name: runtime install failed (non-root container?); continuing without it"
        continue
      fi
    fi

    agent_configure

    for var in $AGENT_REQUIRED_ENV; do
      eval "val=\"\${$var:-}\""
      if [ -z "$val" ]; then
        if [ "${LOOM_AGENT_STRICT_ENV:-0}" = "1" ]; then
          warn "$name: required env var $var is not set; aborting (LOOM_AGENT_STRICT_ENV=1)"
          exit 1
        fi
        warn "$name: $var is not set; runs of this agent will fail until it is provided"
      fi
    done

    printf 'entrypoint-daemon: agent ready: %s (%s)\n' "$name" "$(command -v "$AGENT_BIN")"
  done
fi

if [ "$#" -eq 0 ]; then
  set -- \
    --server "${LOOM_SERVER:-ws://127.0.0.1:7878/rpc}" \
    --machine-name "${LOOM_MACHINE_NAME:-docker}" \
    --data-root "${LOOM_DATA_ROOT:-/data}"
fi

exec loom-daemon "$@"
