# Loom agent contract: Qoder CLI
AGENT_BIN="qodercli"
AGENT_REQUIRED_ENV=""

agent_install() {
  npm i -g "@qoder-ai/qodercli@${QODER_VERSION:-latest}"
}

agent_configure() { :; }
