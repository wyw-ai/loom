use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const EMBEDDED_MARKETPLACE: &str = include_str!("../../../assets/marketplace.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marketplace {
    pub version: String,
    pub agents: Vec<MarketplaceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub distribution: Distribution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npx: Option<NpxDist>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvx: Option<UvxDist>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<BTreeMap<String, BinaryDist>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpxDist {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UvxDist {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryDist {
    pub archive: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTransport {
    pub source: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

pub fn load_embedded() -> Marketplace {
    serde_json::from_str(EMBEDDED_MARKETPLACE)
        .expect("embedded marketplace.json must be valid JSON matching the Marketplace schema")
}

pub fn lookup(id: &str) -> Option<MarketplaceEntry> {
    load_embedded().agents.into_iter().find(|e| e.id == id)
}

pub fn list_all() -> Vec<MarketplaceEntry> {
    load_embedded().agents
}

pub fn current_platform_key() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "darwin-aarch64"
        } else {
            "darwin-x86_64"
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "aarch64") {
            "linux-aarch64"
        } else {
            "linux-x86_64"
        }
    } else if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "windows-aarch64"
        } else {
            "windows-x86_64"
        }
    } else {
        "unknown"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    Auto,
    Npx,
    Uvx,
    Binary,
}

#[derive(Debug)]
pub enum ResolveError {
    NoNpx,
    NoUvx,
    NoBinaryForPlatform(String),
    BinaryNotOnPath(String),
    NoDistribution,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoNpx => write!(
                f,
                "`npx` not found on PATH (install Node.js: https://nodejs.org)"
            ),
            Self::NoUvx => write!(
                f,
                "`uvx` not found on PATH (install uv: https://docs.astral.sh/uv/)"
            ),
            Self::NoBinaryForPlatform(p) => {
                write!(f, "no binary distribution for platform `{}`", p)
            }
            Self::BinaryNotOnPath(cmd) => write!(
                f,
                "binary `{}` not on PATH (install the agent binary or set up PATH)",
                cmd
            ),
            Self::NoDistribution => write!(f, "marketplace entry has no usable distribution"),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolve a marketplace entry to a runnable command + args + env, given a
/// `path_lookup` predicate that returns true if a binary is on PATH. The CLI
/// passes a `which::which`-backed closure; the server can pass the same.
pub fn resolve<F>(
    entry: &MarketplaceEntry,
    prefer: Preference,
    path_lookup: F,
) -> Result<ResolvedTransport, ResolveError>
where
    F: Fn(&str) -> bool,
{
    let try_npx = || -> Option<ResolvedTransport> {
        let n = entry.distribution.npx.as_ref()?;
        if !path_lookup("npx") {
            return None;
        }
        let mut args = vec!["-y".to_string(), n.package.clone()];
        args.extend(n.args.iter().cloned());
        Some(ResolvedTransport {
            source: "npx".into(),
            command: "npx".into(),
            args,
            env: n.env.clone(),
        })
    };
    let try_uvx = || -> Option<ResolvedTransport> {
        let u = entry.distribution.uvx.as_ref()?;
        if !path_lookup("uvx") {
            return None;
        }
        let mut args = vec![u.package.clone()];
        args.extend(u.args.iter().cloned());
        Some(ResolvedTransport {
            source: "uvx".into(),
            command: "uvx".into(),
            args,
            env: u.env.clone(),
        })
    };
    let try_binary = || -> Result<Option<ResolvedTransport>, ResolveError> {
        let bins = match entry.distribution.binary.as_ref() {
            Some(b) => b,
            None => return Ok(None),
        };
        let key = current_platform_key();
        let entry_bin = bins
            .get(key)
            .ok_or_else(|| ResolveError::NoBinaryForPlatform(key.into()))?;
        // resolve-only: drop "./" prefix since we expect the binary on PATH.
        let stripped = entry_bin.cmd.trim_start_matches("./");
        let basename = std::path::Path::new(stripped)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(stripped);
        if !path_lookup(basename) {
            return Err(ResolveError::BinaryNotOnPath(basename.into()));
        }
        Ok(Some(ResolvedTransport {
            source: "binary".into(),
            command: basename.into(),
            args: entry_bin.args.clone(),
            env: entry_bin.env.clone(),
        }))
    };

    match prefer {
        Preference::Npx => try_npx().ok_or(ResolveError::NoNpx),
        Preference::Uvx => try_uvx().ok_or(ResolveError::NoUvx),
        Preference::Binary => try_binary()?.ok_or(ResolveError::NoDistribution),
        Preference::Auto => {
            if let Some(t) = try_npx() {
                return Ok(t);
            }
            if let Some(t) = try_uvx() {
                return Ok(t);
            }
            match try_binary() {
                Ok(Some(t)) => Ok(t),
                Ok(None) => Err(ResolveError::NoDistribution),
                Err(e) => Err(e),
            }
        }
    }
}
