//! Local GUI config.
//!
//! Layout: `~/.config/joi-apps/desktop.toml`
//!
//! ```toml
//! active = "default"
//!
//! [[workspaces]]
//! id = "default"
//! name = "Local"
//! server_url = "ws://127.0.0.1:7878/rpc"
//! actor_id = "actor_human_abcdef12"
//! display_name = "bojun"
//! ```
//!
//! We deliberately keep this file separate from the TUI's `cli.toml`. The
//! TUI only knows about a single profile, and forcing it to share storage
//! with the GUI's multi-workspace world would risk a migration nightmare.
//! On first launch we migrate the TUI's `cli.toml` into a starter workspace
//! named "Local" so the operator doesn't face an empty picker.

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub server_url: String,
    pub actor_id: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesktopConfig {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("joi-apps"))
        .unwrap_or_else(|| PathBuf::from(".joi-apps"))
}

pub fn desktop_config_path() -> PathBuf {
    config_dir().join("desktop.toml")
}

fn legacy_cli_config_path() -> PathBuf {
    config_dir().join("cli.toml")
}

pub fn load_or_init() -> Result<DesktopConfig> {
    let path = desktop_config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<DesktopConfig>(&text) {
            return Ok(cfg);
        }
    }
    // First-run seeding: if the TUI has already been used, mirror its profile
    // as the initial "Local" workspace so the GUI picker is non-empty.
    let mut cfg = DesktopConfig::default();
    if let Some(seed) = try_seed_from_cli() {
        cfg.active = Some(seed.id.clone());
        cfg.workspaces.push(seed);
    }
    save(&cfg).ok();
    Ok(cfg)
}

pub fn save(cfg: &DesktopConfig) -> Result<()> {
    let _ = std::fs::create_dir_all(config_dir());
    let text = toml::to_string_pretty(cfg)?;
    std::fs::write(desktop_config_path(), text)?;
    Ok(())
}

fn try_seed_from_cli() -> Option<Workspace> {
    #[derive(Deserialize)]
    struct CliLike {
        server_url: String,
        actor_id: String,
        #[serde(default)]
        display_name: String,
    }
    let text = std::fs::read_to_string(legacy_cli_config_path()).ok()?;
    let parsed: CliLike = toml::from_str(&text).ok()?;
    Some(Workspace {
        id: "default".into(),
        name: "Local".into(),
        server_url: parsed.server_url,
        actor_id: parsed.actor_id,
        display_name: parsed.display_name,
    })
}

pub fn generate_id() -> String {
    format!("ws_{}", &Uuid::new_v4().simple().to_string()[..8])
}
