use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GUIDE_REPO_URL: &str = "https://github.com/wyw-ai/loom-guide.git";

/// One official skill content source repository. Adding a new official skill
/// source is a single new entry in `OFFICIAL_SKILL_SOURCES`.
struct OfficialSkillSource {
    /// Env vars pointing at a local checkout of the source repo.
    dir_env: &'static [&'static str],
    /// Sibling-directory lookup name and temp clone directory name.
    repo_dir_name: &'static str,
    /// File that must exist for the source to be considered valid.
    required_rel: &'static str,
    /// Env var overriding the repo URL to clone.
    repo_env: &'static str,
    default_repo_url: &'static str,
}

const OFFICIAL_SKILL_SOURCES: &[OfficialSkillSource] = &[
    OfficialSkillSource {
        dir_env: &["LOOM_SKILLS_DIR", "LOOM_SKILL_DIR"],
        repo_dir_name: "loom-skills",
        required_rel: "skills/loom/SKILL.md",
        repo_env: "LOOM_SKILLS_REPO",
        default_repo_url: "https://github.com/wyw-ai/skills.git",
    },
    OfficialSkillSource {
        dir_env: &["LOOM_ACTOR_CIRCUIT_DIR"],
        repo_dir_name: "actor-circuit",
        required_rel: "skills/actor-circuit/SKILL.md",
        repo_env: "LOOM_ACTOR_CIRCUIT_REPO",
        default_repo_url: "https://github.com/wyw-ai/actor-circuit.git",
    },
    OfficialSkillSource {
        dir_env: &["LOOM_CONTEXT_TIER_DIR"],
        repo_dir_name: "context-tier-skill",
        required_rel: "skills/context-tier/SKILL.md",
        repo_env: "LOOM_CONTEXT_TIER_REPO",
        default_repo_url: "https://github.com/wyw-ai/context-tier-skill.git",
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
        &clone_root,
    );
    let skill_sources = resolve_skill_sources(&clone_root);

    generate_guide_snapshot(&guide_dir, &out_dir);
    generate_skill_snapshot(&skill_sources, &out_dir);

    cleanup_temp_source(&guide_dir, &out_dir);
    for source in &skill_sources {
        cleanup_temp_source(&source.content, &out_dir);
    }
}

struct ResolvedSkillSource {
    repo_dir_name: &'static str,
    content: ContentSource,
}

fn resolve_skill_sources(clone_root: &Path) -> Vec<ResolvedSkillSource> {
    OFFICIAL_SKILL_SOURCES
        .iter()
        .map(|source| ResolvedSkillSource {
            repo_dir_name: source.repo_dir_name,
            content: resolve_content_source(
                source.dir_env,
                source.repo_dir_name,
                source.required_rel,
                source.repo_env,
                source.default_repo_url,
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
    required_rel: &str,
    repo_env_name: &str,
    default_repo_url: &str,
    clone_root: &Path,
) -> ContentSource {
    for env_name in env_names {
        println!("cargo:rerun-if-env-changed={env_name}");
    }
    println!("cargo:rerun-if-env-changed={repo_env_name}");

    for env_name in env_names {
        if let Some(raw) = env::var_os(env_name).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(raw);
            if path.join(required_rel).exists() {
                println!("cargo:rerun-if-changed={}", path.display());
                return ContentSource {
                    path,
                    cloned: false,
                };
            }
            panic!(
                "{env_name} points at {}, but {} is missing",
                path.display(),
                required_rel
            );
        }
    }

    for candidate in content_candidates(repo_name) {
        if candidate.join(required_rel).exists() {
            println!("cargo:rerun-if-changed={}", candidate.display());
            return ContentSource {
                path: candidate,
                cloned: false,
            };
        }
    }

    let repo_url = env::var(repo_env_name).unwrap_or_else(|_| default_repo_url.to_string());
    let target = clone_root.join(repo_name);
    clone_repo(&repo_url, &target);
    if !target.join(required_rel).exists() {
        panic!(
            "cloned {repo_url} into {}, but {} is missing",
            target.display(),
            required_rel
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

fn clone_repo(repo_url: &str, target: &Path) {
    if target.exists() {
        fs::remove_dir_all(target)
            .unwrap_or_else(|err| panic!("remove stale clone {} failed: {err}", target.display()));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|err| panic!("create clone dir {} failed: {err}", parent.display()));
    }

    let status = Command::new("git")
        .args(["clone", "--depth", "1", repo_url])
        .arg(target)
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

fn generate_skill_snapshot(sources: &[ResolvedSkillSource], out_dir: &Path) {
    // Snapshot every skill under each source's `skills/` tree (a directory
    // with a SKILL.md). `skills/loom/SKILL.md` is the required file of the
    // loom-skills source, so the default loom skill is always part of the
    // snapshot. A skill id provided by two sources is a release-level error
    // and fails the build.
    let mut skills: Vec<(String, &'static str, PathBuf, Vec<PathBuf>)> = Vec::new();
    for source in sources {
        let content = &source.content;
        let skills_root = content.path.join("skills");
        if !content.cloned {
            println!("cargo:rerun-if-changed={}", skills_root.display());
        }

        let mut skill_dirs = fs::read_dir(&skills_root)
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
        if skill_dirs.is_empty() {
            panic!(
                "no skills with SKILL.md found under {}",
                skills_root.display()
            );
        }

        for skill_dir in skill_dirs {
            let id = skill_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_else(|| panic!("invalid skill directory name: {}", skill_dir.display()))
                .to_owned();
            if let Some(existing) = skills.iter().find(|skill| skill.0 == id) {
                panic!(
                    "skill id `{id}` provided by both official skill sources `{}` and `{}`",
                    existing.1, source.repo_dir_name
                );
            }
            let mut files = collect_files(&skill_dir, !content.cloned);
            files.sort();
            if !content.cloned {
                for path in &files {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
            skills.push((id, source.repo_dir_name, skill_dir, files));
        }
    }
    skills.sort_by(|a, b| a.0.cmp(&b.0));

    let mut generated = String::new();
    generated.push_str("const EMBEDDED_BUILTIN_SKILL_IDS: &[&str] = &[\n");
    for (id, _, _, _) in &skills {
        writeln!(generated, "    {id:?},").expect("write generated skill snapshot");
    }
    generated.push_str("];\n\n");

    let mut static_names = Vec::new();
    for (id, _, skill_dir, files) in &skills {
        let static_name = embedded_skill_files_static_name(id);
        if static_names.contains(&static_name) {
            panic!("skill id `{id}` collides with another skill in generated snapshot");
        }
        static_names.push(static_name.clone());
        writeln!(generated, "static {static_name}: &[EmbeddedSkillFile] = &[")
            .expect("write generated skill snapshot");
        for path in files {
            let rel = path
                .strip_prefix(skill_dir)
                .unwrap_or_else(|err| panic!("strip skill prefix {} failed: {err}", path.display()))
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()));
            writeln!(
                generated,
                "    EmbeddedSkillFile {{ path: {rel:?}, content: {content:?} }},"
            )
            .expect("write generated skill snapshot");
        }
        generated.push_str("];\n\n");
    }

    generated.push_str("static EMBEDDED_BUILTIN_SKILLS: &[EmbeddedBuiltinSkill] = &[\n");
    for (id, _, _, _) in &skills {
        let static_name = embedded_skill_files_static_name(id);
        writeln!(
            generated,
            "    EmbeddedBuiltinSkill {{ id: {id:?}, files: {static_name} }},"
        )
        .expect("write generated skill snapshot");
    }
    generated.push_str("];\n\n");

    let loom_static = skills
        .iter()
        .find(|(id, _, _, _)| id == "loom")
        .map(|(id, _, _, _)| embedded_skill_files_static_name(id))
        .unwrap_or_else(|| panic!("default Loom skill missing from official skill sources"));
    writeln!(
        generated,
        "const EMBEDDED_LOOM_SKILL_FILES: &[EmbeddedSkillFile] = {loom_static};"
    )
    .expect("write generated skill snapshot");

    fs::write(out_dir.join("loom_skill_embedded.rs"), generated)
        .expect("write generated skill snapshot");
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
