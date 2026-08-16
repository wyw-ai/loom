use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// Shared plugin.json parser (also compiled into the CLI lib test harness
// for schema tests). See plugin_manifest.rs for the v1/v2 contract.
#[path = "plugin_manifest.rs"]
mod plugin_manifest;

use plugin_manifest::{check_loom_version, parse_plugin_json, PluginScope};

const GUIDE_REPO_URL: &str = "https://github.com/wyw-ai/loom-guide.git";

/// One official plugin content source repository. A plugin may carry a
/// `plugin.json` manifest declaring skill scope (global/scope/actor-bundle)
/// and context resource declarations. Sources without `plugin.json` are
/// treated as pure skill repositories with all skills at global scope
/// (backward compatible with the former `OFFICIAL_SKILL_SOURCES`).
///
/// Source resolution lifecycle (see `resolve_content_source`):
/// 1. env override — first non-empty `dir_env` var wins (anchor-checked)
/// 2. sibling lookup — `<workspace-parent>/<repo_dir_name>`, then
///    `<workspace>/<repo_dir_name>` (anchor-checked)
/// 3. clone — `repo_env` or `default_repo_url` (+ optional `ref_env`)
///    into OUT_DIR (anchor-checked after clone)
/// Every stage validates the same `repo_anchor_rel`; a non-empty env var
/// or fresh clone failing the check panics the build.
///
/// This is the *external* source form. Internal workspace plugins (see
/// `InternalPluginSource`) live inside this repository and are resolved
/// directly, never through env/sibling/clone.
struct OfficialPluginSource {
    /// Stable identity of this source (used in diagnostics and by
    /// `loom plugin list`); must be unique across the data file.
    // Validated for uniqueness at load time; the field itself is carried for
    // diagnostics and future surfacing, not read by the build script.
    #[allow(dead_code)]
    id: &'static str,
    /// Env vars (in priority order) that may point at a local checkout
    /// of the source repo. The first non-empty value that passes the
    /// anchor check wins; a non-empty value failing it panics.
    dir_env: &'static [&'static str],
    /// Repository directory name, used both for sibling-directory lookup
    /// and as the temp clone directory name under OUT_DIR.
    repo_dir_name: &'static str,
    /// Repo-relative path that must exist for a candidate directory to be
    /// accepted as a valid checkout of this source repo (structural anchor).
    ///
    /// (a) The anchor points at the repo's defining layout (e.g. `skills`),
    /// (b) never at an individual content file, and
    /// (c) serves as a validity check only — it never filters which skills
    ///     or resources are loaded; content discovery scans the whole repo
    ///     (see `scan_and_register_skills`).
    /// (d) A missing anchor panics the build (fail-loud by design, guarding
    ///     against wrong-dir or drifted repo layouts).
    repo_anchor_rel: &'static str,
    /// Env var overriding the repo URL used when cloning is required.
    repo_env: &'static str,
    /// Repo URL cloned when no `repo_env` override is set and no local
    /// checkout is found by env or sibling lookup.
    default_repo_url: &'static str,
    /// Env var for an optional git ref (branch/tag) used when cloning.
    ref_env: &'static str,
}

/// One internal plugin source: a crate inside this workspace whose
/// `plugin.json` is embedded directly (R1 rectification — the memory
/// plugin follows the same manifest spec as every other context
/// plugin). No env override, no sibling lookup, no clone: the path is
/// workspace-root-relative and a missing `plugin.json` fails the build.
struct InternalPluginSource {
    /// Stable identity of this plugin (must be unique across external
    /// and internal sources; validated at load time).
    id: &'static str,
    /// Workspace-root-relative path to the plugin crate directory
    /// (e.g. `crates/plugin-context-memory`).
    path: &'static str,
    /// Diagnostic identity used in skill/resource source attribution
    /// (`internal/<id>`), parallel to the external `repo_dir_name`.
    repo_dir_name: &'static str,
}

/// Parsed `official-plugins.json`: external repo sources plus internal
/// workspace plugin sources.
struct OfficialPluginData {
    external: &'static [OfficialPluginSource],
    internal: &'static [InternalPluginSource],
}

/// Unified plugin entry list, data-driven from `official-plugins.json`
/// (next to this build script). The file accepts two shapes:
///
/// - legacy: a top-level JSON array (external sources only, no
///   internal plugins);
/// - object: `{"external": [...], "internal": [{"id", "path"}, ...]}`
///   (both arrays optional; at least one source overall required).
///
/// Pure skill sources (loom-skills, actor-circuit) carry a skills-only
/// `plugin.json` (layer "skill", no context_resources). Internal
/// sources MUST carry `plugin.json` — a resource-only manifest with
/// no skills is valid. Adding an official plugin = editing the JSON +
/// providing the content; no build.rs change.
///
/// Loaded by `load_official_plugins`; any missing/invalid field, empty
/// external list, or duplicate id fails the build.
fn load_official_plugins() -> OfficialPluginData {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("official-plugins.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let raw = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", manifest_path.display()));
    let value: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("parse {} failed: {err}", manifest_path.display()));
    let (array, internal_array): (Vec<serde_json::Value>, Vec<serde_json::Value>) = match value {
        serde_json::Value::Array(entries) => (entries, Vec::new()),
        serde_json::Value::Object(map) => {
            let external = map
                .get("external")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let internal = map
                .get("internal")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            (external, internal)
        }
        _ => panic!(
            "{} must be a JSON array of plugin source objects, or an \
             object with `external`/`internal` arrays",
            manifest_path.display()
        ),
    };
    if array.is_empty() && internal_array.is_empty() {
        panic!(
            "{} must declare at least one plugin source (external or internal)",
            manifest_path.display()
        );
    }

    let mut sources = Vec::with_capacity(array.len());
    let mut seen_ids: Vec<&str> = Vec::with_capacity(array.len() + internal_array.len());
    for (index, entry) in array.iter().enumerate() {
        let object = entry.as_object().unwrap_or_else(|| {
            panic!(
                "{} entry [{index}] must be an object",
                manifest_path.display()
            )
        });
        let id = required_str(&manifest_path, index, object, "id");
        if seen_ids.contains(&id) {
            panic!(
                "{} entry [{index}] duplicates plugin source id `{id}`",
                manifest_path.display()
            );
        }
        seen_ids.push(id);

        let dir_env_raw = required_str_array(&manifest_path, index, object, "dir_env");
        if dir_env_raw.is_empty() {
            panic!(
                "{} entry [{index}] (`{id}`) must list at least one dir_env",
                manifest_path.display()
            );
        }

        sources.push(OfficialPluginSource {
            id: Box::leak(id.to_owned().into_boxed_str()),
            dir_env: Box::leak(
                dir_env_raw
                    .into_iter()
                    .map(|s| Box::leak(s.to_owned().into_boxed_str()) as &'static str)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            repo_dir_name: Box::leak(
                required_str(&manifest_path, index, object, "repo_dir_name")
                    .to_owned()
                    .into_boxed_str(),
            ),
            repo_anchor_rel: Box::leak(
                required_str(&manifest_path, index, object, "repo_anchor_rel")
                    .to_owned()
                    .into_boxed_str(),
            ),
            repo_env: Box::leak(
                required_str(&manifest_path, index, object, "repo_env")
                    .to_owned()
                    .into_boxed_str(),
            ),
            default_repo_url: Box::leak(
                required_str(&manifest_path, index, object, "default_repo_url")
                    .to_owned()
                    .into_boxed_str(),
            ),
            ref_env: Box::leak(
                required_str(&manifest_path, index, object, "ref_env")
                    .to_owned()
                    .into_boxed_str(),
            ),
        });
    }
    let mut internal = Vec::with_capacity(internal_array.len());
    for (index, entry) in internal_array.iter().enumerate() {
        let object = entry.as_object().unwrap_or_else(|| {
            panic!(
                "{} internal entry [{index}] must be an object",
                manifest_path.display()
            )
        });
        let id = required_str(&manifest_path, index, object, "id");
        if seen_ids.contains(&id) {
            panic!(
                "{} internal entry [{index}] duplicates plugin source id `{id}`",
                manifest_path.display()
            );
        }
        seen_ids.push(id);
        let path = required_str(&manifest_path, index, object, "path");
        internal.push(InternalPluginSource {
            id: Box::leak(id.to_owned().into_boxed_str()),
            path: Box::leak(path.to_owned().into_boxed_str()),
            repo_dir_name: Box::leak(format!("internal/{id}").into_boxed_str()),
        });
    }

    OfficialPluginData {
        external: Box::leak(sources.into_boxed_slice()),
        internal: Box::leak(internal.into_boxed_slice()),
    }
}

fn required_str<'a>(
    manifest_path: &Path,
    index: usize,
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> &'a str {
    object.get(field).and_then(|v| v.as_str()).unwrap_or_else(|| {
        panic!(
            "{} entry [{index}] is missing required string field `{field}`",
            manifest_path.display()
        )
    })
}

fn required_str_array(
    manifest_path: &Path,
    index: usize,
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Vec<String> {
    let invalid_array = || {
        panic!(
            "{} entry [{index}] field `{field}` must be an array of non-empty strings",
            manifest_path.display()
        )
    };
    let array = object
        .get(field)
        .and_then(|v| v.as_array())
        .unwrap_or_else(invalid_array);
    let mut values = Vec::with_capacity(array.len());
    for item in array {
        let invalid_item = || {
            panic!(
                "{} entry [{index}] field `{field}` must be an array of non-empty strings",
                manifest_path.display()
            )
        };
        let s = item.as_str().unwrap_or_else(invalid_item);
        if s.is_empty() {
            invalid_item();
        }
        values.push(s.to_owned());
    }
    values
}

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let clone_root = out_dir.join("official-runtime-content");

    let guide_dir = resolve_content_source(
        &["LOOM_GUIDE_DIR"],
        "loom-guide",
        "guides",
        "LOOM_GUIDE_REPO",
        GUIDE_REPO_URL,
        "LOOM_GUIDE_REF",
        &clone_root,
    );
    let official_plugins = load_official_plugins();
    let mut plugin_sources = resolve_plugin_sources(official_plugins.external, &clone_root);
    plugin_sources.extend(resolve_internal_sources(official_plugins.internal));

    generate_guide_snapshot(&guide_dir, &out_dir);
    generate_plugin_snapshot(&plugin_sources, &out_dir);

    cleanup_temp_source(&guide_dir, &out_dir);
    for source in &plugin_sources {
        cleanup_temp_source(&source.content, &out_dir);
    }
}

struct ResolvedPluginSource {
    repo_dir_name: &'static str,
    content: ContentSource,
}

fn resolve_plugin_sources(
    sources: &'static [OfficialPluginSource],
    clone_root: &Path,
) -> Vec<ResolvedPluginSource> {
    sources
        .iter()
        .map(|source| ResolvedPluginSource {
            repo_dir_name: source.repo_dir_name,
            content: resolve_content_source(
                source.dir_env,
                source.repo_dir_name,
                source.repo_anchor_rel,
                source.repo_env,
                source.default_repo_url,
                source.ref_env,
                clone_root,
            ),
        })
        .collect()
}

/// Resolve internal plugin sources: workspace crates whose plugin.json
/// is embedded directly. No env override, no sibling lookup, no clone —
/// the declared path is workspace-root-relative and a missing
/// plugin.json fails loud (internal plugins MUST carry a manifest).
fn resolve_internal_sources(
    sources: &'static [InternalPluginSource],
) -> Vec<ResolvedPluginSource> {
    let workspace_root = workspace_root();
    sources
        .iter()
        .map(|source| {
            let dir = workspace_root.join(source.path);
            let plugin_json = dir.join("plugin.json");
            if !plugin_json.is_file() {
                panic!(
                    "internal plugin `{}` ({}): plugin.json missing at {}",
                    source.id,
                    source.path,
                    plugin_json.display()
                );
            }
            ResolvedPluginSource {
                repo_dir_name: source.repo_dir_name,
                content: ContentSource {
                    path: dir,
                    cloned: false,
                },
            }
        })
        .collect()
}

#[derive(Debug)]
struct ContentSource {
    path: PathBuf,
    cloned: bool,
}

fn resolve_content_source(
    env_names: &[&str],
    repo_name: &str,
    repo_anchor_rel: &str,
    repo_env_name: &str,
    default_repo_url: &str,
    ref_env_name: &str,
    clone_root: &Path,
) -> ContentSource {
    for env_name in env_names {
        println!("cargo:rerun-if-env-changed={env_name}");
    }
    println!("cargo:rerun-if-env-changed={repo_env_name}");
    println!("cargo:rerun-if-env-changed={ref_env_name}");

    for env_name in env_names {
        if let Some(raw) = env::var_os(env_name).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(raw);
            if path.join(repo_anchor_rel).exists() {
                println!("cargo:rerun-if-changed={}", path.display());
                return ContentSource {
                    path,
                    cloned: false,
                };
            }
            panic!(
                "{env_name} points at {}, but {} is missing",
                path.display(),
                repo_anchor_rel
            );
        }
    }

    for candidate in content_candidates(repo_name) {
        if candidate.join(repo_anchor_rel).exists() {
            println!("cargo:rerun-if-changed={}", candidate.display());
            return ContentSource {
                path: candidate,
                cloned: false,
            };
        }
    }

    let repo_url = env::var(repo_env_name).unwrap_or_else(|_| default_repo_url.to_string());
    let git_ref = env::var(ref_env_name).ok().filter(|r| !r.is_empty());
    let target = clone_root.join(repo_name);
    clone_repo(&repo_url, &target, git_ref.as_deref());
    if !target.join(repo_anchor_rel).exists() {
        panic!(
            "cloned {repo_url} into {}, but {} is missing",
            target.display(),
            repo_anchor_rel
        );
    }
    ContentSource {
        path: target,
        cloned: true,
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli is inside the workspace root")
        .to_path_buf()
}

fn content_candidates(repo_name: &str) -> Vec<PathBuf> {
    let repo_root = workspace_root();
    let parent = repo_root.parent();

    let mut candidates = Vec::new();
    if let Some(parent) = parent {
        candidates.push(parent.join(repo_name));
    }
    candidates.push(repo_root.join(repo_name));
    candidates
}

fn clone_repo(repo_url: &str, target: &Path, git_ref: Option<&str>) {
    if target.exists() {
        fs::remove_dir_all(target)
            .unwrap_or_else(|err| panic!("remove stale clone {} failed: {err}", target.display()));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|err| panic!("create clone dir {} failed: {err}", parent.display()));
    }

    let mut cmd = Command::new("git");
    cmd.args(["clone", "--depth", "1"]);
    if let Some(r) = git_ref {
        cmd.args(["--branch", r]);
    }
    cmd.arg(repo_url).arg(target);
    let status = cmd
        .status()
        .unwrap_or_else(|err| panic!("run git clone for {repo_url} failed: {err}"));
    if !status.success() {
        panic!("git clone {repo_url} {} failed: {status}", target.display());
    }
}

fn cleanup_temp_source(source: &ContentSource, out_dir: &Path) {
    if !source.cloned {
        return;
    }

    let canonical_out = fs::canonicalize(out_dir)
        .unwrap_or_else(|err| panic!("canonicalize OUT_DIR {} failed: {err}", out_dir.display()));
    let canonical_source = fs::canonicalize(&source.path).unwrap_or_else(|err| {
        panic!(
            "canonicalize temp content {} failed: {err}",
            source.path.display()
        )
    });
    if !canonical_source.starts_with(&canonical_out) {
        panic!(
            "refusing to remove temp content outside OUT_DIR: {}",
            source.path.display()
        );
    }
    fs::remove_dir_all(&source.path).unwrap_or_else(|err| {
        panic!(
            "remove temp content {} failed: {err}",
            source.path.display()
        )
    });
}

fn generate_guide_snapshot(guide_source: &ContentSource, out_dir: &Path) {
    let guides_dir = guide_source.path.join("guides");
    // Temp clones are deleted after snapshot generation; registering their
    // paths would make Cargo treat the build script as always dirty and
    // re-clone on every build.
    if !guide_source.cloned {
        println!("cargo:rerun-if-changed={}", guides_dir.display());
    }

    let mut entries = fs::read_dir(&guides_dir)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", guides_dir.display()))
        .map(|entry| entry.expect("read guide entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    entries.sort();

    if entries.is_empty() {
        panic!("no guide topics found under {}", guides_dir.display());
    }

    let mut generated = String::new();
    generated.push_str("const EMBEDDED_TOPICS: &[EmbeddedTopic] = &[\n");
    for path in entries {
        if !guide_source.cloned {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_else(|| panic!("invalid guide topic filename: {}", path.display()));
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()));
        let title = markdown_title(&content).unwrap_or_else(|| title_from_id(id));
        writeln!(
            generated,
            "    EmbeddedTopic {{ id: {id:?}, title: {title:?}, content: {content:?} }},"
        )
        .expect("write generated guide snapshot");
    }
    generated.push_str("];\n");

    fs::write(out_dir.join("loom_guide_embedded.rs"), generated)
        .expect("write generated guide snapshot");
}

// ---------------------------------------------------------------------------
// Plugin snapshot generation
// ---------------------------------------------------------------------------

/// A skill discovered in a plugin repo, tagged with its dispatch scope.
struct PluginSkillEntry {
    id: String,
    source: &'static str,
    skill_dir: PathBuf,
    files: Vec<PathBuf>,
    scope: PluginScope,
}

/// A resource declared in plugin.json, carrying its plugin identity for
/// the embedded manifest (v2: plugin_id/version/config_keys are consumed
/// by `loom plugin list` introspection).
#[derive(Clone)]
struct PluginResourceEntry {
    scheme: String,
    priority: Option<i32>,
    plugin_id: String,
    version: String,
    config_keys: Vec<String>,
}

/// A plugin-level manifest entry for the embedded manifest table.
struct PluginManifestEntry {
    id: String,
    name: String,
    version: String,
    layer: String,
    has_executable: bool,
    resources: Vec<PluginResourceEntry>,
}

fn generate_plugin_snapshot(sources: &[ResolvedPluginSource], out_dir: &Path) {
    let mut all_skills: Vec<PluginSkillEntry> = Vec::new();
    let mut all_resources: Vec<PluginResourceEntry> = Vec::new();
    let mut all_manifests: Vec<PluginManifestEntry> = Vec::new();
    // Declared context resources per source (parallel to `sources`),
    // feeding the zero-content guard below.
    let mut resource_counts: Vec<usize> = vec![0; sources.len()];

    for (source_index, source) in sources.iter().enumerate() {
        let content = &source.content;
        let plugin_json_path = content.path.join("plugin.json");

        if plugin_json_path.is_file() {
            // Full plugin: parse plugin.json for scope dispatch
            if !content.cloned {
                println!("cargo:rerun-if-changed={}", plugin_json_path.display());
            }
            let manifest = parse_plugin_json(&plugin_json_path);
            check_loom_version(
                &manifest.loom_version,
                env!("CARGO_PKG_VERSION"),
                &manifest.id,
            );

            // Collect skills declared in plugin.json
            for decl in &manifest.skills {
                let skill_dir = content.path.join(&decl.path);
                if !skill_dir.join("SKILL.md").is_file() {
                    panic!(
                        "plugin.json skill `{}` path `{}` missing SKILL.md in {}",
                        decl.id,
                        decl.path,
                        content.path.display()
                    );
                }
                register_skill(
                    &mut all_skills,
                    &decl.id,
                    source.repo_dir_name,
                    &skill_dir,
                    !content.cloned,
                    decl.scope,
                );
            }

            // Collect resources declared in plugin.json (v2 shape: no scope,
            // carrying plugin identity for introspection)
            let mut plugin_resources = Vec::new();
            for decl in &manifest.resources {
                let entry = PluginResourceEntry {
                    scheme: decl.scheme.clone(),
                    priority: decl.priority,
                    plugin_id: manifest.id.clone(),
                    version: manifest.version.clone(),
                    config_keys: decl.config_keys.clone(),
                };
                plugin_resources.push(entry);
            }
            // Post-D-C3 every declared resource is global (resource scope
            // dispatch was never implemented), so the flat list is the
            // union of all manifest resources.
            resource_counts[source_index] = plugin_resources.len();
            all_resources.extend(plugin_resources.iter().cloned());
            all_manifests.push(PluginManifestEntry {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                layer: manifest.layer.clone(),
                has_executable: manifest.has_executable,
                resources: plugin_resources,
            });

            // Also scan skills/ dir for any skills NOT listed in plugin.json
            // (treat them as global scope for backward compat within the repo)
            let skills_root = content.path.join("skills");
            if skills_root.is_dir() {
                if !content.cloned {
                    println!("cargo:rerun-if-changed={}", skills_root.display());
                }
                let declared_ids: Vec<&str> =
                    manifest.skills.iter().map(|s| s.id.as_str()).collect();
                scan_and_register_skills(
                    &mut all_skills,
                    &skills_root,
                    source.repo_dir_name,
                    !content.cloned,
                    PluginScope::Global,
                    &declared_ids,
                );
            }
        } else {
            // Pure skill repo (no plugin.json): all skills at global scope
            let skills_root = content.path.join("skills");
            if !content.cloned {
                println!("cargo:rerun-if-changed={}", skills_root.display());
            }
            scan_and_register_skills(
                &mut all_skills,
                &skills_root,
                source.repo_dir_name,
                !content.cloned,
                PluginScope::Global,
                &[],
            );
        }
    }

    // Zero-content guard: every official source must contribute at least
    // one skill OR one declared context resource. A source contributing
    // none indicates a drifted repo layout or a misconfigured anchor;
    // fail loud instead of silently embedding less. (R1 rectification:
    // resource-only plugins such as the internal memory plugin are valid
    // official sources — the former skills-only guard would reject them.)
    for (source, &resource_count) in sources.iter().zip(&resource_counts) {
        let contributed = all_skills
            .iter()
            .filter(|skill| skill.source == source.repo_dir_name)
            .count();
        if contributed == 0 && resource_count == 0 {
            panic!(
                "official plugin source `{}` contributed no skills and no \
                 context resources",
                source.repo_dir_name
            );
        }
    }

    all_skills.sort_by(|a, b| a.id.cmp(&b.id));
    all_resources.sort_by(|a, b| a.scheme.cmp(&b.scheme));

    let mut generated = String::new();

    // --- Skill ID list (all skills, for backward compat) ---
    generated.push_str("const EMBEDDED_BUILTIN_SKILL_IDS: &[&str] = &[\n");
    for skill in &all_skills {
        writeln!(generated, "    {:?},", skill.id).expect("write skill snapshot");
    }
    generated.push_str("];\n\n");

    // --- Per-skill file arrays ---
    let mut static_names = Vec::new();
    for skill in &all_skills {
        let static_name = embedded_skill_files_static_name(&skill.id);
        if static_names.contains(&static_name) {
            panic!("skill id `{}` collides in generated snapshot", skill.id);
        }
        static_names.push(static_name.clone());
        writeln!(generated, "static {static_name}: &[EmbeddedSkillFile] = &[")
            .expect("write skill snapshot");
        for path in &skill.files {
            let rel = path
                .strip_prefix(&skill.skill_dir)
                .unwrap_or_else(|err| panic!("strip skill prefix {} failed: {err}", path.display()))
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()));
            writeln!(
                generated,
                "    EmbeddedSkillFile {{ path: {rel:?}, content: {content:?} }},"
            )
            .expect("write skill snapshot");
        }
        generated.push_str("];\n\n");
    }

    // --- EMBEDDED_BUILTIN_SKILLS (backward compat: all skills as flat list) ---
    generated.push_str("static EMBEDDED_BUILTIN_SKILLS: &[EmbeddedBuiltinSkill] = &[\n");
    for skill in &all_skills {
        let static_name = embedded_skill_files_static_name(&skill.id);
        writeln!(
            generated,
            "    EmbeddedBuiltinSkill {{ id: {:?}, files: {static_name} }},",
            skill.id
        )
        .expect("write skill snapshot");
    }
    generated.push_str("];\n\n");

    // --- EMBEDDED_LOOM_SKILL_FILES (backward compat) ---
    let loom_static = all_skills
        .iter()
        .find(|s| s.id == "loom")
        .map(|s| embedded_skill_files_static_name(&s.id))
        .unwrap_or_else(|| panic!("default Loom skill missing from official plugin sources"));
    writeln!(
        generated,
        "const EMBEDDED_LOOM_SKILL_FILES: &[EmbeddedSkillFile] = {loom_static};"
    )
    .expect("write skill snapshot");

    // --- Scope-classified skill ID lists ---
    let global_ids: Vec<&str> = all_skills
        .iter()
        .filter(|s| s.scope == PluginScope::Global)
        .map(|s| s.id.as_str())
        .collect();
    let scope_ids: Vec<&str> = all_skills
        .iter()
        .filter(|s| s.scope == PluginScope::Scope)
        .map(|s| s.id.as_str())
        .collect();
    let bundle_ids: Vec<&str> = all_skills
        .iter()
        .filter(|s| s.scope == PluginScope::ActorBundle)
        .map(|s| s.id.as_str())
        .collect();

    generated.push_str("\n#[allow(dead_code)]\nconst EMBEDDED_GLOBAL_SKILL_IDS: &[&str] = &[");
    for id in &global_ids {
        write!(generated, "{id:?}, ").expect("write skill snapshot");
    }
    generated.push_str("];\n");

    generated.push_str("#[allow(dead_code)]\nconst EMBEDDED_SCOPE_SKILL_IDS: &[&str] = &[");
    for id in &scope_ids {
        write!(generated, "{id:?}, ").expect("write skill snapshot");
    }
    generated.push_str("];\n");

    generated.push_str("#[allow(dead_code)]\nconst EMBEDDED_BUNDLE_SKILL_IDS: &[&str] = &[");
    for id in &bundle_ids {
        write!(generated, "{id:?}, ").expect("write skill snapshot");
    }
    generated.push_str("];\n");

    // --- Scope-classified resource declarations ---
    // Post-D-C3 resources have no scope dimension; the flat global list is
    // authoritative. Per-plugin arrays feed the manifest table below.
    generated.push_str("\nconst EMBEDDED_GLOBAL_RESOURCES: &[EmbeddedPluginResource] = &[\n");
    for r in &all_resources {
        write_resource_entry(&mut generated, r);
    }
    generated.push_str("];\n");

    // --- Per-plugin resource arrays + manifest table (v2 introspection) ---
    all_manifests.sort_by(|a, b| a.id.cmp(&b.id));
    for manifest in &all_manifests {
        let static_name = embedded_plugin_resources_static_name(&manifest.id);
        writeln!(
            generated,
            "\n#[allow(dead_code)]\nstatic {static_name}: &[EmbeddedPluginResource] = &["
        )
        .expect("write resource snapshot");
        for r in &manifest.resources {
            write_resource_entry(&mut generated, r);
        }
        generated.push_str("];\n");
    }

    generated.push_str(
        "\npub(crate) static EMBEDDED_PLUGIN_MANIFESTS: &[EmbeddedPluginManifest] = &[\n",
    );
    for manifest in &all_manifests {
        let static_name = embedded_plugin_resources_static_name(&manifest.id);
        writeln!(
            generated,
            "    EmbeddedPluginManifest {{ id: {:?}, name: {:?}, version: {:?}, layer: {:?}, \
             has_executable: {}, resources: {static_name} }},",
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.layer,
            manifest.has_executable,
        )
        .expect("write manifest snapshot");
    }
    generated.push_str("];\n");

    fs::write(out_dir.join("loom_skill_embedded.rs"), generated)
        .expect("write generated plugin snapshot");
}

/// Scan a `skills/` directory and register each skill directory (with SKILL.md)
/// into `all_skills`, skipping any id in `skip_ids`.
fn scan_and_register_skills(
    all_skills: &mut Vec<PluginSkillEntry>,
    skills_root: &Path,
    repo_dir_name: &'static str,
    emit_rerun: bool,
    default_scope: PluginScope,
    skip_ids: &[&str],
) {
    let mut skill_dirs = fs::read_dir(skills_root)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", skills_root.display()))
        .map(|entry| entry.expect("read skill entry").path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.starts_with('.'))
        })
        .filter(|path| path.join("SKILL.md").is_file())
        .collect::<Vec<_>>();
    skill_dirs.sort();

    for skill_dir in skill_dirs {
        let id = skill_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_else(|| panic!("invalid skill directory name: {}", skill_dir.display()))
            .to_owned();
        if skip_ids.contains(&id.as_str()) {
            continue;
        }
        register_skill(
            all_skills,
            &id,
            repo_dir_name,
            &skill_dir,
            emit_rerun,
            default_scope,
        );
    }
}

/// Register a single skill, checking for duplicate ids.
fn register_skill(
    all_skills: &mut Vec<PluginSkillEntry>,
    id: &str,
    source: &'static str,
    skill_dir: &Path,
    emit_rerun: bool,
    scope: PluginScope,
) {
    if let Some(existing) = all_skills.iter().find(|s| s.id == id) {
        panic!(
            "skill id `{id}` provided by both official plugin sources `{}` and `{}`",
            existing.source, source
        );
    }
    let mut files = collect_files(skill_dir, emit_rerun);
    files.sort();
    if emit_rerun {
        for path in &files {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    all_skills.push(PluginSkillEntry {
        id: id.to_owned(),
        source,
        skill_dir: skill_dir.to_path_buf(),
        files,
        scope,
    });
}

fn embedded_skill_files_static_name(skill_id: &str) -> String {
    embedded_static_name("EMBEDDED_SKILL_FILES_", skill_id)
}

fn embedded_plugin_resources_static_name(plugin_id: &str) -> String {
    embedded_static_name("EMBEDDED_PLUGIN_RESOURCES_", plugin_id)
}

fn embedded_static_name(prefix: &str, id: &str) -> String {
    let mut name = String::from(prefix);
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch.to_ascii_uppercase());
        } else {
            name.push('_');
        }
    }
    name
}

/// Emit one `EmbeddedPluginResource` literal (v2 shape: plugin identity +
/// declared config keys ride along the scheme/priority pair).
fn write_resource_entry(generated: &mut String, r: &PluginResourceEntry) {
    let prio = match r.priority {
        Some(p) => format!("Some({p})"),
        None => "None".to_string(),
    };
    let config_keys = format!(
        "&[{}]",
        r.config_keys
            .iter()
            .map(|key| format!("{key:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    writeln!(
        generated,
        "    EmbeddedPluginResource {{ scheme: {:?}, priority: {prio}, plugin_id: {:?}, \
         version: {:?}, config_keys: {config_keys} }},",
        r.scheme, r.plugin_id, r.version,
    )
    .expect("write resource snapshot");
}

fn collect_files(dir: &Path, emit_rerun: bool) -> Vec<PathBuf> {
    if emit_rerun {
        println!("cargo:rerun-if-changed={}", dir.display());
    }
    let mut files = Vec::new();
    collect_files_inner(dir, emit_rerun, &mut files);
    files
}

fn collect_files_inner(dir: &Path, emit_rerun: bool, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()))
        .map(|entry| entry.expect("read skill entry").path())
        .collect::<Vec<_>>();
    for path in entries {
        if path.is_dir() {
            if emit_rerun {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            collect_files_inner(&path, emit_rerun, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

fn markdown_title(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn title_from_id(id: &str) -> String {
    id.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
