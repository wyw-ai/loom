mod client;
mod cmd;
mod config;
mod render;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::client::Client;
use crate::render::OutputMode;

#[derive(Parser, Debug)]
#[command(name = "joi", about = "Joi multi-actor collaboration CLI")]
struct Args {
    /// Override the configured server URL (defaults to ws://127.0.0.1:7878/rpc).
    #[arg(long, global = true, env = "JOI_SERVER")]
    server: Option<String>,
    /// Override the configured local actor id.
    #[arg(long = "as", global = true, env = "JOI_ACTOR")]
    actor: Option<String>,
    /// Override the configured local display name.
    #[arg(long = "display", global = true, env = "JOI_DISPLAY")]
    display: Option<String>,
    /// Emit machine-readable JSON instead of human-friendly text.
    #[arg(long, global = true, env = "JOI_JSON")]
    json: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Show local config + server info.
    Who,
    /// Manage channels.
    Channel {
        #[command(subcommand)]
        sub: ChannelCmd,
    },
    /// Manage threads.
    Thread {
        #[command(subcommand)]
        sub: ThreadCmd,
    },
    /// Send a content.add event into a thread.
    Say {
        text: String,
        #[arg(long)]
        r#in: String,
        #[arg(long)]
        reply: Option<String>,
    },
    /// Hand off the turn to an agent: a `content.add` event carrying a
    /// `HandsOffTo` relation pointing at the target actor.
    Handoff {
        /// Target actor id; omit to pick from a list of registered agents/humans.
        agent: Option<String>,
        #[arg(long)]
        r#in: String,
        #[arg(long, default_value = "")]
        message: String,
    },
    /// Respond to an action.request event.
    Action {
        #[command(subcommand)]
        sub: ActionCmd,
    },
    /// Manage agents.
    Agent {
        #[command(subcommand)]
        sub: AgentCmd,
    },
    /// Read events / history from a scope (thread by default; pass --channel for a channel scope).
    Event {
        #[command(subcommand)]
        sub: EventCmd,
    },
    /// Inspect actors known to the server.
    Actor {
        #[command(subcommand)]
        sub: ActorCmd,
    },
    /// Publish, fetch, or read artifacts.
    Artifact {
        #[command(subcommand)]
        sub: ArtifactCmd,
    },
    /// Interactive chat REPL. Bind to a thread with `--in <tid>` or to a
    /// channel's common area with `--channel <cid>`. Omit both to launch
    /// the UI without a bound scope — the sidebar opens automatically so
    /// you can pick or create one without leaving the TUI.
    Chat {
        /// Bind to a thread scope.
        #[arg(long)]
        r#in: Option<String>,
        /// Bind to a channel's public common area.
        #[arg(long, conflicts_with = "in")]
        channel: Option<String>,
    },
    /// Run joi as a stdio MCP server. Typically not invoked by humans —
    /// the runtime auto-injects this as a `session/new.mcpServers` entry
    /// when an agent's spec opts into `memory.delivery.mcp`.
    Mcp {
        #[command(subcommand)]
        sub: McpCmd,
    },
}

#[derive(Subcommand, Debug)]
enum McpCmd {
    /// Expose the running actor's per-actor memory as MCP tools
    /// (`memory.query` / `memory.append` / `memory.get`).
    Memory {
        /// Actor id used as `actorId` on newly-appended records. Usually
        /// supplied via the env (`JOI_ACTOR`) but explicit takes precedence.
        #[arg(long = "actor-id", env = "JOI_ACTOR")]
        actor_id: String,
        /// Path to `{agent.profile}` — the runtime expands `{agent.profile}`
        /// before spawn, so specs should hand over a fully-resolved path.
        #[arg(long = "profile-dir")]
        profile_dir: PathBuf,
        /// Optional override for the store's shard granularity. Defaults to
        /// `month`. Accepted: `month`, `day`.
        #[arg(long = "shard-by")]
        shard_by: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ChannelCmd {
    /// Create a new channel. Channels created via this CLI are private by
    /// default — the caller is the sole initial member; invite others
    /// with `joi channel invite`.
    Create {
        #[arg(long)]
        title: String,
    },
    /// List channels visible to this caller (public channels + private
    /// channels the caller is a member of).
    List,
    /// Add an actor to a channel's member set.
    Invite {
        channel_id: String,
        actor_id: String,
    },
    /// Remove an actor from a channel's member set.
    Revoke {
        channel_id: String,
        actor_id: String,
    },
    /// Print the resolved member rows for a channel.
    Members { channel_id: String },
}

#[derive(Subcommand, Debug)]
enum ThreadCmd {
    Create {
        #[arg(long)]
        channel: String,
        #[arg(long, default_value = "Untitled")]
        title: String,
    },
    List {
        #[arg(long)]
        channel: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum EventCmd {
    /// List events in a thread (default) or channel scope.
    List {
        /// Scope id (thread id by default; pass --channel to read a channel scope).
        #[arg(long)]
        r#in: String,
        /// Read a channel scope instead of a thread scope.
        #[arg(long)]
        channel: bool,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Cursor: only return events older than this event id.
        #[arg(long)]
        before: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ActorCmd {
    /// List every actor the server knows about.
    List,
}

#[derive(Subcommand, Debug)]
enum ArtifactCmd {
    /// Publish an inline-text artifact (body comes from --text, --file, or stdin).
    Publish {
        /// Filename to record on the artifact (also used in the artifact:// uri).
        #[arg(long)]
        name: String,
        /// Defaults to text/markdown.
        #[arg(long = "media-type")]
        media_type: Option<String>,
        /// Inline body text. Mutually exclusive with --file.
        #[arg(long, conflicts_with = "file")]
        text: Option<String>,
        /// Read body from a local file. Mutually exclusive with --text.
        #[arg(long, conflicts_with = "text")]
        file: Option<PathBuf>,
    },
    /// Fetch artifact metadata by id (`art_…`) or by `artifact://` uri.
    Get { id_or_uri: String },
    /// Print artifact body (text only). Use --max-bytes to fetch more than 64 KiB.
    Read {
        artifact_id: String,
        #[arg(long, default_value_t = 65536)]
        max_bytes: u64,
    },
}

#[derive(Subcommand, Debug)]
enum ActionCmd {
    Accept {
        event_id: String,
        #[arg(long, default_value = "allow")]
        option: String,
    },
    Decline {
        event_id: String,
        #[arg(long, default_value = "deny")]
        option: String,
    },
}

#[derive(Subcommand, Debug)]
enum AgentCmd {
    /// List locally registered agents.
    List,
    /// Show the bundled marketplace catalog.
    Marketplace,
    /// Install an agent from the bundled marketplace.
    Install {
        marketplace_id: String,
        /// Local actor id to assign (defaults to `actor_<marketplace_id>`).
        #[arg(long = "actor-id")]
        local_actor_id: Option<String>,
        #[arg(long = "name")]
        display_name: Option<String>,
        /// Force a specific distribution: auto (default), npx, uvx, binary.
        #[arg(long)]
        prefer: Option<String>,
    },
    /// Add a custom agent interactively.
    Add,
    /// Register an agent from a local JSON spec file.
    Register {
        path: PathBuf,
    },
    /// Remove a locally registered agent.
    Remove {
        actor_id: String,
    },
    Start {
        actor_id: String,
    },
    Stop {
        actor_id: String,
    },
    Log {
        actor_id: String,
        #[arg(long, default_value_t = 50)]
        tail: u32,
    },
    /// Run as the v1 external agent client: load every AgentSpec under
    /// --specs (defaults to ~/.config/joi/agents) and supervise each agent
    /// over its own server connection. Pair with `JOI_DISABLE_EMBEDDED_RUNTIME=1`
    /// on the server to disable in-process supervision.
    Serve {
        /// Override the directory of AgentSpec JSON files.
        #[arg(long)]
        specs: Option<PathBuf>,
        /// Comma-separated actor ids to load. Empty/omitted = load every
        /// AgentSpec under --specs.
        #[arg(long = "allow-actors", value_delimiter = ',')]
        allow_actors: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    render::set_output_mode(if args.json {
        OutputMode::Json
    } else {
        OutputMode::Pretty
    });
    let cfg = config::resolve(
        args.server.clone(),
        args.actor.clone(),
        args.display.clone(),
    )?;

    if let Cmd::Who = args.cmd {
        if render::is_json() {
            render::print_json(&serde_json::json!({
                "server": cfg.server_url,
                "actor": cfg.actor_id,
                "display": cfg.display_name,
                "config": config::config_path().display().to_string(),
            }));
        } else {
            println!("server   = {}", cfg.server_url);
            println!("actor    = {}", cfg.actor_id);
            println!("display  = {}", cfg.display_name);
            println!("config   = {}", config::config_path().display());
        }
        return Ok(());
    }

    // `agent serve` opens its own per-agent connections and never acts as the
    // local human actor — bypass the up-front connection_open below so we
    // don't pollute the server's actor table with an unused row.
    if let Cmd::Agent {
        sub: AgentCmd::Serve {
            specs,
            allow_actors,
        },
    } = args.cmd
    {
        return cmd::agent_serve::run(specs, cfg.server_url, allow_actors).await;
    }

    // `mcp memory` never talks to the joi server — it's spawned by the ACP
    // runtime as a stdio MCP child. Short-circuit before opening a websocket
    // so we don't wait on an online server that the agent doesn't need.
    if let Cmd::Mcp {
        sub:
            McpCmd::Memory {
                actor_id,
                profile_dir,
                shard_by,
            },
    } = &args.cmd
    {
        return cmd::mcp_memory::run(actor_id.clone(), profile_dir.clone(), shard_by.clone());
    }

    let client = Client::connect(&cfg.server_url).await?;
    client.initialize().await?;
    let _ = client
        .open_connection(&cfg.actor_id, Some(&cfg.display_name))
        .await?;

    match args.cmd {
        Cmd::Who => unreachable!(),
        Cmd::Channel { sub } => match sub {
            ChannelCmd::Create { title } => {
                cmd::channel::create(client, cfg.actor_id.clone(), title).await?
            }
            ChannelCmd::List => cmd::channel::list(client).await?,
            ChannelCmd::Invite {
                channel_id,
                actor_id,
            } => cmd::channel::invite(client, channel_id, actor_id).await?,
            ChannelCmd::Revoke {
                channel_id,
                actor_id,
            } => cmd::channel::revoke(client, channel_id, actor_id).await?,
            ChannelCmd::Members { channel_id } => cmd::channel::members(client, channel_id).await?,
        },
        Cmd::Thread { sub } => match sub {
            ThreadCmd::Create { channel, title } => {
                cmd::thread::create(client, channel, title).await?
            }
            ThreadCmd::List { channel } => cmd::thread::list(client, channel).await?,
        },
        Cmd::Say { text, r#in, reply } => {
            cmd::say::run(client, cfg.actor_id, r#in, text, reply).await?
        }
        Cmd::Handoff {
            agent,
            r#in,
            message,
        } => cmd::handoff::run(client, cfg.actor_id, agent, r#in, message).await?,
        Cmd::Action { sub } => match sub {
            ActionCmd::Accept { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, true).await?
            }
            ActionCmd::Decline { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, false).await?
            }
        },
        Cmd::Agent { sub } => match sub {
            AgentCmd::List => cmd::agent::list(client).await?,
            AgentCmd::Marketplace => cmd::agent::marketplace(client).await?,
            AgentCmd::Install {
                marketplace_id,
                local_actor_id,
                display_name,
                prefer,
            } => {
                cmd::agent::install(client, marketplace_id, local_actor_id, display_name, prefer)
                    .await?
            }
            AgentCmd::Add => cmd::agent::add(client).await?,
            AgentCmd::Register { path } => cmd::agent::register(client, path).await?,
            AgentCmd::Remove { actor_id } => cmd::agent::remove(client, actor_id).await?,
            AgentCmd::Start { actor_id } => cmd::agent::start(client, actor_id).await?,
            AgentCmd::Stop { actor_id } => cmd::agent::stop(client, actor_id).await?,
            AgentCmd::Log { actor_id, tail } => cmd::agent::log(client, actor_id, tail).await?,
            AgentCmd::Serve { .. } => unreachable!("handled before client setup"),
        },
        Cmd::Mcp { .. } => unreachable!("handled before client setup"),
        Cmd::Event { sub } => match sub {
            EventCmd::List {
                r#in,
                channel,
                limit,
                before,
            } => cmd::event::list(client, r#in, channel, limit, before).await?,
        },
        Cmd::Actor { sub } => match sub {
            ActorCmd::List => cmd::actor::list(client).await?,
        },
        Cmd::Artifact { sub } => match sub {
            ArtifactCmd::Publish {
                name,
                media_type,
                text,
                file,
            } => cmd::artifact::publish(client, cfg.actor_id, name, media_type, text, file).await?,
            ArtifactCmd::Get { id_or_uri } => cmd::artifact::get(client, id_or_uri).await?,
            ArtifactCmd::Read {
                artifact_id,
                max_bytes,
            } => cmd::artifact::read(client, artifact_id, max_bytes).await?,
        },
        Cmd::Chat { r#in, channel } => {
            let (scope_id, scope_kind) = match (r#in, channel) {
                (Some(tid), _) => (tid, proto::types::ScopeKind::Thread),
                (None, Some(cid)) => (cid, proto::types::ScopeKind::Channel),
                (None, None) => (String::new(), proto::types::ScopeKind::Thread),
            };
            cmd::chat::run(client, cfg.actor_id, scope_id, scope_kind).await?
        }
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();
}
