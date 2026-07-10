use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use proto::methods::method;
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

#[derive(Debug, Clone)]
struct MachineRow {
    id: String,
    name: String,
    connection_actor_id: String,
    workspace_id: Option<String>,
    owner_actor_id: Option<String>,
    inventory_revision: Option<u64>,
    capabilities: Vec<String>,
    agent_count: usize,
}

impl MachineRow {
    fn can_command(&self) -> bool {
        self.capabilities.iter().any(|cap| cap == "machine.command")
    }

    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "connectionActorId": self.connection_actor_id,
            "workspaceId": self.workspace_id,
            "ownerActorId": self.owner_actor_id,
            "inventoryRevision": self.inventory_revision,
            "capabilities": self.capabilities,
            "agentCount": self.agent_count,
            "canCommand": self.can_command(),
        })
    }
}

pub async fn list(client: Arc<Client>) -> Result<()> {
    let machines = list_machine_rows(&client).await?;
    if render::is_json() {
        render::print_json(&json!({
            "machines": machines.iter().map(MachineRow::to_json).collect::<Vec<_>>()
        }));
        return Ok(());
    }

    if machines.is_empty() {
        println!("No daemon machines found.");
        return Ok(());
    }
    for machine in machines {
        let command = if machine.can_command() {
            "command"
        } else {
            "read-only"
        };
        println!(
            "{}\t{}\t{}\tagents={}",
            machine.id, machine.name, command, machine.agent_count
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn agent_create(
    client: Arc<Client>,
    machine_id: String,
    provider_id: String,
    actor_id: Option<String>,
    name: String,
    instructions: Option<String>,
    instructions_file: Option<PathBuf>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    autostart: bool,
) -> Result<()> {
    let machine = require_machine(&client, &machine_id).await?;
    let instructions = read_instructions(instructions, instructions_file)?;
    let mut command = json!({
        "op": "agent.create",
        "providerId": provider_id,
        "name": name,
        "autostart": autostart,
    });
    insert_if_nonempty(&mut command, "actorId", actor_id);
    insert_if_nonempty(&mut command, "instructions", instructions);
    insert_if_nonempty(&mut command, "model", model);
    insert_if_nonempty(&mut command, "reasoningEffort", reasoning_effort);

    let output = run_machine_command(client, machine, command).await?;
    if render::is_json() {
        render::print_json(&output);
    } else {
        let actor_id = output
            .pointer("/agentSpec/actor/id")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let path = output.get("path").and_then(Value::as_str).unwrap_or("");
        if path.is_empty() {
            println!("created agent {actor_id}");
        } else {
            println!("created agent {actor_id}\t{path}");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn agent_update(
    client: Arc<Client>,
    machine_id: String,
    actor_id: String,
    name: Option<String>,
    instructions: Option<String>,
    instructions_file: Option<PathBuf>,
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<()> {
    let machine = require_machine(&client, &machine_id).await?;
    let command = agent_update_command(
        actor_id,
        name,
        instructions,
        instructions_file,
        model,
        reasoning_effort,
    )?;
    let output = run_machine_command(client, machine, command).await?;
    if render::is_json() {
        render::print_json(&output);
    } else {
        let actor_id = output
            .pointer("/agentSpec/actor/id")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let path = output.get("path").and_then(Value::as_str).unwrap_or("");
        if path.is_empty() {
            println!("updated agent {actor_id}");
        } else {
            println!("updated agent {actor_id}\t{path}");
        }
    }
    Ok(())
}

fn agent_update_command(
    actor_id: String,
    name: Option<String>,
    instructions: Option<String>,
    instructions_file: Option<PathBuf>,
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<Value> {
    let instructions = read_instructions(instructions, instructions_file)?;
    let mut command = json!({
        "op": "agent.update",
        "actorId": actor_id,
    });
    if let Some(name) = name.and_then(nonempty_owned) {
        command["name"] = json!(name.clone());
        command["displayName"] = json!(name);
    }
    insert_if_nonempty(&mut command, "instructions", instructions);
    insert_if_nonempty(&mut command, "model", model);
    insert_if_nonempty(&mut command, "reasoningEffort", reasoning_effort);
    Ok(command)
}

pub async fn agent_remove(client: Arc<Client>, machine_id: String, actor_id: String) -> Result<()> {
    let machine = require_machine(&client, &machine_id).await?;
    let output = run_machine_command(
        client,
        machine,
        json!({
            "op": "agent.remove",
            "actorId": actor_id,
        }),
    )
    .await?;
    if render::is_json() {
        render::print_json(&output);
    } else {
        println!("removed agent");
    }
    Ok(())
}

pub async fn agent_skill_list(
    client: Arc<Client>,
    machine_id: String,
    actor_id: String,
) -> Result<()> {
    let machine = require_machine(&client, &machine_id).await?;
    let output = run_machine_command(
        client,
        machine,
        json!({
            "op": "agent.skill.list",
            "actorId": actor_id,
        }),
    )
    .await?;
    if render::is_json() {
        render::print_json(&output);
        return Ok(());
    }
    let skills = output
        .get("skills")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if skills.is_empty() {
        println!("(no custom skills)");
        return Ok(());
    }
    for skill in skills {
        let id = skill.get("id").and_then(Value::as_str).unwrap_or("");
        let source = skill.get("source").and_then(Value::as_str).unwrap_or("");
        println!("{id}\t{source}");
    }
    Ok(())
}

pub async fn agent_skill_add(
    client: Arc<Client>,
    machine_id: String,
    actor_id: String,
    source: PathBuf,
) -> Result<()> {
    let machine = require_machine(&client, &machine_id).await?;
    let command = json!({
        "op": "agent.skill.add",
        "actorId": actor_id,
        "source": source.display().to_string(),
    });
    let output = run_machine_command(client, machine, command).await?;
    if render::is_json() {
        render::print_json(&output);
    } else {
        let skills = output
            .pointer("/agentSpec/bundle/skills")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let added = skills
            .iter()
            .find(|skill| {
                skill
                    .get("source")
                    .and_then(Value::as_str)
                    .map(|value| value == source.display().to_string())
                    .unwrap_or(false)
            })
            .or_else(|| skills.last());
        let id = added
            .and_then(|skill| skill.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        println!("added skill {id}");
    }
    Ok(())
}

pub async fn agent_skill_remove(
    client: Arc<Client>,
    machine_id: String,
    actor_id: String,
    skill_id: String,
) -> Result<()> {
    let machine = require_machine(&client, &machine_id).await?;
    let output = run_machine_command(
        client,
        machine,
        json!({
            "op": "agent.skill.remove",
            "actorId": actor_id,
            "skillId": skill_id,
        }),
    )
    .await?;
    if render::is_json() {
        render::print_json(&output);
    } else if output
        .get("removed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        println!("removed skill");
    } else {
        println!("skill was not configured");
    }
    Ok(())
}

async fn run_machine_command(
    client: Arc<Client>,
    machine: MachineRow,
    command: Value,
) -> Result<Value> {
    if !machine.can_command() {
        bail!(
            "machine `{}` does not advertise machine.command",
            machine.id
        );
    }
    let value = client
        .call_raw(
            method::MACHINE_COMMAND,
            Some(json!({
                "machineId": machine.id,
                "machineActorId": machine.connection_actor_id,
                "workspaceId": machine.workspace_id,
                "ifInventoryRevision": machine.inventory_revision,
                "command": command,
                "timeoutMs": 30_000,
            })),
        )
        .await?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("machine command failed");
        bail!("{error}");
    }
    Ok(value.get("output").cloned().unwrap_or(Value::Null))
}

async fn require_machine(client: &Arc<Client>, machine_id: &str) -> Result<MachineRow> {
    list_machine_rows(client)
        .await?
        .into_iter()
        .find(|machine| machine.id == machine_id)
        .ok_or_else(|| anyhow!("unknown daemon machine `{machine_id}`"))
}

async fn list_machine_rows(client: &Arc<Client>) -> Result<Vec<MachineRow>> {
    let value = client.call_raw(method::ACTOR_LIST, None).await?;
    let actors = value
        .get("actors")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("actor.list returned no actors array"))?;
    let mut machines = actors
        .iter()
        .filter_map(machine_from_actor)
        .collect::<Vec<_>>();
    machines.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(machines)
}

fn machine_from_actor(actor: &Value) -> Option<MachineRow> {
    let meta = actor.get("_meta").and_then(Value::as_object)?;
    if meta.get("role").and_then(Value::as_str) != Some("machine") {
        return None;
    }
    if meta.get("source").and_then(Value::as_str) != Some("daemon") {
        return None;
    }
    let id = meta.get("machineId")?.as_str()?.to_string();
    let connection_actor_id = actor
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("actor_service_{id}"));
    let capabilities = meta
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let agent_count = meta
        .get("agentSpecs")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    Some(MachineRow {
        id,
        name: meta
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("machine")
            .to_string(),
        connection_actor_id,
        workspace_id: nonempty_string(meta.get("workspaceId")),
        owner_actor_id: nonempty_string(meta.get("ownerActorId")),
        inventory_revision: meta.get("revision").and_then(Value::as_u64),
        capabilities,
        agent_count,
    })
}

fn read_instructions(
    instructions: Option<String>,
    instructions_file: Option<PathBuf>,
) -> Result<Option<String>> {
    match (instructions, instructions_file) {
        (Some(_), Some(_)) => bail!("use either --instructions or --instructions-file, not both"),
        (Some(value), None) => Ok(nonempty_owned(value)),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("read instructions file {}", path.display()))
            .map(nonempty_owned),
        (None, None) => Ok(None),
    }
}

fn insert_if_nonempty(target: &mut Value, key: &str, value: Option<String>) {
    let Some(value) = value.and_then(nonempty_owned) else {
        return;
    };
    target[key] = json!(value);
}

fn nonempty_owned(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn nonempty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_update_command_uses_update_operation_and_omits_unspecified_fields() {
        let command = agent_update_command(
            "actor_impl".into(),
            Some("Implementation Agent".into()),
            None,
            None,
            None,
            Some("high".into()),
        )
        .expect("build agent update command");

        assert_eq!(
            command,
            json!({
                "op": "agent.update",
                "actorId": "actor_impl",
                "name": "Implementation Agent",
                "displayName": "Implementation Agent",
                "reasoningEffort": "high",
            })
        );
    }

    #[test]
    fn agent_update_command_rejects_two_instruction_sources() {
        let error = agent_update_command(
            "actor_impl".into(),
            None,
            Some("inline".into()),
            Some(PathBuf::from("AGENTS.md")),
            None,
            None,
        )
        .expect_err("two instruction sources must be rejected");

        assert!(error
            .to_string()
            .contains("use either --instructions or --instructions-file"));
    }
}
