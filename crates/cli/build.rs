use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
struct OfficialPluginSource {
    /// Env vars (in priority order) that may point at a local checkout
    /// of the source repo. The first non-empty value that passes the
    /// anchor check wins; a non-empty value failing it panics.
    dir_env: &'static [&'static str],
    /// Repository directory name, used both for sibling-directory lookup
    /// and as the temp clone directory name under OUT_DIR.
    repo_dir_name: &'static str,
    /// Repo-relative path that must exist for a candidate directory to be
    /// accepted as a valid checkout of this source repo (identity anchor).
    ///
    /// This is a validity check only — it never filters which skills or
    /// resources are loaded; content discovery scans the whole repo (see
    /// `scan_and_register_skills`). A missing anchor panics the build
    /// (fail-loud by design, guarding against wrong-dir or drifted
    /// repo layouts).
    repo_anchor_rel: &'static str,
    /// Env var overriding the repo URL used when cloning is required.
    repo_env: &'static str,
    /// Repo URL cloned when no `repo_env` override is set and no local
    /// checkout is found by env or sibling lookup.
    default_repo_url: &'static str,
    /// Env var for an optional git ref (branch/tag) used when cloning.
    ref_env: &'static str,
}

/// Unified plugin entry list. Pure skill repos (loom-skills, actor-circuit)
/// have no `plugin.json` and fall back to global-scope skill loading.
/// Full plugin repos (loom-plugin-context-tier) carry `plugin.json` for
/// multi-dimensional dispatch.
const OFFICIAL_PLUGINS: &[OfficialPluginSource] = &[
    OfficialPluginSource {
        dir_env: &["LOOM_SKILLS_DIR", "LOOM_SKILL_DIR"],
        repo_dir_name: "loom-skills",
        repo_anchor_rel: "skills/loom/SKILL.md",
        repo_env: "LOOM_SKILLS_REPO",
        default_repo_url: "https://github.com/wyw-ai/skills.git",
        ref_env: "LOOM_SKILLS_REF",
    },
    OfficialPluginSource {
        dir_env: &["LOOM_ACTOR_CIRCUIT_DIR"],
        repo_dir_name: "actor-circuit",
        repo_anchor_rel: "skills/actor-circuit/SKILL.md",
        repo_env: "LOOM_ACTOR_CIRCUIT_REPO",
        default_repo_url: "https://github.com/wyw-ai/actor-circuit.git",
        ref_env: "LOOM_ACTOR_CIRCUIT_REF",
    },
    OfficialPluginSource {
        dir_env: &["LOOM_CONTEXT_TIER_DIR"],
        repo_dir_name: "loom-plugin-context-tier",
        repo_anchor_rel: "skills/context-tier/SKILL.md",
        repo_env: "LOOM_CONTEXT_TIER_REPO",
        default_repo_url: "https://github.com/wyw-ai/loom-plugin-context-tier.git",
        ref_env: "LOOM_CONTEXT_TIER_REF",
    },
];

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
    let plugin_sources = resolve_plugin_sources(&clone_root);

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

fn resolve_plugin_sources(clone_root: &Path) -> Vec<ResolvedPluginSource> {
    OFFICIAL_PLUGINS
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

fn content_candidates(repo_name: &str) -> Vec<PathBuf> {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli is inside the workspace root");
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
// plugin.json parsing (build-time, no serde dependency — manual JSON walk)
// ---------------------------------------------------------------------------

/// Parsed `plugin.json` manifest from a plugin repository.
struct PluginManifest {
    skills: Vec<DeclaredSkill>,
    resources: Vec<DeclaredResource>,
}

struct DeclaredSkill {
    id: String,
    path: String,
    scope: PluginScope,
}

struct DeclaredResource {
    scheme: String,
    priority: Option<i32>,
    scope: PluginScope,
}

/// Scope classification for build-time dispatch.
/// - `Global` → project_builtin_skill_targets() / default_agent_context_spec()
/// - `Scope` → scope-level skill targets / AgentContextSpec overlay
/// - `ActorBundle` → actor_bundle_skill_targets() / actor-specific agentcontext.json
#[derive(Clone, Copy, PartialEq, Eq)]
enum PluginScope {
    Global,
    Scope,
    ActorBundle,
}

fn parse_plugin_json(path: &Path) -> PluginManifest {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read plugin.json {} failed: {err}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("parse plugin.json {} failed: {err}", path.display()));

    let mut skills = Vec::new();
    if let Some(arr) = json.get("skills").and_then(|v| v.as_array()) {
        for entry in arr {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("plugin.json skill missing id: {}", path.display()))
                .to_owned();
            let skill_path = entry
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("plugin.json skill `{id}` missing path"))
                .to_owned();
            let scope = parse_scope(entry.get("scope"), &id, path);
            skills.push(DeclaredSkill {
                id,
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
                .unwrap_or_else(|| panic!("plugin.json context_resource missing scheme"))
                .to_owned();
            let priority = entry.get("priority").and_then(|v| v.as_i64()).map(|n| n as i32);
            let scope = parse_scope(entry.get("scope"), &scheme, path);
            resources.push(DeclaredResource {
                scheme,
                priority,
                scope,
            });
        }
    }

    PluginManifest { skills, resources }
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

/// A resource declared in plugin.json, tagged with its dispatch scope.
struct PluginResourceEntry {
    scheme: String,
    priority: Option<i32>,
    scope: PluginScope,
}

fn generate_plugin_snapshot(sources: &[ResolvedPluginSource], out_dir: &Path) {
    let mut all_skills: Vec<PluginSkillEntry> = Vec::new();
    let mut all_resources: Vec<PluginResourceEntry> = Vec::new();

    for source in sources {
        let content = &source.content;
        let plugin_json_path = content.path.join("plugin.json");

        if plugin_json_path.is_file() {
            // Full plugin: parse plugin.json for scope dispatch
            if !content.cloned {
                println!("cargo:rerun-if-changed={}", plugin_json_path.display());
            }
            let manifest = parse_plugin_json(&plugin_json_path);

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

            // Collect resources declared in plugin.json
            for decl in &manifest.resources {
                all_resources.push(PluginResourceEntry {
                    scheme: decl.scheme.clone(),
                    priority: decl.priority,
                    scope: decl.scope,
                });
            }

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
    generated.push_str("\nconst EMBEDDED_GLOBAL_RESOURCES: &[EmbeddedPluginResource] = &[\n");
    for r in all_resources.iter().filter(|r| r.scope == PluginScope::Global) {
        let prio = match r.priority {
            Some(p) => format!("Some({p})"),
            None => "None".to_string(),
        };
        writeln!(
            generated,
            "    EmbeddedPluginResource {{ scheme: {:?}, priority: {prio} }},",
            r.scheme
        )
        .expect("write resource snapshot");
    }
    generated.push_str("];\n");

    generated.push_str("#[allow(dead_code)]\nconst EMBEDDED_SCOPE_RESOURCES: &[EmbeddedPluginResource] = &[\n");
    for r in all_resources.iter().filter(|r| r.scope == PluginScope::Scope) {
        let prio = match r.priority {
            Some(p) => format!("Some({p})"),
            None => "None".to_string(),
        };
        writeln!(
            generated,
            "    EmbeddedPluginResource {{ scheme: {:?}, priority: {prio} }},",
            r.scheme
        )
        .expect("write resource snapshot");
    }
    generated.push_str("];\n");

    generated.push_str("#[allow(dead_code)]\nconst EMBEDDED_BUNDLE_RESOURCES: &[EmbeddedPluginResource] = &[\n");
    for r in all_resources.iter().filter(|r| r.scope == PluginScope::ActorBundle) {
        let prio = match r.priority {
            Some(p) => format!("Some({p})"),
            None => "None".to_string(),
        };
        writeln!(
            generated,
            "    EmbeddedPluginResource {{ scheme: {:?}, priority: {prio} }},",
            r.scheme
        )
        .expect("write resource snapshot");
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
    let mut name = String::from("EMBEDDED_SKILL_FILES_");
    for ch in skill_id.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch.to_ascii_uppercase());
        } else {
            name.push('_');
        }
    }
    name
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
