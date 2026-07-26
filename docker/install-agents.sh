#!/bin/sh
# install-agents.sh <agent> [agent...]
#
# Installs agent CLIs by sourcing contract scripts from docker/agents/.
# Used both at image build time (Dockerfile.daemon) and at container start
# (entrypoint-daemon.sh, as a fallback for agents not baked into the image).
#
# Each contract script in $AGENTS_DIR/<name>.sh defines:
#   AGENT_BIN           binary that must appear on PATH after install
#   AGENT_REQUIRED_ENV  (used by the entrypoint, ignored here)
#   agent_install()     how to install the CLI
#   agent_configure()   optional post-install file generation
set -eu

if [ -z "${AGENTS_DIR:-}" ]; then
  _self_dir="$(CDPATH= cd "$(dirname "$0")" && pwd)"
  if [ -d "$_self_dir/agents" ]; then
    # Running from the repo (docker/install-agents.sh next to docker/agents).
    AGENTS_DIR="$_self_dir/agents"
  else
    # Installed into an image (contracts live in /opt/loom/agents).
    AGENTS_DIR="/opt/loom/agents"
  fi
fi

fail() {
  printf 'install-agents: %s\n' "$*" >&2
  exit 1
}

[ "$#" -ge 1 ] || fail "usage: install-agents.sh <agent> [agent...]"

for name in $(printf '%s\n' "$*" | tr ',' ' '); do
  script="$AGENTS_DIR/$name.sh"
  [ -f "$script" ] || fail "unknown agent '$name' (no contract at $script)"

  AGENT_BIN=""
  AGENT_REQUIRED_ENV=""
  agent_install() { fail "agent '$name' does not define agent_install()"; }
  agent_configure() { :; }
  # shellcheck disable=SC1090
  . "$script"
  [ -n "$AGENT_BIN" ] || fail "agent '$name' does not define AGENT_BIN"

  if command -v "$AGENT_BIN" >/dev/null 2>&1; then
    printf 'install-agents: %s: already installed (%s)\n' "$name" "$(command -v "$AGENT_BIN")"
  else
    printf 'install-agents: %s: installing...\n' "$name"
    agent_install
    command -v "$AGENT_BIN" >/dev/null 2>&1 \
      || fail "$name: install finished but '$AGENT_BIN' is still not on PATH"
    printf 'install-agents: %s: installed (%s)\n' "$name" "$(command -v "$AGENT_BIN")"
  fi

  agent_configure
done
