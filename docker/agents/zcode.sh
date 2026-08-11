# Loom agent contract: ZCode (Z.AI)
#
# ZCode ships as a desktop application (ZCode.app) with no official headless
# npm distribution, so it cannot be installed into a container image out of
# the box. If you have a working install route (internal mirror, unpacked
# app bundle), point ZCODE_INSTALL_CMD at it and the contract works like any
# other agent.
AGENT_BIN="zcode"
AGENT_REQUIRED_ENV=""

agent_install() {
  if [ -z "${ZCODE_INSTALL_CMD:-}" ]; then
    echo "agents/zcode.sh: ZCode is desktop-app distributed; set ZCODE_INSTALL_CMD to a custom install command" >&2
    exit 1
  fi
  sh -c "$ZCODE_INSTALL_CMD"
}

agent_configure() { :; }
