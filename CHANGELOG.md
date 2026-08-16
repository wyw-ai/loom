# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Loom is pre-1.0: the JSON-RPC protocol and the on-disk formats are still
drafts and may change between releases.

## [Unreleased]

### Added

- CLI: `loom plugin list` — unified entry point listing context layer
  plugins across official, external and builtin sources, with ghost
  detection (embedded manifests vs runtime factory table, both
  directions). Supports `--verbose` (source/version/registration
  detail) and `--json` (experimental) output forms.
- Plugin manifest schema v2 (`plugin.json`): rejects fields removed by
  the single-concept model (`registration`, `crate`, `scope`) and
  reserves the `executable` field for the future process-boundary
  form. v1 manifests remain accepted and are normalized to v2 at
  build time.
- Workspace crate `plugin-context-tier`: the context-tier plugin
  migrated from its external repository into the workspace
  (`crates/plugin-context-tier`, manifest id `plugin-context-tier`,
  version 1.1.0 kept as an independent semantic version).
- Skills-only internal plugins: `skills/loom-skills` (loom +
  attachment skills) and `skills/actor-circuit` (L0/L1/L2
  collaboration vocabulary, MIT-attributed from the upstream
  read-only clone) onboarded with v2 manifests at layer `skill`;
  a skill-layer manifest declaring `context_resources` now fails
  loud at load time.

### Changed

- Context layer: official plugin sources are now data-driven from
  `crates/cli/official-plugins.json` (embedded at build time) instead
  of a hardcoded list; official plugin additions or removals no
  longer require code changes.
- Context layer: resource factories now receive the merged resource
  config envelope. The memory resource reads its spec from the
  `"memory"` config key, merged per-field with the actor-side spec
  (actor values win on conflict, conflicts logged at error level with
  both values). Malformed config degrades to defaults with a warning
  instead of skipping the resource.
- Context layer: a plugin resource returning an empty section list now
  logs a warning (scheme plus a config/registration hint) instead of
  silently contributing nothing; assembly order and behavior are
  otherwise unchanged.
- Docs: repository documentation aligned to the single-concept model —
  context layer is the sole top-level concept (AOP middle layer) and
  "plugin" is the role word for supply units; new `plugin-guide.md`
  and `plugin-list.md` under `docs/context-layer/`.
- Context layer: the memory context resource is now a fully normalized
  plugin — registered through `inventory` like every other resource
  (the builtin factory-table entry is removed), shipping a
  `plugin.json` v2 manifest embedded via a new `internal` source in
  `official-plugins.json`, and listed by `loom plugin list` as an
  Official plugin (v1.0.0) instead of `builtin`. Assembly output is
  golden-test equivalent to the previous builtin path (baseline hash
  unchanged across the migration).
- Docs: `docs/context-layer/plugin-guide.md` is now the single
  specification anchor for context layer plugins — five-face
  consistency criteria (manifest, registration, config, listing,
  documentation), exemption terms with an exemption ledger, and the
  memory plugin documented as the first normalized internal plugin
  (including D-D3 conflict error semantics for the memory instance).
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

- Ecosystem integration: `plugin-memory` renamed to
  `plugin-context-memory` (crate, manifest id and all references);
  `official-plugins.json` now lists every plugin as internal with the
  external array retired empty, and `build.rs` guards "at least one
  source overall" instead of requiring an external source. Golden
  baselines stay byte-identical across the source switch.

## [0.1.1] - 2026-07-30

- Add repository guardrails for protected release branches.
- Harden release packaging and GitHub Pages deployment.
- Fix Windows runtime packaging and download metadata.
