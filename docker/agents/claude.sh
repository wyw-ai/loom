# Loom agent contract: Claude Code (Anthropic)
#
# Contract variables consumed by install-agents.sh / entrypoint-daemon.sh:
#   AGENT_BIN           binary that must appear on PATH after install
#   AGENT_REQUIRED_ENV  space-separated env vars the entrypoint warns about
#   agent_install()     install the CLI (build time and runtime fallback)
#   agent_configure()   optional, generate config files after install
AGENT_BIN="claude"
AGENT_REQUIRED_ENV="ANTHROPIC_API_KEY"

agent_install() {
  npm i -g "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION:-latest}"
}

agent_configure() { :; }
