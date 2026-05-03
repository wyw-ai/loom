//! Repository-wide guard: catch new references to the retired
//! `dev-helper` / `dev_helper` / `servicectl` / `run-agent.sh`
//! surface introduced after the migration.
//!
//! The migration (see `docs/remove-dev-helper-migration-design.md`)
//! deletes the legacy code paths in favour of joi-native
//! agent/service specs. Once the rewrite is done it is easy to
//! regress by copy-pasting an old recipe; this test is the safety
//! net.
//!
//! Allowed references are whitelisted by *path prefix*. Anything
//! else that mentions one of the legacy tokens fails the test with a
//! file:line list, plus a hint pointing at the design doc.
//!
//! Run with `cargo test -p joi-cli --test no_legacy_refs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const LEGACY_TOKENS: &[&str] = &[
    "dev-helper",
    "dev_helper",
    "servicectl",
    "run-agent.sh",
];

/// Path prefixes (relative to the workspace root) where legacy
/// tokens are still legitimately referenced. Anything outside this
/// set is treated as a regression.
const ALLOWED_PREFIXES: &[&str] = &[
    // The migration tool itself: name + path + module doc.
    "crates/cli/src/bin/migrate_dev_helper.rs",
    // Cargo target entries for the migration bin.
    "crates/cli/Cargo.toml",
    // The design doc and migration plan are the canonical record.
    "docs/",
    // README/CHANGELOG narrate the migration history.
    "README.md",
    "CHANGELOG.md",
    // Source files that link out to the design doc by name in a
    // module-level comment. Reviewing these one-off references is
    // cheaper than deleting them: they're useful breadcrumbs.
    "crates/cli/src/cmd/spec.rs",
    "crates/cli/src/cmd/workspace.rs",
    "crates/cli/src/main.rs",
    "data/agents/README.md",
    "data/services/repo-cache/bundle/sync.sh",
    // The grep guard test itself (this file) lists the tokens.
    "crates/cli/tests/no_legacy_refs.rs",
];

/// Directories to skip outright (build artefacts, vendored deps,
/// node_modules, etc.). The walk is small (<5k files for this
/// repo) so we don't bother with `.gitignore` parsing.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".vscode",
    ".idea",
];

fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR = .../joi-apps/crates/cli; walk up two.
    manifest
        .ancestors()
        .nth(2)
        .expect("workspace root above crates/cli")
        .to_path_buf()
}

fn is_skipped_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

fn is_text_extension(p: &Path) -> bool {
    let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
        return p
            .file_name()
            .and_then(|s| s.to_str())
            .map(|n| {
                matches!(
                    n,
                    "Cargo.toml" | "README" | "CHANGELOG" | "Makefile" | "Dockerfile"
                )
            })
            .unwrap_or(false);
    };
    matches!(
        ext,
        "rs" | "toml"
            | "md"
            | "json"
            | "yaml"
            | "yml"
            | "sh"
            | "bash"
            | "zsh"
            | "py"
            | "ts"
            | "js"
            | "html"
            | "css"
            | "txt"
            | "lock"
    )
}

fn collect_files(root: &Path, into: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if is_skipped_dir(&name) {
                continue;
            }
            collect_files(&path, into);
        } else if is_text_extension(&path) {
            into.push(path);
        }
    }
}

fn matches_allowed_prefix(rel: &str) -> bool {
    ALLOWED_PREFIXES
        .iter()
        .any(|p| rel == *p || rel.starts_with(p))
}

#[test]
fn no_new_dev_helper_references() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);

    let mut hits: BTreeSet<String> = BTreeSet::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if matches_allowed_prefix(&rel) {
            continue;
        }
        let body = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for (lineno, line) in body.lines().enumerate() {
            for token in LEGACY_TOKENS {
                if line.contains(token) {
                    hits.insert(format!("{}:{}: {}", rel, lineno + 1, line.trim()));
                    break;
                }
            }
        }
    }

    if !hits.is_empty() {
        let body: Vec<String> = hits.into_iter().collect();
        panic!(
            "Found {} new reference(s) to retired dev-helper surface.\n\
             Either remove the reference, or — if it's a legitimate \
             pointer to the migration history — add the path prefix \
             to ALLOWED_PREFIXES in this test file.\n\
             See docs/remove-dev-helper-migration-design.md for context.\n\n\
             Hits:\n  {}",
            body.len(),
            body.join("\n  "),
        );
    }
}
