use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ENV_CONFIG_DIR: &str = "JOI_CONFIG_DIR";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    pub actor_id: String,
    #[serde(default)]
    pub display_name: String,
}

impl Default for Config {
    fn default() -> Self {
        let suffix = Uuid::new_v4().simple().to_string()[..8].to_string();
        Self {
            server_url: "ws://127.0.0.1:7878/rpc".into(),
            actor_id: format!("actor_human_{}", suffix),
            display_name: whoami_or("you"),
        }
    }
}

fn whoami_or(default: &str) -> String {
    std::env::var("USER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.into())
}

pub fn config_dir() -> PathBuf {
    if let Some(value) = std::env::var_os(ENV_CONFIG_DIR).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    dirs::home_dir()
        .map(|home| home.join(".joi-apps"))
        .unwrap_or_else(|| PathBuf::from(".joi-apps"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("cli.toml")
}

pub fn agent_specs_dir() -> PathBuf {
    config_dir().join("agents")
}

pub fn service_specs_dir() -> PathBuf {
    config_dir().join("services")
}

pub fn load_or_init() -> Result<Config> {
    migrate_legacy_configs();
    let path = config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<Config>(&text) {
            return Ok(cfg);
        }
    }
    let cfg = Config::default();
    let _ = std::fs::create_dir_all(config_dir());
    let text = toml::to_string_pretty(&cfg)?;
    let _ = std::fs::write(&path, text);
    Ok(cfg)
}

pub fn resolve(
    server_url: Option<String>,
    actor_id: Option<String>,
    display_name: Option<String>,
) -> Result<Config> {
    let mut cfg = load_or_init()?;
    if let Some(s) = server_url.or_else(|| std::env::var("JOI_SERVER").ok()) {
        cfg.server_url = s;
    }
    if let Some(a) = actor_id.or_else(|| std::env::var("JOI_ACTOR").ok()) {
        cfg.actor_id = a;
    }
    if let Some(d) = display_name.or_else(|| std::env::var("JOI_DISPLAY").ok()) {
        cfg.display_name = d;
    }
    Ok(cfg)
}

fn migrate_legacy_configs() {
    let Some(legacy_root) = legacy_config_dir() else {
        return;
    };
    let new_root = config_dir();
    copy_legacy_file(&legacy_root, &new_root, "cli.toml");
    copy_legacy_file(&legacy_root, &new_root, "desktop.toml");
    copy_legacy_dir(&legacy_agent_specs_dir(), &agent_specs_dir());
    copy_legacy_dir(&legacy_service_specs_dir(), &service_specs_dir());
}

fn legacy_config_dir() -> Option<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("joi-apps"))
        .filter(|dir| dir != &config_dir())
}

fn legacy_agent_specs_dir() -> PathBuf {
    dirs::config_dir()
        .map(|dir| dir.join("joi").join("agents"))
        .unwrap_or_else(|| PathBuf::from(".joi").join("agents"))
}

fn legacy_service_specs_dir() -> PathBuf {
    dirs::config_dir()
        .map(|dir| dir.join("joi").join("services"))
        .unwrap_or_else(|| PathBuf::from(".joi").join("services"))
}

fn copy_legacy_file(legacy_root: &std::path::Path, new_root: &std::path::Path, file_name: &str) {
    let source = legacy_root.join(file_name);
    let dest = new_root.join(file_name);
    if dest.exists() || !source.is_file() {
        return;
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::copy(source, dest);
}

fn copy_legacy_dir(source: &std::path::Path, dest: &std::path::Path) {
    if dest.exists() || !source.is_dir() {
        return;
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = copy_dir_recursive(source, dest);
}

fn copy_dir_recursive(source: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if ty.is_file() && !dest_path.exists() {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}
