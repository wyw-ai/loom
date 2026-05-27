use std::io;
use std::path::{Component, Path, PathBuf};

use proto::methods::AgentBundleSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedBundleInstall {
    pub version: String,
    pub install_dir: PathBuf,
}

pub fn normalized_bundle_version(bundle: &AgentBundleSpec, source: &Path) -> String {
    let raw = if !bundle.version.trim().is_empty() {
        bundle.version.trim().to_string()
    } else {
        source
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("bundle")
            .to_string()
    };
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn resolved_bundle_version(bundle: &AgentBundleSpec, source: &Path) -> String {
    if bundle.source.trim().is_empty() {
        String::new()
    } else {
        normalized_bundle_version(bundle, source)
    }
}

pub fn prepare_bundle_install(
    bundle_root: &Path,
    source: &Path,
    bundle: &AgentBundleSpec,
) -> io::Result<PreparedBundleInstall> {
    let version = resolved_bundle_version(bundle, source);
    validate_version_component(&version)?;
    let install_dir = resolve_versioned_install_dir(bundle_root, &version)?;
    validate_source_target_overlap(source, &install_dir)?;

    Ok(PreparedBundleInstall {
        version,
        install_dir,
    })
}

pub fn validate_bundle_current(
    actor_root: &Path,
    profile: &Path,
    logs: &Path,
    bundle_root: &Path,
    current: &Path,
) -> io::Result<PathBuf> {
    let resolved_current = resolve_path_within(actor_root, current, "bundle current")?;
    let canonical_actor_root = std::fs::canonicalize(actor_root)?;
    if resolved_current == canonical_actor_root {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "bundle current `{}` cannot equal actor root `{}`",
                current.display(),
                actor_root.display()
            ),
        ));
    }

    let canonical_bundle_root = std::fs::canonicalize(bundle_root)?;
    if canonical_bundle_root.starts_with(&resolved_current) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "bundle current `{}` cannot contain bundle root `{}`",
                current.display(),
                bundle_root.display()
            ),
        ));
    }

    for (label, protected) in [("profile", profile), ("logs", logs)] {
        let protected = resolve_path_within(actor_root, protected, label)?;
        if resolved_current.starts_with(&protected) || protected.starts_with(&resolved_current) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "bundle current `{}` overlaps actor {label} dir `{}`",
                    current.display(),
                    protected.display()
                ),
            ));
        }
    }

    Ok(resolved_current)
}

fn validate_version_component(version: &str) -> io::Result<()> {
    let mut components = Path::new(version).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("bundle version `{version}` is not a safe path component"),
        )),
    }
}

fn resolve_versioned_install_dir(bundle_root: &Path, version: &str) -> io::Result<PathBuf> {
    let canonical_root = std::fs::canonicalize(bundle_root)?;
    let install_dir = canonical_root.join(version);
    let canonical_parent = std::fs::canonicalize(install_dir.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "bundle install dir has no parent",
        )
    })?)?;
    if canonical_parent != canonical_root {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "bundle install dir `{}` escapes bundle root `{}`",
                install_dir.display(),
                bundle_root.display()
            ),
        ));
    }
    Ok(install_dir)
}

fn validate_source_target_overlap(source: &Path, install_dir: &Path) -> io::Result<()> {
    let canonical_source = std::fs::canonicalize(source)?;
    if canonical_source == install_dir
        || canonical_source.starts_with(install_dir)
        || install_dir.starts_with(&canonical_source)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "bundle source `{}` overlaps install dir `{}`",
                source.display(),
                install_dir.display()
            ),
        ));
    }
    Ok(())
}

fn absolutize(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn resolve_path_within(root: &Path, path: &Path, label: &str) -> io::Result<PathBuf> {
    let absolute_root = absolutize(root)?;
    let absolute_path = absolutize(path)?;
    let relative = absolute_path.strip_prefix(&absolute_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{label} `{}` must stay under `{}`",
                path.display(),
                root.display()
            ),
        )
    })?;
    let relative = normalize_relative_path(relative, label)?;
    Ok(std::fs::canonicalize(root)?.join(relative))
}

fn normalize_relative_path(path: &Path, label: &str) -> io::Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{label} `{}` is not a safe relative path", path.display()),
                ));
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-bundle-tests-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos()
        ));
        path
    }

    #[test]
    fn resolved_version_uses_source_basename() {
        let bundle = AgentBundleSpec {
            source: "/tmp/demo-bundle".into(),
            version: String::new(),
            ..Default::default()
        };
        assert_eq!(
            resolved_bundle_version(&bundle, Path::new("/tmp/demo-bundle")),
            "demo-bundle"
        );
    }

    #[test]
    fn prepare_bundle_install_rejects_dotdot_version() {
        let root = temp_path("version-dotdot");
        std::fs::create_dir_all(&root).expect("create root");
        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("create source");
        let bundle = AgentBundleSpec {
            source: source.display().to_string(),
            version: "..".into(),
            ..Default::default()
        };

        let err = prepare_bundle_install(&root, &source, &bundle).expect_err("must reject ..");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn prepare_bundle_install_rejects_overlapping_source_and_target() {
        let root = temp_path("overlap");
        std::fs::create_dir_all(root.join("alias")).expect("create alias anchor");
        let source = root.join("demo-bundle");
        std::fs::create_dir_all(&source).expect("create source");
        let bundle_root = root.join("alias").join("..");
        let bundle = AgentBundleSpec {
            source: source.display().to_string(),
            version: String::new(),
            ..Default::default()
        };

        let err = prepare_bundle_install(&bundle_root, &source, &bundle)
            .expect_err("must reject overlap");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn validate_bundle_current_rejects_profile_subtree() {
        let root = temp_path("current-profile");
        let profile = root.join("profile");
        let logs = root.join("logs");
        let bundle_root = root.join("bundles");
        for dir in [&profile, &logs, &bundle_root] {
            std::fs::create_dir_all(dir).expect("create actor dir");
        }

        let err =
            validate_bundle_current(&root, &profile, &logs, &bundle_root, &profile.join("live"))
                .expect_err("must reject profile subtree");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn validate_bundle_current_rejects_bundle_root_ancestor() {
        let root = temp_path("current-ancestor");
        let profile = root.join("profile");
        let logs = root.join("logs");
        let bundle_root = root.join("runtime").join("bundles");
        for dir in [&profile, &logs, &bundle_root] {
            std::fs::create_dir_all(dir).expect("create actor dir");
        }

        let err =
            validate_bundle_current(&root, &profile, &logs, &bundle_root, &root.join("runtime"))
                .expect_err("must reject current containing bundle root");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn validate_bundle_current_allows_dedicated_runtime_path() {
        let root = temp_path("current-valid");
        let profile = root.join("profile");
        let logs = root.join("logs");
        let bundle_root = root.join("runtime").join("bundles");
        let current = root.join("runtime").join("live");
        for dir in [&profile, &logs, &bundle_root] {
            std::fs::create_dir_all(dir).expect("create actor dir");
        }
        let expected = std::fs::canonicalize(&root)
            .expect("canonicalize root")
            .join("runtime")
            .join("live");

        let resolved = validate_bundle_current(&root, &profile, &logs, &bundle_root, &current)
            .expect("current should be valid");

        assert_eq!(resolved, expected);
        std::fs::remove_dir_all(root).ok();
    }
}
