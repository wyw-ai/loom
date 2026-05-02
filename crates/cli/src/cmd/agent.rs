use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
            model: None,
            session: None,
            output_format: None,
            prompt_via: proto::methods::PromptVia::default(),
            interactive: None,
            provider: None,
        },
        autostart: false,
        models: None,
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
        handoff: None,
        prompt_template: None,
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
            model: None,
            session: None,
            output_format: None,
            prompt_via: proto::methods::PromptVia::default(),
            interactive: None,
            provider: None,
        },
        autostart: false,
        models: None,
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
        handoff: None,
        prompt_template: None,
    };
    let path = write_spec(&spec)?;
    println!("registered {} at {}", spec.actor.id, path.display());
    Ok(())
}

pub fn example() {
    println!(
        "{}",
        r#"Joi AgentSpec examples
======================

`joi agent serve` loads AgentSpec JSON files from `~/.config/joi/agents/` by
default. You can also point at another directory:

  joi agent serve --specs ./agents
  joi agent serve --specs ./agents --allow-actors actor_claude,actor_copilot

Each file describes one actor and one transport. Register a single file with:

  joi agent register ./agents/actor_example.json

Common fields
-------------

- `actor.id`: stable actor id, usually `actor_*`.
- `actor.kind`: use `agent` for normal agents.
- `transport.kind`: one of `acp_stdio`, `command`, or `interactive_command`.
- `transport.command`: executable to spawn.
- `transport.args`: argv passed to the executable. Use arrays, not shell strings.
- `transport.env`: environment variables for the provider process.
- `models.default`: optional default model. For `interactive_command`, Joi appends
  `--model=<model>` to the final argv when a model is active.
- `bundle`: optional local skills/bundle source. When published into server data,
  scope skills can expose these bundles under the current workspace's `skills/`.

1. acp_stdio
------------

Use this when the provider speaks Agent Client Protocol over stdio. Completion,
streaming, tool calls, and session lifecycle are handled by ACP frames.

Example:

{
  "actor": {
    "id": "actor_acp_demo",
    "kind": "agent",
    "displayName": "ACP Demo",
    "capabilities": {}
  },
  "transport": {
    "kind": "acp_stdio",
    "command": "npx",
    "args": ["-y", "@example/acp-agent"],
    "env": {
      "EXAMPLE_API_KEY": "${EXAMPLE_API_KEY}"
    }
  },
  "autostart": false,
  "identity": {},
  "memory": {
    "delivery": {
      "prompt": true,
      "mcp": true
    }
  }
}

Notes:
- Prefer `acp_stdio` when the provider supports ACP natively.
- The child process is long-lived.
- Joi does not need a sentinel because ACP supplies completion events.

2. command
----------

Use this for one-shot commands where process exit means the turn is complete.
Joi can pass the prompt by argv or stdin depending on `promptVia`.

Example:

{
  "actor": {
    "id": "actor_command_demo",
    "kind": "agent",
    "displayName": "Command Demo",
    "capabilities": {}
  },
  "transport": {
    "kind": "command",
    "command": "python3",
    "args": ["./scripts/answer_once.py", "{prompt}"],
    "env": {
      "JOI_MODE": "command"
    },
    "promptVia": {
      "kind": "args"
    },
    "outputFormat": {
      "kind": "text"
    },
    "session": {
      "idStrategy": "extract",
      "extract": {
        "from": "stdout",
        "regex": "SESSION_ID=([A-Za-z0-9_-]+)"
      }
    }
  },
  "autostart": false
}

Notes:
- Use `command` only when the provider exits after each prompt.
- stdout becomes the user-visible response unless a structured output format is configured.
- If the provider supports resume, configure `transport.session`; otherwise omit it.

3. interactive_command
----------------------

Use this for Claude/Copilot-style CLIs that accept a prompt plus a session id,
but do not expose ACP frames and may not use process exit as the logical turn
boundary. Joi owns one provider session per `(actor, scope.kind, scope.id)` and
uses an explicit completion sentinel, default `__JOI_DONE__`.

Claude example:

{
  "actor": {
    "id": "actor_claude",
    "kind": "agent",
    "displayName": "Claude Interactive",
    "capabilities": {}
  },
  "transport": {
    "kind": "interactive_command",
    "command": "claude",
    "model": "sonnet",
    "interactive": {
      "session": {
        "newArgs": ["{prompt}", "--session-id", "{session_id}"],
        "resumeArgs": ["{prompt}", "--resume", "{session_id}"]
      },
      "prompt": {
        "completionContract": {
          "sentinel": "__JOI_DONE__"
        }
      },
      "completion": {
        "maxTurnMs": 120000
      },
      "output": {
        "stripSentinel": true,
        "stripAnsi": true
      },
      "kill": {
        "onComplete": { "action": "sigterm", "graceMs": 0, "fallback": "sigkill" },
        "onCancel": { "action": "sigterm", "graceMs": 0, "fallback": "sigkill" },
        "onTimeout": { "action": "sigkill" }
      }
    },
    "provider": {
      "kind": "claude",
      "settings": {
        "mode": "actor_profile"
      }
    }
  },
  "models": {
    "default": "sonnet",
    "choices": [
      { "id": "sonnet", "label": "Claude Sonnet" },
      { "id": "opus", "label": "Claude Opus" }
    ]
  },
  "autostart": false,
  "identity": {},
  "memory": {
    "delivery": {
      "prompt": true,
      "mcp": true
    }
  }
}

Claude settings modes:
- `global`: omit `--settings`.
- `actor_profile`: pass `--settings {agent.profile}/claude/settings.json`.
- `custom`: pass `--settings <path>` from `provider.settings.path`.

Copilot example:

{
  "actor": {
    "id": "actor_copilot",
    "kind": "agent",
    "displayName": "Copilot Interactive",
    "capabilities": {}
  },
  "transport": {
    "kind": "interactive_command",
    "command": "copilot",
    "interactive": {
      "session": {
        "newArgs": ["--interactive", "{prompt}", "--resume", "{session_id}", "--silent", "--no-color"],
        "resumeArgs": ["--interactive", "{prompt}", "--resume", "{session_id}", "--silent", "--no-color"]
      },
      "completion": {
        "maxTurnMs": 120000
      },
      "kill": {
        "onComplete": { "action": "sigterm", "graceMs": 0, "fallback": "sigkill" },
        "onCancel": { "action": "sigterm", "graceMs": 0, "fallback": "sigkill" },
        "onTimeout": { "action": "sigkill" }
      }
    }
  },
  "autostart": false
}

interactive_command notes:
- `{prompt}` is the Joi envelope plus the completion contract and the new user message.
- `{session_id}` is a Joi-owned UUID for first use, then the saved provider session id.
- The default session boundary is one provider session per actor per thread, and one
  separate provider session per actor per channel common area.
- A successful turn requires detecting the sentinel, flushing final text, saving/updating
  the session record, applying the completion kill policy, and emitting a successful finish.
- Process exit without the sentinel is treated as a failed turn.
- If a model is active, Joi appends exactly one argv token: `--model=<model>`.
- The provider runs from the current scope workspace. If scope skills are enabled, the
  workspace contains `skills/` pointing at the current thread or channel skills.
"#
    );
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

pub(crate) fn default_specs_dir() -> PathBuf {
    if let Ok(s) = std::env::var("JOI_AGENT_SPECS") {
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    dirs::config_dir()
        .map(|d| d.join("joi").join("agents"))
        .unwrap_or_else(|| PathBuf::from(".joi").join("agents"))
}

fn spec_path(actor_id: &str) -> PathBuf {
    default_specs_dir().join(format!("{actor_id}.json"))
}

pub(crate) fn load_specs_at(dir: &Path) -> Result<Vec<AgentSpec>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
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
    load_specs_at(&default_specs_dir())
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
