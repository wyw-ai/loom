use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use proto::methods::*;

use crate::render;

pub fn list() -> Result<()> {
    let agents = load_specs()?
        .into_iter()
        .map(|spec| AgentInfo {
            spec,
            status: "registered".into(),
            pid: None,
            session_id: None,
        })
        .collect::<Vec<_>>();
    let res = AgentListResult { agents };
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

pub fn marketplace() -> Result<()> {
    let res = AgentMarketplaceListResult {
        entries: proto::marketplace::list_all(),
    };
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

pub fn install(
    marketplace_id: String,
    local_actor_id: Option<String>,
    display_name: Option<String>,
    prefer: Option<String>,
) -> Result<()> {
    use proto::marketplace::{lookup, resolve, Preference};

    let entry = lookup(&marketplace_id)
        .ok_or_else(|| anyhow::anyhow!("marketplace entry `{marketplace_id}` not found"))?;
    let prefer = match prefer.as_deref().unwrap_or("auto") {
        "npx" => Preference::Npx,
        "uvx" => Preference::Uvx,
        "binary" => Preference::Binary,
        _ => Preference::Auto,
    };
    let resolved = resolve(&entry, prefer, path_lookup_via_env)?;
    let local_id =
        local_actor_id.unwrap_or_else(|| format!("actor_{}", entry.id.replace('-', "_")));
    let display = display_name.unwrap_or_else(|| entry.name.clone());
    let spec = AgentSpec {
        actor: proto::types::Actor {
            id: local_id,
            kind: proto::types::ActorKind::Agent,
            display_name: display,
            capabilities: None,
            _meta: None,
        },
        transport: AgentTransport {
            kind: "acp_stdio".into(),
            command: resolved.command.clone(),
            args: resolved.args.clone(),
            env: resolved.env.clone(),
            auth_method: None,
            session: None,
            output_format: None,
            prompt_via: proto::methods::PromptVia::default(),
        },
        autostart: false,
        bundle: None,
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
    let path = write_spec(&spec)?;
    println!(
        "installed {} at {} (source={}, command={})",
        spec.actor.id,
        path.display(),
        resolved.source,
        spec.transport.command,
    );
    Ok(())
}

pub fn add() -> Result<()> {
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
            auth_method: None,
            session: None,
            output_format: None,
            prompt_via: proto::methods::PromptVia::default(),
        },
        autostart: false,
        bundle: None,
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
    let path = write_spec(&spec)?;
    println!("registered {} at {}", spec.actor.id, path.display());
    Ok(())
}

pub fn register(path: PathBuf) -> Result<()> {
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read agent spec {}", path.display()))?;
    let spec: AgentSpec = serde_json::from_str(&text)?;
    let dest = write_spec(&spec)?;
    println!("registered {} at {}", spec.actor.id, dest.display());
    Ok(())
}

pub fn remove(actor_id: String) -> Result<()> {
    let path = spec_path(&actor_id);
    if !path.exists() {
        bail!("agent spec not found: {}", path.display());
    }
    std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    println!("removed {} ({})", actor_id, path.display());
    Ok(())
}

pub fn start(actor_id: String) -> Result<()> {
    bail!(
        "server no longer starts agents directly. Run `joi agent serve --allow-actors {}`.",
        actor_id
    )
}

pub fn stop(actor_id: String) -> Result<()> {
    bail!(
        "server no longer stops agents directly. Stop the `joi agent serve` process that owns `{}`.",
        actor_id
    )
}

pub fn log(actor_id: String, _tail: u32) -> Result<()> {
    bail!(
        "server no longer stores agent logs. Inspect the `joi agent serve` process that owns `{}`.",
        actor_id
    )
}

fn default_specs_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("joi").join("agents"))
        .unwrap_or_else(|| PathBuf::from(".joi").join("agents"))
}

fn spec_path(actor_id: &str) -> PathBuf {
    default_specs_dir().join(format!("{actor_id}.json"))
}

fn write_spec(spec: &AgentSpec) -> Result<PathBuf> {
    let dir = default_specs_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(format!("{}.json", spec.actor.id));
    let text = serde_json::to_string_pretty(spec)?;
    std::fs::write(&path, format!("{text}\n"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn load_specs() -> Result<Vec<AgentSpec>> {
    let dir = default_specs_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read agent spec {}", path.display()))?;
        let spec: AgentSpec = serde_json::from_str(&text)
            .with_context(|| format!("parse agent spec {}", path.display()))?;
        out.push(spec);
    }
    out.sort_by(|a, b| a.actor.id.cmp(&b.actor.id));
    Ok(out)
}

fn path_lookup_via_env(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return true;
        }
        if cfg!(windows) {
            for ext in ["exe", "cmd", "bat"] {
                let mut c = candidate.clone();
                c.set_extension(ext);
                if c.is_file() {
                    return true;
                }
            }
        }
    }
    false
}

fn prompt(label: &str) -> Result<String> {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}
