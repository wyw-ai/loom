//! `loom plugin` — unified entry for context layer plugin introspection.
//!
//! S1 scope: `list` — the exact projection of actual assembly
//! participants. Every row is backed by a real supply path (embedded
//! manifest + inventory registration, inventory-only external
//! registration, or a builtin factory entry). Manifest schemes with no
//! backing registration are ghosts and fail loud instead of being shown
//! as available.

use std::collections::HashMap;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::cmd::agent_serve::{builtin_factory_table_overview, EMBEDDED_PLUGIN_MANIFESTS};
use crate::render;
use agent_runtime::discover_plugins;

/// Where a listed plugin's supply actually comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum PluginSource {
    /// Declared in an embedded official manifest and registered via
    /// inventory (or a builtin factory entry surfaced as official).
    Official,
    /// Registered via inventory only — not part of the official set.
    External,
    /// Produced by loom's own builtin factory table (memory, file);
    /// displayed with source `official` and version `builtin`.
    Builtin,
}

impl PluginSource {
    fn label(self) -> &'static str {
        match self {
            PluginSource::Official | PluginSource::Builtin => "official",
            PluginSource::External => "external",
        }
    }
}

/// One `loom plugin list` row: a scheme that can actually supply context
/// sections at assembly time, with its provenance.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PluginListEntry {
    scheme: String,
    source: PluginSource,
    priority: i32,
    version: String,
    plugin_id: Option<String>,
    plugin_name: Option<&'static str>,
    config_keys: Vec<&'static str>,
    registered_via: &'static str,
    endpoint: &'static str,
}

/// Manifest-declared schemes that have neither an inventory registration
/// nor a builtin factory entry. Such schemes would be silently skipped at
/// assembly time (unknown-scheme warning), so `list` refuses to show them
/// as available.
fn detect_ghosts(manifest_schemes: &[&'static str], known: &HashMap<String, i32>) -> Vec<&'static str> {
    manifest_schemes
        .iter()
        .copied()
        .filter(|scheme| !known.contains_key(*scheme))
        .collect()
}

/// Collect the exact set of assembly participants for `loom plugin list`.
///
/// Union of three disjoint groups, sorted by ascending priority:
/// - builtin factory entries not covered by inventory (memory, file)
/// - embedded-manifest official plugins with declared resources
/// - inventory registrations not covered by the manifest (external)
pub(crate) fn collect_plugin_entries() -> Result<Vec<PluginListEntry>> {
    let inventory = discover_plugins();
    // (scheme, priority) for everything the factory table can build; the
    // table merges inventory registrations with the builtin entries.
    let factory_table: HashMap<String, i32> = builtin_factory_table_overview().into_iter().collect();

    let mut entries: Vec<PluginListEntry> = Vec::new();

    // Builtin: factory-table schemes that are not inventory registrations.
    for (scheme, priority) in &factory_table {
        if inventory.contains_key(scheme) {
            continue;
        }
        entries.push(PluginListEntry {
            scheme: scheme.clone(),
            source: PluginSource::Builtin,
            priority: *priority,
            version: "builtin".to_string(),
            plugin_id: None,
            plugin_name: None,
            config_keys: Vec::new(),
            registered_via: "builtin factory table",
            endpoint: "in-process factory",
        });
    }

    // Official: embedded manifest entries with declared resources.
    let mut manifest_schemes: Vec<&'static str> = Vec::new();
    for manifest in EMBEDDED_PLUGIN_MANIFESTS {
        if manifest.resources.is_empty() {
            continue; // pure skill source, not a resource plugin
        }
        for resource in manifest.resources {
            manifest_schemes.push(resource.scheme);
            let priority = resource
                .priority
                .or_else(|| factory_table.get(resource.scheme).copied())
                .unwrap_or(100);
            entries.push(PluginListEntry {
                scheme: resource.scheme.to_string(),
                source: PluginSource::Official,
                priority,
                version: resource.version.to_string(),
                plugin_id: Some(resource.plugin_id.to_string()),
                plugin_name: Some(manifest.name),
                config_keys: resource.config_keys.iter().copied().collect(),
                registered_via: if inventory.contains_key(resource.scheme) {
                    "inventory"
                } else {
                    "manifest only"
                },
                endpoint: if manifest.has_executable {
                    "executable reserved (not implemented); in-process factory"
                } else {
                    "in-process factory"
                },
            });
        }
    }

    let ghosts = detect_ghosts(&manifest_schemes, &factory_table);
    if !ghosts.is_empty() {
        bail!(
            "plugin scheme(s) declared in embedded manifests but not registered: {} \
             (expected via inventory or builtin factory; refusing to list them as available)",
            ghosts.join(", ")
        );
    }

    // External: inventory registrations not covered by the manifest.
    for scheme in inventory.keys() {
        if manifest_schemes.contains(&scheme.as_str()) {
            continue;
        }
        let priority = factory_table.get(scheme).copied().unwrap_or(100);
        entries.push(PluginListEntry {
            scheme: scheme.clone(),
            source: PluginSource::External,
            priority,
            version: "unknown".to_string(),
            plugin_id: None,
            plugin_name: None,
            config_keys: Vec::new(),
            registered_via: "inventory",
            endpoint: "in-process factory",
        });
    }

    entries.sort_by(|a, b| (a.priority, &a.scheme).cmp(&(b.priority, &b.scheme)));
    Ok(entries)
}

/// `loom plugin list [--verbose] [--json]`.
pub fn list(verbose: bool, json: bool) -> Result<()> {
    let entries = collect_plugin_entries()?;
    if json || render::is_json() {
        render::print_json(&entries);
        return Ok(());
    }
    if verbose {
        print_verbose(&entries);
        return Ok(());
    }
    print_table(&entries);
    Ok(())
}

fn print_table(entries: &[PluginListEntry]) {
    let scheme_w = entries
        .iter()
        .map(|e| e.scheme.len())
        .chain(std::iter::once("SCHEME".len()))
        .max()
        .unwrap_or(6);
    let source_w = entries
        .iter()
        .map(|e| e.source.label().len())
        .chain(std::iter::once("SOURCE".len()))
        .max()
        .unwrap_or(6);
    println!(
        "{:<scheme_w$}  {:<source_w$}  {:>8}  {}",
        "SCHEME", "SOURCE", "PRIORITY", "VERSION"
    );
    for entry in entries {
        println!(
            "{:<scheme_w$}  {:<source_w$}  {:>8}  {}",
            entry.scheme,
            entry.source.label(),
            entry.priority,
            entry.version
        );
    }
}

fn print_verbose(entries: &[PluginListEntry]) {
    for (idx, entry) in entries.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        println!("{}:", entry.scheme);
        println!("  source:      {}", entry.source.label());
        println!("  version:     {}", entry.version);
        println!("  priority:    {}", entry.priority);
        if let (Some(name), Some(id)) = (entry.plugin_name, entry.plugin_id.as_deref()) {
            println!("  plugin:      {name} ({id})");
        }
        if !entry.config_keys.is_empty() {
            println!("  config keys: {}", entry.config_keys.join(", "));
        }
        println!("  registered:  {}", entry.registered_via);
        println!("  endpoint:    {}", entry.endpoint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reconciliation: `loom plugin list` output must equal the union of
    /// actual assembly participants — embedded manifest resource schemes,
    /// inventory registrations, and builtin factory entries — in both
    /// directions, with no ghosts.
    #[test]
    fn plugin_list_matches_assembly_participants() {
        let entries = collect_plugin_entries().expect("collect plugin entries");
        let listed: Vec<&str> = entries.iter().map(|e| e.scheme.as_str()).collect();

        let inventory: Vec<String> = discover_plugins().keys().cloned().collect();
        let factory: Vec<String> = builtin_factory_table_overview()
            .into_iter()
            .map(|(scheme, _)| scheme)
            .collect();
        let manifest: Vec<&'static str> = EMBEDDED_PLUGIN_MANIFESTS
            .iter()
            .filter(|m| !m.resources.is_empty())
            .flat_map(|m| m.resources.iter().map(|r| r.scheme))
            .collect();

        let listed_set: std::collections::BTreeSet<String> =
            listed.iter().map(|s| s.to_string()).collect();
        let mut expected_set: std::collections::BTreeSet<String> =
            manifest.iter().map(|s| s.to_string()).collect();
        for scheme in inventory.iter().chain(factory.iter()) {
            expected_set.insert(scheme.clone());
        }

        let missing: Vec<&String> = expected_set.difference(&listed_set).collect();
        let extra: Vec<&String> = listed_set.difference(&expected_set).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "plugin list mismatch: missing={missing:?} extra={extra:?}"
        );

        // Ghost detection must be empty for the shipped configuration.
        let factory_map: HashMap<String, i32> =
            builtin_factory_table_overview().into_iter().collect();
        assert!(detect_ghosts(&manifest, &factory_map).is_empty());

        // Normalization invariant: the per-resource version and plugin id
        // embedded by build.rs pass the manifest values through verbatim.
        for m in EMBEDDED_PLUGIN_MANIFESTS {
            for r in m.resources {
                assert_eq!(r.version, m.version, "resource version diverged from manifest");
                assert_eq!(r.plugin_id, m.id, "resource plugin id diverged from manifest");
            }
        }

        // Sanity: the known participants are present with expected sources.
        // R1c final state: memory carries an embedded plugin.json v2
        // manifest (official-plugins.json internal source), so it
        // classifies as Official — same spec as every other context
        // plugin, no builtin-table privilege left.
        for (scheme, source) in [
            ("memory", PluginSource::Official),
            ("file", PluginSource::Builtin),
            ("warm-summary", PluginSource::Official),
            ("message-list", PluginSource::Official),
        ] {
            let entry = entries
                .iter()
                .find(|e| e.scheme == scheme)
                .unwrap_or_else(|| panic!("expected scheme {scheme} in plugin list"));
            assert_eq!(entry.source, source, "unexpected source for {scheme}");
        }
    }

    #[test]
    fn ghost_detection_flags_unbacked_manifest_schemes() {
        let mut known = HashMap::new();
        known.insert("memory".to_string(), 5);
        let ghosts = detect_ghosts(&["memory", "ghost-scheme"], &known);
        assert_eq!(ghosts, vec!["ghost-scheme"]);
    }
}
