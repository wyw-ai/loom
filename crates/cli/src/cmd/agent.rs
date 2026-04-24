use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn list(client: Arc<Client>) -> Result<()> {
    let res: AgentListResult = client.call(method::AGENT_LIST, json!({})).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.agents.is_empty() {
        println!("(no registered agents)");
    }
    for a in res.agents {
        println!(
            "{}\t{}\tstatus={}\tcommand={}",
            a.spec.actor.id, a.spec.actor.display_name, a.status, a.spec.transport.command,
        );
    }
    Ok(())
}

pub async fn marketplace(client: Arc<Client>) -> Result<()> {
    let res: AgentMarketplaceListResult = client
        .call(method::AGENT_LIST_MARKETPLACE, json!({}))
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.entries.is_empty() {
        println!("(empty marketplace)");
        return Ok(());
    }
    for e in res.entries {
        let mut dists: Vec<&str> = Vec::new();
        if e.distribution.npx.is_some() {
            dists.push("npx");
        }
        if e.distribution.uvx.is_some() {
            dists.push("uvx");
        }
        if e.distribution.binary.is_some() {
            dists.push("binary");
        }
        println!(
            "{:<14} {:<12} [{}]\t{}",
            e.id,
            e.version,
            dists.join(","),
            e.description,
        );
    }
    Ok(())
}

pub async fn install(
    client: Arc<Client>,
    marketplace_id: String,
    local_actor_id: Option<String>,
    display_name: Option<String>,
    prefer: Option<String>,
) -> Result<()> {
    let mut params = json!({ "marketplaceId": marketplace_id });
    if let Some(id) = local_actor_id {
        params["localActorId"] = json!(id);
    }
    if let Some(name) = display_name {
        params["displayName"] = json!(name);
    }
    if let Some(p) = prefer {
        params["prefer"] = json!(p);
    }
    let res: AgentInstallResult = client.call(method::AGENT_INSTALL, params).await?;
    println!(
        "installed {} (source={}, command={})",
        res.agent.spec.actor.id, res.source, res.agent.spec.transport.command,
    );
    Ok(())
}

pub async fn add(client: Arc<Client>) -> Result<()> {
    let id = prompt("actor id (e.g. actor_my_agent): ")?;
    if id.is_empty() {
        anyhow::bail!("actor id is required");
    }
    let display = prompt("display name: ")?;
    let command = prompt("command (e.g. npx, claude-acp): ")?;
    if command.is_empty() {
        anyhow::bail!("command is required");
    }
    let args_line = prompt("args (space-separated, blank for none): ")?;
    let args: Vec<String> = if args_line.is_empty() {
        Vec::new()
    } else {
        args_line.split_whitespace().map(String::from).collect()
    };
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    loop {
        let kv = prompt("env KEY=VAL (blank to finish): ")?;
        if kv.is_empty() {
            break;
        }
        if let Some((k, v)) = kv.split_once('=') {
            env.insert(k.trim().into(), v.trim().into());
        } else {
            eprintln!("(skipped) expected KEY=VAL, got `{}`", kv);
        }
    }
    let cwd = prompt("cwd (blank for {agent.workspace}): ")?;
    let cwd = if cwd.is_empty() {
        "{agent.workspace}".into()
    } else {
        cwd
    };

    let spec = AgentSpec {
        actor: proto::types::Actor {
            id: id.clone(),
            kind: proto::types::ActorKind::Agent,
            display_name: if display.is_empty() {
                id.clone()
            } else {
                display
            },
            capabilities: None,
            _meta: None,
        },
        transport: AgentTransport {
            kind: "acp_stdio".into(),
            command,
            args,
            env,
            cwd,
            auth_method: None,
            session: None,
            output_format: None,
            prompt_via: proto::methods::PromptVia::default(),
        },
        autostart: false,
        identity: Some(proto::methods::IdentitySpec::default()),
        memory: Some(proto::methods::MemorySpec {
            delivery: proto::methods::MemoryDeliverySpec {
                prompt: true,
                mcp: true,
            },
            ..Default::default()
        }),
        announcement: None,
    };
    let res: AgentRegisterResult = client
        .call(method::AGENT_REGISTER, json!({ "spec": spec }))
        .await?;
    println!("registered {}", res.agent.spec.actor.id);
    Ok(())
}

pub async fn register(client: Arc<Client>, path: PathBuf) -> Result<()> {
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read agent spec {}", path.display()))?;
    let spec: AgentSpec = serde_json::from_str(&text)?;
    let res: AgentRegisterResult = client
        .call(method::AGENT_REGISTER, json!({ "spec": spec }))
        .await?;
    println!("registered {}", res.agent.spec.actor.id);
    Ok(())
}

pub async fn remove(client: Arc<Client>, actor_id: String) -> Result<()> {
    let _: AgentOkResult = client
        .call(method::AGENT_UNREGISTER, json!({ "actorId": actor_id }))
        .await?;
    println!("removed {}", actor_id);
    Ok(())
}

pub async fn start(client: Arc<Client>, actor_id: String) -> Result<()> {
    let res: AgentSimpleResult = client
        .call(method::AGENT_START, json!({ "actorId": actor_id }))
        .await?;
    println!(
        "started {} (status={})",
        res.agent.spec.actor.id, res.agent.status
    );
    Ok(())
}

pub async fn stop(client: Arc<Client>, actor_id: String) -> Result<()> {
    let _: AgentOkResult = client
        .call(method::AGENT_STOP, json!({ "actorId": actor_id }))
        .await?;
    println!("stopped {}", actor_id);
    Ok(())
}

pub async fn log(client: Arc<Client>, actor_id: String, tail: u32) -> Result<()> {
    let res: AgentLogResult = client
        .call(
            method::AGENT_LOG,
            json!({ "actorId": actor_id, "tail": tail }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.lines.is_empty() {
        println!("(no log lines)");
    }
    for line in res.lines {
        println!("{}", line);
    }
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}
