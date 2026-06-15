# loom multi-target build
#
# Usage:
#   make help                # show all targets
#   make build               # native debug
#   make release             # native release
#   make mac-arm-release     # one specific target
#   make all-release         # every target, release
#   make all                 # debug + release
#
# Override on command line:
#   make linux-x86-release LINUX_BUILDER=cross
#   make all-release DIST_DIR=/tmp/out

CARGO          ?= cargo
CROSS          ?= cross
LIPO           ?= lipo
DIST_DIR       ?= dist
PACKAGE_OUT_DIR ?= $(DIST_DIR)/packages
# Default to native cargo + musl-cross toolchain; cross+Docker is broken on
# Apple Silicon (rustc segfaults under QEMU). Override with LINUX_BUILDER=cross
# if you actually have a working cross container setup.
LINUX_BUILDER  ?= $(CARGO)

VERSION := $(shell awk -F\" '/^version/ {print $$2; exit}' Cargo.toml 2>/dev/null || echo 0.0.0)
GIT_SHA := $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)

PKG_FLAGS := -p loom-cli -p loom-server
BINS      := loom loom-daemon loom-server

TRIPLE_MAC_ARM     := aarch64-apple-darwin
TRIPLE_MAC_X86     := x86_64-apple-darwin
TRIPLE_LINUX_X86   := x86_64-unknown-linux-musl
TRIPLE_LINUX_ARM   := aarch64-unknown-linux-musl
TRIPLE_WINDOWS_X86 := x86_64-pc-windows-msvc

ALL_TRIPLES := \
  $(TRIPLE_MAC_ARM) \
  $(TRIPLE_MAC_X86) \
  $(TRIPLE_LINUX_X86) \
  $(TRIPLE_LINUX_ARM) \
  $(TRIPLE_WINDOWS_X86)

.DEFAULT_GOAL := help

# ---- Help ----------------------------------------------------------------

.PHONY: help
help:
	@echo "loom build (version=$(VERSION) sha=$(GIT_SHA))"
	@echo ""
	@echo "Native (host triple):"
	@echo "  build                       cargo build (debug)"
	@echo "  release                     cargo build --release"
	@echo ""
	@echo "Per target (-debug | -release):"
	@echo "  mac-arm-{debug,release}     $(TRIPLE_MAC_ARM)"
	@echo "  mac-x86-{debug,release}     $(TRIPLE_MAC_X86)"
	@echo "  mac-universal-{...}         lipo of mac-arm + mac-x86"
	@echo "  linux-x86-{debug,release}   $(TRIPLE_LINUX_X86)"
	@echo "  linux-arm-{debug,release}   $(TRIPLE_LINUX_ARM)"
	@echo "  windows-x86-{debug,release} $(TRIPLE_WINDOWS_X86)"
	@echo ""
	@echo "Bulk:"
	@echo "  all-debug                   every target, debug"
	@echo "  all-release                 every target, release"
	@echo "  all                         debug + release"
	@echo "  package-release             one-shot runtime archives + mac arm64 GUI dmg"
	@echo ""
	@echo "Maintenance:"
	@echo "  install-targets             rustup target add (all triples)"
	@echo "  test                        cargo test --workspace"
	@echo "  fmt / lint                  cargo fmt / clippy"
	@echo "  clean                       rm -rf $(DIST_DIR)"
	@echo "  distclean                   clean + cargo clean"
	@echo ""
	@echo "Overrides:"
	@echo "  CARGO=$(CARGO)  CROSS=$(CROSS)  LIPO=$(LIPO)"
	@echo "  DIST_DIR=$(DIST_DIR)"
	@echo "  PACKAGE_OUT_DIR=$(PACKAGE_OUT_DIR)"
	@echo "  LINUX_BUILDER=$(LINUX_BUILDER)   (set to '$(CROSS)' to use cross-rs containers)"

# ---- Native --------------------------------------------------------------

.PHONY: build release
build:
	$(CARGO) build $(PKG_FLAGS)

release:
	$(CARGO) build --release $(PKG_FLAGS)

# ---- Per-target template -------------------------------------------------
#
# $1 = friendly name (mac-arm)
# $2 = rust triple   (aarch64-apple-darwin)
# $3 = builder       ($(CARGO) or $(CROSS))
#
# Targets are .PHONY — cargo does its own incremental tracking, so we
# always invoke it. The copy step refreshes $(DIST_DIR) with the latest
# artifact even if cargo decided nothing needed rebuilding.

define define-target

.PHONY: $(1)-debug $(1)-release

$(1)-debug:
	$(3) build --target $(2) $(PKG_FLAGS)
	@mkdir -p $(DIST_DIR)/debug/$(2)
	cp target/$(2)/debug/loom        $(DIST_DIR)/debug/$(2)/loom
	cp target/$(2)/debug/loom-daemon $(DIST_DIR)/debug/$(2)/loom-daemon
	cp target/$(2)/debug/loom-server $(DIST_DIR)/debug/$(2)/loom-server

$(1)-release:
	$(3) build --release --target $(2) $(PKG_FLAGS)
	@mkdir -p $(DIST_DIR)/release/$(2)
	cp target/$(2)/release/loom        $(DIST_DIR)/release/$(2)/loom
	cp target/$(2)/release/loom-daemon $(DIST_DIR)/release/$(2)/loom-daemon
	cp target/$(2)/release/loom-server $(DIST_DIR)/release/$(2)/loom-server

endef

$(eval $(call define-target,mac-arm,$(TRIPLE_MAC_ARM),$(CARGO)))
$(eval $(call define-target,mac-x86,$(TRIPLE_MAC_X86),$(CARGO)))
$(eval $(call define-target,linux-x86,$(TRIPLE_LINUX_X86),$(LINUX_BUILDER)))
$(eval $(call define-target,linux-arm,$(TRIPLE_LINUX_ARM),$(LINUX_BUILDER)))

# Windows target — binaries carry .exe extension; keep it so Windows
# users get native executables.
.PHONY: windows-x86-debug windows-x86-release

windows-x86-debug:
	$(CARGO) build --target $(TRIPLE_WINDOWS_X86) $(PKG_FLAGS)
	@mkdir -p $(DIST_DIR)/debug/$(TRIPLE_WINDOWS_X86)
	cp target/$(TRIPLE_WINDOWS_X86)/debug/loom.exe        $(DIST_DIR)/debug/$(TRIPLE_WINDOWS_X86)/loom.exe
	cp target/$(TRIPLE_WINDOWS_X86)/debug/loom-daemon.exe $(DIST_DIR)/debug/$(TRIPLE_WINDOWS_X86)/loom-daemon.exe
	cp target/$(TRIPLE_WINDOWS_X86)/debug/loom-server.exe $(DIST_DIR)/debug/$(TRIPLE_WINDOWS_X86)/loom-server.exe

windows-x86-release:
	$(CARGO) build --release --target $(TRIPLE_WINDOWS_X86) $(PKG_FLAGS)
	@mkdir -p $(DIST_DIR)/release/$(TRIPLE_WINDOWS_X86)
	cp target/$(TRIPLE_WINDOWS_X86)/release/loom.exe        $(DIST_DIR)/release/$(TRIPLE_WINDOWS_X86)/loom.exe
	cp target/$(TRIPLE_WINDOWS_X86)/release/loom-daemon.exe $(DIST_DIR)/release/$(TRIPLE_WINDOWS_X86)/loom-daemon.exe
	cp target/$(TRIPLE_WINDOWS_X86)/release/loom-server.exe $(DIST_DIR)/release/$(TRIPLE_WINDOWS_X86)/loom-server.exe

# ---- macOS universal binary (lipo) ---------------------------------------

UNIVERSAL_DIR := universal-apple-darwin

.PHONY: mac-universal-debug mac-universal-release
mac-universal-debug: mac-arm-debug mac-x86-debug
	@mkdir -p $(DIST_DIR)/debug/$(UNIVERSAL_DIR)
	$(LIPO) -create -output $(DIST_DIR)/debug/$(UNIVERSAL_DIR)/loom \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_ARM)/loom \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_X86)/loom
	$(LIPO) -create -output $(DIST_DIR)/debug/$(UNIVERSAL_DIR)/loom-daemon \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_ARM)/loom-daemon \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_X86)/loom-daemon
	$(LIPO) -create -output $(DIST_DIR)/debug/$(UNIVERSAL_DIR)/loom-server \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_ARM)/loom-server \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_X86)/loom-server

mac-universal-release: mac-arm-release mac-x86-release
	@mkdir -p $(DIST_DIR)/release/$(UNIVERSAL_DIR)
	$(LIPO) -create -output $(DIST_DIR)/release/$(UNIVERSAL_DIR)/loom \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_ARM)/loom \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_X86)/loom
	$(LIPO) -create -output $(DIST_DIR)/release/$(UNIVERSAL_DIR)/loom-daemon \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_ARM)/loom-daemon \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_X86)/loom-daemon
	$(LIPO) -create -output $(DIST_DIR)/release/$(UNIVERSAL_DIR)/loom-server \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_ARM)/loom-server \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_X86)/loom-server

# ---- Bulk ----------------------------------------------------------------

.PHONY: all-debug all-release all
all-debug: \
  mac-arm-debug \
  mac-x86-debug \
  mac-universal-debug \
  linux-x86-debug \
  linux-arm-debug \
  windows-x86-debug

all-release: \
  mac-arm-release \
  mac-x86-release \
  mac-universal-release \
  linux-x86-release \
  linux-arm-release \
  windows-x86-release

all: all-debug all-release

# ---- Maintenance ---------------------------------------------------------

.PHONY: install-targets test fmt lint clean distclean

install-targets:
	rustup target add $(ALL_TRIPLES)

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

clean:
	rm -rf $(DIST_DIR)

distclean: clean
	$(CARGO) clean

# ---- Desktop GUI (Tauri) -------------------------------------------------
#
# Prerequisites (one-time):
#   pnpm --dir apps/gui-web install
#   cargo install tauri-cli --version ^2
# Then:
#   make gui-dev       # launches vite dev server + tauri window
#   make gui-release   # builds a signed/unsigned bundle into crates/gui/target/
#
# The GUI crate is excluded from workspace default-members, so normal
# `cargo build` doesn't pay its compile cost. Invoke through these targets
# or directly with `cargo tauri dev|build` from crates/gui/.

PNPM ?= pnpm
GUI_NO_PROXY_HOSTS ?= localhost,127.0.0.1,::1

.PHONY: gui-deps gui-dev gui-release gui-dmg-mac-arm gui-clean package-release

gui-deps:
	$(PNPM) --dir apps/gui-web install

gui-dev:
	cd crates/gui && NO_PROXY="$(GUI_NO_PROXY_HOSTS),$${NO_PROXY}" no_proxy="$(GUI_NO_PROXY_HOSTS),$${no_proxy}" $(CARGO) tauri dev

gui-release:
	cd crates/gui && $(CARGO) tauri build

gui-dmg-mac-arm: mac-arm-release
	cd crates/gui && $(CARGO) tauri build --target $(TRIPLE_MAC_ARM) --bundles dmg --ci

gui-clean:
	rm -rf apps/gui-web/node_modules apps/gui-web/dist crates/gui/gen

package-release:
	CARGO="$(CARGO)" PNPM="$(PNPM)" DIST_DIR="$(DIST_DIR)" PACKAGE_OUT_DIR="$(PACKAGE_OUT_DIR)" LINUX_BUILDER="$(LINUX_BUILDER)" bash scripts/package-release.sh
