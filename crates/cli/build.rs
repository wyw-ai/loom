use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let guide_dir = find_content_dir("LOOM_GUIDE_DIR", "loom-guide", "guides");
    let skill_dir = find_content_dir("LOOM_SKILL_DIR", "loom-skill", "SKILL.md");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    generate_guide_snapshot(&guide_dir, &out_dir);
    copy_skill_snapshot(&skill_dir, &out_dir);
}

fn find_content_dir(env_name: &str, repo_name: &str, required_rel: &str) -> PathBuf {
    println!("cargo:rerun-if-env-changed={env_name}");

    if let Some(raw) = env::var_os(env_name).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(raw);
        if path.join(required_rel).exists() {
            println!("cargo:rerun-if-changed={}", path.display());
            return path;
        }
        panic!(
            "{env_name} points at {}, but {} is missing",
            path.display(),
            required_rel
        );
    }

    for candidate in content_candidates(repo_name) {
        if candidate.join(required_rel).exists() {
            println!("cargo:rerun-if-changed={}", candidate.display());
            return candidate;
        }
    }

    let candidates = content_candidates(repo_name)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    panic!(
        "missing official {repo_name} content; set {env_name} or clone {repo_name} next to the loom repo. Checked: {candidates}"
    );
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

fn generate_guide_snapshot(guide_dir: &Path, out_dir: &Path) {
    let guides_dir = guide_dir.join("guides");
    println!("cargo:rerun-if-changed={}", guides_dir.display());

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
        println!("cargo:rerun-if-changed={}", path.display());
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

fn copy_skill_snapshot(skill_dir: &Path, out_dir: &Path) {
    let skill_md = skill_dir.join("SKILL.md");
    println!("cargo:rerun-if-changed={}", skill_md.display());
    let target_dir = out_dir.join("loom-skill");
    fs::create_dir_all(&target_dir).expect("create generated loom-skill dir");
    fs::copy(&skill_md, target_dir.join("SKILL.md"))
        .unwrap_or_else(|err| panic!("copy {} failed: {err}", skill_md.display()));
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
