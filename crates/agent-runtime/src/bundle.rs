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

    let install_dir = bundle_root.join(&version);
    validate_install_dir(bundle_root, &install_dir)?;
    validate_source_target_overlap(source, &install_dir)?;

    Ok(PreparedBundleInstall {
        version,
        install_dir,
    })
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

fn validate_install_dir(bundle_root: &Path, install_dir: &Path) -> io::Result<()> {
    let canonical_root = std::fs::canonicalize(bundle_root)?;
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
    Ok(())
}

fn validate_source_target_overlap(source: &Path, install_dir: &Path) -> io::Result<()> {
    let canonical_source = std::fs::canonicalize(source)?;
    let absolute_install = absolutize(install_dir)?;
    if canonical_source == absolute_install
        || canonical_source.starts_with(&absolute_install)
        || absolute_install.starts_with(&canonical_source)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "joi-bundle-tests-{name}-{}",
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
        let source = root.join("demo-bundle");
        std::fs::create_dir_all(&source).expect("create source");
        let bundle = AgentBundleSpec {
            source: source.display().to_string(),
            version: String::new(),
            ..Default::default()
        };

        let err = prepare_bundle_install(&root, &source, &bundle).expect_err("must reject overlap");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(root).ok();
    }
}
