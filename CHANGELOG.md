# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Loom is pre-1.0: the JSON-RPC protocol and the on-disk formats are still
drafts and may change between releases.

## [Unreleased]

### Changed

- Context layer: `agentcontext.json` resources that omit `priority` now
  inherit the base-layer value (new resources default to 100) instead of
  silently deserializing to 0. Configs relying on the implicit 0
  (never-skip reserved value) must declare `priority: 0` explicitly.
- Context layer: an explicit `priority` on a resource entry now takes
  effect on assembly order, overriding the resource's built-in priority
  (previously a declared priority differing from the built-in value was
  silently ignored). Default specs are unaffected — their declared
  priorities match the built-in values.
- Internal: memory context resource extracted into a standalone
  `plugin-memory` crate and wired through the same ContextResource
  factory chain as every other resource (compose no longer special-cases
  memory). Zero user-visible behavior change: section content, ordering,
  degradation and budget waterfall are golden-test equivalent to the
  previous pre-rendered pipeline. `plugin-memory` depends only on
  `context-layer-core`/`loom-proto` (no `agent-runtime` dependency),
  proving the third-party plugin path is privilege-free.

## [0.1.1] - 2026-07-30

- Add repository guardrails for protected release branches.
- Harden release packaging and GitHub Pages deployment.
- Fix Windows runtime packaging and download metadata.
