use std::path::Path;

use anyhow::{bail, Result};
use serde_json::json;

use crate::{cmd::agent_serve, render};

pub fn materialize(id: &str, output: &Path) -> Result<()> {
    if id != "loom" {
        bail!("unsupported embedded skill `{id}`; expected `loom`");
    }
    let path = agent_serve::materialize_embedded_loom_skill(output)?;
    if render::is_json() {
        render::print_json(&json!({
            "version": "loom.skill-materialization.v1",
            "id": id,
            "path": path,
            "source": "embedded",
        }));
    } else {
        println!("{}", path.display());
    }
    Ok(())
}
