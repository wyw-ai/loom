# Loom agent contract: OpenCode
#
# OpenCode talks to model providers configured by the user (OpenAI,
# Anthropic, ...); which key it needs depends on the chosen provider,
# so AGENT_REQUIRED_ENV stays empty — set the key for your provider in .env.
AGENT_BIN="opencode"
AGENT_REQUIRED_ENV=""

agent_install() {
  npm i -g "opencode-ai@${OPENCODE_VERSION:-latest}"
}

agent_configure() { :; }
