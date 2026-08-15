//! Shared `plugin.json` manifest parsing (build-time, no serde derive —
//! manual JSON walk). This module is compiled twice:
//!
//! - into `build.rs` (via `#[path]` mod) to generate the embedded snapshot
//! - into the CLI lib test harness (via `#[path]` mod) for schema tests
//!
//! Schema versions (see docs/context-layer/plugin-guide.md):
//! - v1 (`loom-plugin/v1`): read with normalization — the removed fields
//!   `context_resources[].registration`, `context_resources[].crate` and
//!   `context_resources[].scope` are accepted and dropped.
//! - v2 (`loom-plugin/v2`): the removed fields above are REJECTED
//!   (D-C3: resource scope dispatch was never implemented; registration
//!   and crate names are host-side implementation details).
//!
//! All errors panic with a message that includes the manifest path —
//! build inputs fail loud by design.

use std::path::Path;

/// Parsed `plugin.json` manifest, normalized to the v2 in-memory shape
/// regardless of input version.
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub loom_version: String,
    pub layer: String,
    /// `executable` is a reserved (M5) field: schema-validated here but
    /// not loaded. Presence is surfaced for `--verbose` display only.
    pub has_executable: bool,
    pub skills: Vec<DeclaredSkill>,
    pub resources: Vec<DeclaredResource>,
}

pub struct DeclaredSkill {
    pub id: String,
    pub path: String,
    pub scope: PluginScope,
}

pub struct DeclaredResource {
    pub scheme: String,
    pub priority: Option<i32>,
    /// Shallow key set of `config_schema` — the config fields this plugin
    /// declares as accepted (validation + `--verbose` display).
    pub config_keys: Vec<String>,
}

/// Scope classification for build-time skill dispatch.
/// - `Global` → project_builtin_skill_targets() / default_agent_context_spec()
/// - `Scope` → scope-level skill targets / AgentContextSpec overlay
/// - `ActorBundle` → actor_bundle_skill_targets() / actor-specific agentcontext.json
///
/// Only skills carry scope (D-C3: `context_resources[].scope` was removed —
/// resource scope dispatch was never implemented).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PluginScope {
    Global,
    Scope,
    ActorBundle,
}

/// The only layer value accepted in S1. S2 may extend the enum.
pub const PLUGIN_LAYER_CONTEXT: &str = "context";

/// The only `executable.protocol` value accepted in S1 (reserved field).
pub const PLUGIN_EXECUTABLE_PROTOCOL_JSONRPC_STDIO: &str = "jsonrpc-stdio";

pub fn parse_plugin_json(path: &Path) -> PluginManifest {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read plugin.json {} failed: {err}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("parse plugin.json {} failed: {err}", path.display()));

    let schema_version = parse_schema_version(&json, path);

    let id = required_str(&json, "id", path).to_owned();
    let name = required_str(&json, "name", path).to_owned();
    let version = required_str(&json, "version", path).to_owned();
    let loom_version = required_str(&json, "loom_version", path).to_owned();
    let layer = required_str(&json, "layer", path).to_owned();
    if layer != PLUGIN_LAYER_CONTEXT {
        panic!(
            "plugin.json `{id}` declares unsupported layer `{layer}` in {} \
             (S1 accepts only `{PLUGIN_LAYER_CONTEXT}`)",
            path.display()
        );
    }

    let has_executable = validate_executable(&json, &id, path);

    let mut skills = Vec::new();
    if let Some(arr) = json.get("skills").and_then(|v| v.as_array()) {
        for entry in arr {
            let skill_id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("plugin.json skill missing id: {}", path.display()))
                .to_owned();
            let skill_path = entry
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    panic!("plugin.json skill `{skill_id}` missing path: {}", path.display())
                })
                .to_owned();
            let scope = parse_scope(entry.get("scope"), &skill_id, path);
            skills.push(DeclaredSkill {
                id: skill_id,
                path: skill_path,
                scope,
            });
        }
    }

    let mut resources = Vec::new();
    if let Some(arr) = json.get("context_resources").and_then(|v| v.as_array()) {
        for entry in arr {
            let scheme = entry
                .get("scheme")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    panic!("plugin.json context_resource missing scheme: {}", path.display())
                })
                .to_owned();
            let priority = entry.get("priority").and_then(|v| v.as_i64()).map(|n| n as i32);

            if schema_version == 2 {
                for removed in ["registration", "crate", "scope"] {
                    if entry.get(removed).is_some() {
                        panic!(
                            "plugin.json v2 context_resource `{scheme}` still declares \
                             removed field `{removed}` in {} (D-C3: dropped in v2; \
                             registration and crate are host-side details, resource \
                             scope dispatch was never implemented)",
                            path.display()
                        );
                    }
                }
            }
            // v1 normalization: registration/crate/scope are silently dropped.

            let config_keys = entry
                .get("config_schema")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    let mut keys: Vec<String> =
                        obj.keys().map(|key| key.to_string()).collect();
                    keys.sort();
                    keys
                })
                .unwrap_or_default();

            resources.push(DeclaredResource {
                scheme,
                priority,
                config_keys,
            });
        }
    }

    PluginManifest {
        id,
        name,
        version,
        loom_version,
        layer,
        has_executable,
        skills,
        resources,
    }
}

fn parse_schema_version(json: &serde_json::Value, path: &Path) -> u8 {
    let schema = json
        .get("$schema")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!(
                "plugin.json {} missing `$schema` (expected \
                 `loom-plugin/v1` or `loom-plugin/v2`)",
                path.display()
            )
        });
    match schema {
        "loom-plugin/v1" => 1,
        "loom-plugin/v2" => 2,
        other => panic!(
            "plugin.json {} declares unknown `$schema` `{other}` \
             (expected `loom-plugin/v1` or `loom-plugin/v2`)",
            path.display()
        ),
    }
}

fn required_str<'a>(json: &'a serde_json::Value, field: &str, path: &Path) -> &'a str {
    json.get(field)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!("plugin.json {} missing required string field `{field}`", path.display())
        })
}

fn validate_executable(json: &serde_json::Value, plugin_id: &str, path: &Path) -> bool {
    let Some(executable) = json.get("executable") else {
        return false;
    };
    if !executable.is_object() {
        panic!(
            "plugin.json `{plugin_id}` executable must be an object in {}",
            path.display()
        );
    }
    if executable.get("command").and_then(|v| v.as_str()).is_none() {
        panic!(
            "plugin.json `{plugin_id}` executable missing string `command` in {}",
            path.display()
        );
    }
    if let Some(args) = executable.get("args") {
        if !args.is_array()
            || !args
                .as_array()
                .expect("checked is_array")
                .iter()
                .all(|v| v.is_string())
        {
            panic!(
                "plugin.json `{plugin_id}` executable `args` must be an array of \
                 strings in {}",
                path.display()
            );
        }
    }
    match executable.get("protocol").and_then(|v| v.as_str()) {
        Some(PLUGIN_EXECUTABLE_PROTOCOL_JSONRPC_STDIO) => {}
        Some(other) => panic!(
            "plugin.json `{plugin_id}` executable protocol `{other}` is not a \
             legal value in {} (S1 reserves only \
             `{PLUGIN_EXECUTABLE_PROTOCOL_JSONRPC_STDIO}`)",
            path.display()
        ),
        None => panic!(
            "plugin.json `{plugin_id}` executable missing `protocol` in {} \
             (S1 reserves only `{PLUGIN_EXECUTABLE_PROTOCOL_JSONRPC_STDIO}`)",
            path.display()
        ),
    }
    true
}

fn parse_scope(raw: Option<&serde_json::Value>, label: &str, path: &Path) -> PluginScope {
    match raw.and_then(|v| v.as_str()) {
        Some("global") => PluginScope::Global,
        Some("scope") => PluginScope::Scope,
        Some("actor-bundle") => PluginScope::ActorBundle,
        Some(other) => panic!(
            "plugin.json `{label}` has unknown scope `{other}` in {}",
            path.display()
        ),
        None => PluginScope::Global, // default
    }
}

/// Build-time compatibility check of `loom_version` against the current
/// Loom version. Only the `>=X.Y.Z` range form is supported (the sole
/// form used by real manifests); anything else fails loud.
pub fn check_loom_version(range: &str, current: &str, plugin_id: &str) {
    let Some(min) = range.strip_prefix(">=") else {
        panic!(
            "plugin.json `{plugin_id}` declares unsupported loom_version range \
             `{range}` (only `>=X.Y.Z` is supported)"
        );
    };
    let required = parse_version_triple(min, plugin_id);
    let actual = parse_version_triple(current, plugin_id);
    if actual < required {
        panic!(
            "plugin.json `{plugin_id}` requires loom_version {range} but this \
             build is {current}"
        );
    }
}

/// Parse `X.Y.Z` with an optional pre-release/build suffix (the suffix is
/// ignored for comparison; numeric triple only).
fn parse_version_triple(raw: &str, plugin_id: &str) -> (u64, u64, u64) {
    let numeric = raw.split(['-', '+']).next().unwrap_or(raw);
    let parts: Vec<u64> = numeric
        .split('.')
        .map(|part| {
            part.parse::<u64>().unwrap_or_else(|_| {
                panic!(
                    "plugin.json `{plugin_id}` has malformed version component \
                     `{part}` in `{raw}`"
                )
            })
        })
        .collect();
    if parts.len() != 3 {
        panic!(
            "plugin.json `{plugin_id}` has malformed version `{raw}` \
             (expected X.Y.Z)"
        );
    }
    (parts[0], parts[1], parts[2])
}
