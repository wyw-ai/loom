# Loom agent contract: GitHub Copilot CLI
#
# Copilot CLI signs in with a GitHub account (OAuth device flow) or a token.
# In a container, mount a prepared config dir or provide a token env var;
# there is no single mandatory key, so AGENT_REQUIRED_ENV stays empty.
AGENT_BIN="copilot"
AGENT_REQUIRED_ENV=""

agent_install() {
  npm i -g "@github/copilot@${COPILOT_VERSION:-latest}"
}

agent_configure() { :; }
