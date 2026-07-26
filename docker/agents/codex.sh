# Loom agent contract: Codex CLI (OpenAI)
AGENT_BIN="codex"
AGENT_REQUIRED_ENV="OPENAI_API_KEY"

agent_install() {
  npm i -g "@openai/codex@${CODEX_VERSION:-latest}"
}

agent_configure() { :; }
