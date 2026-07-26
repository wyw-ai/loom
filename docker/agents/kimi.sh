# Loom agent contract: Kimi Code CLI (Moonshot AI)
#
# Upstream installs via an official shell script (no verified npm package at
# the time of writing). Auth: run `/login` (OAuth) or use a Kimi Code console
# API key — set KIMI_API_KEY / MOONSHOT_API_KEY in .env depending on your
# account type.
AGENT_BIN="kimi"
AGENT_REQUIRED_ENV=""

agent_install() {
  if [ -n "${KIMI_INSTALL_CMD:-}" ]; then
    sh -c "$KIMI_INSTALL_CMD"
    return
  fi
  curl -LsSf https://code.kimi.com/install.sh | bash
}

agent_configure() { :; }
