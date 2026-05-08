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

use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub id: String,
    pub name: String,
    #[serde(default = "default_machine_kind")]
    pub kind: String,
    pub specs_dir: String,
    pub data_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesktopConfig {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub machines: Vec<MachineConfig>,
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
            return Ok(with_default_machines(cfg));
        }
    }
    // First-run seeding: if the TUI has already been used, mirror its profile
    // as the initial "Local" workspace so the GUI picker is non-empty.
    let mut cfg = DesktopConfig::default();
    if let Some(seed) = try_seed_from_cli() {
        cfg.active = Some(seed.id.clone());
        cfg.workspaces.push(seed);
    }
    cfg = with_default_machines(cfg);
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

pub fn generate_machine_id() -> String {
    format!("machine_{}", &Uuid::new_v4().simple().to_string()[..8])
}

pub fn default_agent_specs_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("joi").join("agents"))
        .unwrap_or_else(|| PathBuf::from(".joi").join("agents"))
}

pub fn default_agent_data_root() -> PathBuf {
    dirs::home_dir()
        .map(|d| d.join(".agentx"))
        .unwrap_or_else(|| PathBuf::from(".agentx"))
}

pub fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

pub fn home_path_expr(path: &Path) -> String {
    abbreviate_home(path).unwrap_or_else(|| path.display().to_string())
}

pub fn default_agent_specs_dir_expr() -> String {
    home_path_expr(&default_agent_specs_dir())
}

pub fn default_agent_data_root_expr() -> String {
    "~/.agentx".into()
}

pub fn machine_specs_dir_expr(workspace_key: &str, machine_key: &str) -> String {
    format!("~/.joi-apps/machines/{workspace_key}/{machine_key}/agents")
}

pub fn machine_data_root_expr(workspace_key: &str, machine_key: &str) -> String {
    format!("~/.agentx/machines/{workspace_key}/{machine_key}")
}

fn abbreviate_home(path: &Path) -> Option<String> {
    let home = dirs::home_dir()?;
    let rest = path.strip_prefix(home).ok()?;
    if rest.as_os_str().is_empty() {
        Some("~".into())
    } else {
        Some(format!("~/{}", rest.display()))
    }
}

pub fn active_workspace_id(cfg: &DesktopConfig) -> Option<&str> {
    cfg.active
        .as_deref()
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .or_else(|| cfg.workspaces.first())
        .map(|workspace| workspace.id.as_str())
}

pub fn machine_belongs_to_active_workspace(machine: &MachineConfig, cfg: &DesktopConfig) -> bool {
    machine.workspace_id.as_deref() == active_workspace_id(cfg)
}

pub fn default_machine_for_workspace(workspace_id: &str) -> MachineConfig {
    let workspace_key = safe_config_key(workspace_id);
    let suffix = workspace_key
        .strip_prefix("ws_")
        .unwrap_or(workspace_key.as_str());
    MachineConfig {
        workspace_id: Some(workspace_id.to_string()),
        id: format!("machine_{suffix}"),
        name: "Local Machine".into(),
        kind: default_machine_kind(),
        specs_dir: machine_specs_dir_expr(&workspace_key, "local"),
        data_root: machine_data_root_expr(&workspace_key, "local"),
    }
}

fn with_default_machines(mut cfg: DesktopConfig) -> DesktopConfig {
    let mut changed = false;

    for machine in &mut cfg.machines {
        if normalize_home_path_expr(&mut machine.specs_dir) {
            changed = true;
        }
        if normalize_home_path_expr(&mut machine.data_root) {
            changed = true;
        }
    }

    if cfg.workspaces.is_empty() {
        if cfg.machines.is_empty() {
            cfg.machines.push(default_machine());
            changed = true;
        }
        if changed {
            save(&cfg).ok();
        }
        return cfg;
    }

    let active_id = active_workspace_id(&cfg)
        .map(ToString::to_string)
        .expect("workspaces is not empty");
    if cfg.active.as_deref() != Some(active_id.as_str()) {
        cfg.active = Some(active_id.clone());
        changed = true;
    }

    for machine in &mut cfg.machines {
        if machine.workspace_id.is_none() {
            machine.workspace_id = Some(active_id.clone());
            changed = true;
        }
        if normalize_home_path_expr(&mut machine.specs_dir) {
            changed = true;
        }
        if normalize_home_path_expr(&mut machine.data_root) {
            changed = true;
        }
    }

    let workspace_ids: Vec<String> = cfg
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect();
    for workspace_id in workspace_ids {
        if !cfg
            .machines
            .iter()
            .any(|machine| machine.workspace_id.as_deref() == Some(workspace_id.as_str()))
        {
            cfg.machines
                .push(default_machine_for_workspace(&workspace_id));
            changed = true;
        }
    }

    if changed {
        save(&cfg).ok();
    }
    cfg
}

fn default_machine() -> MachineConfig {
    MachineConfig {
        workspace_id: None,
        id: "local".into(),
        name: "Local Machine".into(),
        kind: default_machine_kind(),
        specs_dir: default_agent_specs_dir_expr(),
        data_root: default_agent_data_root_expr(),
    }
}

fn default_machine_kind() -> String {
    "local".into()
}

fn safe_config_key(value: &str) -> String {
    let key: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if key.is_empty() {
        "workspace".into()
    } else {
        key
    }
}

fn normalize_home_path_expr(value: &mut String) -> bool {
    let resolved = expand_home(value);
    if !resolved.is_absolute() {
        return false;
    }
    let next = home_path_expr(&resolved);
    if next == *value {
        false
    } else {
        *value = next;
        true
    }
}
