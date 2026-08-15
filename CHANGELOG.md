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

## [0.1.1] - 2026-07-30

- Add repository guardrails for protected release branches.
- Harden release packaging and GitHub Pages deployment.
- Fix Windows runtime packaging and download metadata.
