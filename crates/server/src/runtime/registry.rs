//! Disk-backed registry helpers. Most of this lives directly in `RuntimeManager`
//! today (see `mod.rs::load_disk_specs` / `register`), but this module exists so
//! that the supervision logic and disk persistence can be split apart later.

use std::path::Path;

use proto::methods::AgentSpec;

pub fn read_spec(path: &Path) -> std::io::Result<AgentSpec> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn write_spec(path: &Path, spec: &AgentSpec) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(spec)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}
