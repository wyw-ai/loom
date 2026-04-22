# joi-apps multi-target build
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
#   make linux-x86-release LINUX_BUILDER=cargo
#   make all-release DIST_DIR=/tmp/out

CARGO          ?= cargo
CROSS          ?= cross
LIPO           ?= lipo
DIST_DIR       ?= dist
LINUX_BUILDER  ?= $(CROSS)

VERSION := $(shell awk -F\" '/^version/ {print $$2; exit}' Cargo.toml 2>/dev/null || echo 0.0.0)
GIT_SHA := $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)

PKG_FLAGS := -p joi-cli -p joi-server
BINS      := joi joi-server

TRIPLE_MAC_ARM   := aarch64-apple-darwin
TRIPLE_MAC_X86   := x86_64-apple-darwin
TRIPLE_LINUX_X86 := x86_64-unknown-linux-musl
TRIPLE_LINUX_ARM := aarch64-unknown-linux-musl

ALL_TRIPLES := \
  $(TRIPLE_MAC_ARM) \
  $(TRIPLE_MAC_X86) \
  $(TRIPLE_LINUX_X86) \
  $(TRIPLE_LINUX_ARM)

.DEFAULT_GOAL := help

# ---- Help ----------------------------------------------------------------

.PHONY: help
help:
	@echo "joi-apps build (version=$(VERSION) sha=$(GIT_SHA))"
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
	@echo ""
	@echo "Bulk:"
	@echo "  all-debug                   every target, debug"
	@echo "  all-release                 every target, release"
	@echo "  all                         debug + release"
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
	@echo "  LINUX_BUILDER=$(LINUX_BUILDER)   (set to '$(CARGO)' to skip cross)"

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
	cp target/$(2)/debug/joi        $(DIST_DIR)/debug/$(2)/joi
	cp target/$(2)/debug/joi-server $(DIST_DIR)/debug/$(2)/joi-server

$(1)-release:
	$(3) build --release --target $(2) $(PKG_FLAGS)
	@mkdir -p $(DIST_DIR)/release/$(2)
	cp target/$(2)/release/joi        $(DIST_DIR)/release/$(2)/joi
	cp target/$(2)/release/joi-server $(DIST_DIR)/release/$(2)/joi-server

endef

$(eval $(call define-target,mac-arm,$(TRIPLE_MAC_ARM),$(CARGO)))
$(eval $(call define-target,mac-x86,$(TRIPLE_MAC_X86),$(CARGO)))
$(eval $(call define-target,linux-x86,$(TRIPLE_LINUX_X86),$(LINUX_BUILDER)))
$(eval $(call define-target,linux-arm,$(TRIPLE_LINUX_ARM),$(LINUX_BUILDER)))

# ---- macOS universal binary (lipo) ---------------------------------------

UNIVERSAL_DIR := universal-apple-darwin

.PHONY: mac-universal-debug mac-universal-release
mac-universal-debug: mac-arm-debug mac-x86-debug
	@mkdir -p $(DIST_DIR)/debug/$(UNIVERSAL_DIR)
	$(LIPO) -create -output $(DIST_DIR)/debug/$(UNIVERSAL_DIR)/joi \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_ARM)/joi \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_X86)/joi
	$(LIPO) -create -output $(DIST_DIR)/debug/$(UNIVERSAL_DIR)/joi-server \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_ARM)/joi-server \
	  $(DIST_DIR)/debug/$(TRIPLE_MAC_X86)/joi-server

mac-universal-release: mac-arm-release mac-x86-release
	@mkdir -p $(DIST_DIR)/release/$(UNIVERSAL_DIR)
	$(LIPO) -create -output $(DIST_DIR)/release/$(UNIVERSAL_DIR)/joi \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_ARM)/joi \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_X86)/joi
	$(LIPO) -create -output $(DIST_DIR)/release/$(UNIVERSAL_DIR)/joi-server \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_ARM)/joi-server \
	  $(DIST_DIR)/release/$(TRIPLE_MAC_X86)/joi-server

# ---- Bulk ----------------------------------------------------------------

.PHONY: all-debug all-release all
all-debug: \
  mac-arm-debug \
  mac-x86-debug \
  mac-universal-debug \
  linux-x86-debug \
  linux-arm-debug

all-release: \
  mac-arm-release \
  mac-x86-release \
  mac-universal-release \
  linux-x86-release \
  linux-arm-release

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
