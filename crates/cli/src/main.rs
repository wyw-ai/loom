mod client;
mod cmd;
mod config;
mod render;
mod service;

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
    /// Send a content.add event into a thread or channel.
    Say {
        text: String,
        #[arg(long)]
        r#in: String,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
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
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
        #[arg(long, default_value = "")]
        message: String,
    },
    /// Respond to an action.request event.
    Action {
        #[command(subcommand)]
        sub: ActionCmd,
    },
    /// Manage agents. Run `joi agent example` for AgentSpec examples covering
    /// acp_stdio, command, and interactive_command transports.
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
    /// Manage the long-lived service host (am bridge, scheduler, ...). See
    /// `docs/service-plugin-system-design.md` §11.
    Service {
        #[command(subcommand)]
        sub: ServiceCmd,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceCmd {
    /// Run as the service host: load every ServiceSpec under --specs and
    /// supervise each plugin instance over its own server connection.
    /// S1 ships no built-in plugins; specs whose `kind` lacks a plugin
    /// are logged-and-skipped (see `docs/service-plugin-system-design.md`
    /// §12 phases S2/S3).
    Serve {
        /// Override the directory of ServiceSpec JSON files. Defaults
        /// to `~/.config/joi/services/` (or `$JOI_SERVICE_SPECS`).
        #[arg(long)]
        specs: Option<PathBuf>,
        /// Comma-separated spec ids to load. Empty/omitted = load every
        /// spec under --specs.
        #[arg(long = "allow-services", value_delimiter = ',')]
        allow_services: Vec<String>,
    },
    /// Validate a single ServiceSpec JSON file. Exits 0 on success and
    /// non-zero with the parsing/validation error otherwise.
    Validate { path: PathBuf },
    /// Per-message AM bridge handler. Spawned by `am listen --script
    /// "joi service am-handler --service-id <id>"` once per DingTalk
    /// message. Replaces `examples/am-joi-channel-bridge.py`.
    AmHandler {
        /// ServiceSpec id under --specs (defaults to ~/.config/joi/services/).
        #[arg(long = "service-id")]
        service_id: String,
        /// Override the specs directory.
        #[arg(long)]
        specs: Option<PathBuf>,
        /// Internal: spawned by ourselves in async_send mode. Carries
        /// the JSON payload `{sourceEvent, triggerId, scopeKind, scopeId}`.
        #[arg(long = "async-reply", hide = true)]
        async_reply: Option<String>,
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
    /// Expose pinned-announcement tools (`announcement.set` /
    /// `announcement.clear`) so an agent can publish a recap to the
    /// right-side panel of any chat scope it has write access to. Connects
    /// back to the running joi-server over WebSocket and proxies each tool
    /// call to one `event/append`.
    Announcement {
        #[arg(long = "actor-id", env = "JOI_ACTOR")]
        actor_id: String,
        /// joi-server WebSocket URL. Falls back to `JOI_SERVER`; the
        /// runtime hands this over explicitly when spawning the MCP child.
        #[arg(long = "server", env = "JOI_SERVER")]
        server: String,
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
    /// Create or update an actor row. Useful for service bridge identities.
    Upsert {
        actor_id: String,
        /// Actor kind: human, agent, or service.
        #[arg(long, default_value = "service")]
        kind: String,
        /// Display name. Defaults to actor_id.
        #[arg(long)]
        display: Option<String>,
    },
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
    /// Show AgentSpec examples for acp_stdio, command, and interactive_command transports.
    Example,
    /// Register an agent provider from a local JSON spec file.
    Register {
        path: PathBuf,
    },
    /// Remove a locally registered agent provider.
    Remove {
        provider_id: String,
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
    /// Run as the v1 external agent client: load every AgentProviderSpec under
    /// --specs (defaults to ~/.config/joi/agents) and supervise each agent
    /// over its own server connection.
    Serve {
        /// Override the directory of AgentProviderSpec JSON files.
        #[arg(long)]
        specs: Option<PathBuf>,
        /// Comma-separated actor ids to load. Empty/omitted = load every
        /// actor from every AgentProviderSpec under --specs.
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

    // Agent registry management is local to `joi agent serve`; the server is
    // only the message bus.
    if let Cmd::Agent { sub } = args.cmd {
        match sub {
            AgentCmd::List => cmd::agent::list()?,
            AgentCmd::Marketplace => cmd::agent::marketplace()?,
            AgentCmd::Install {
                marketplace_id,
                local_actor_id,
                display_name,
                prefer,
            } => cmd::agent::install(marketplace_id, local_actor_id, display_name, prefer)?,
            AgentCmd::Add => cmd::agent::add()?,
            AgentCmd::Example => cmd::agent::example(),
            AgentCmd::Register { path } => cmd::agent::register(path)?,
            AgentCmd::Remove { provider_id } => cmd::agent::remove(provider_id)?,
            AgentCmd::Start { actor_id } => cmd::agent::start(actor_id)?,
            AgentCmd::Stop { actor_id } => cmd::agent::stop(actor_id)?,
            AgentCmd::Log { actor_id, tail } => cmd::agent::log(actor_id, tail)?,
            AgentCmd::Serve { .. } => unreachable!("handled above"),
        }
        return Ok(());
    }

    // `service serve` follows the same shape as `agent serve`: it opens
    // its own per-service connections (one per ServiceSpec, bound to the
    // service actor) and must not pollute the actor table with a human
    // entry. Same early-exit pattern.
    if let Cmd::Service {
        sub: ServiceCmd::Serve {
            specs,
            allow_services,
        },
    } = args.cmd
    {
        return cmd::service::serve(specs, cfg.server_url, allow_services).await;
    }

    // `service validate` is offline — no server contact needed.
    if let Cmd::Service {
        sub: ServiceCmd::Validate { path },
    } = &args.cmd
    {
        return cmd::service::validate(path.clone());
    }

    // `service am-handler` opens its own connection bound to the AM
    // service actor; bypass the human-actor `connection/open` below
    // (would otherwise pollute the actor table and fight the
    // §9.4 preempt rule).
    if let Cmd::Service {
        sub:
            ServiceCmd::AmHandler {
                service_id,
                specs,
                async_reply,
            },
    } = args.cmd
    {
        return cmd::service::am_handler(cfg.server_url, service_id, specs, async_reply).await;
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

    // `mcp announcement` does talk to the server, but it must bind as the
    // *agent's* actor (so ACL gates apply correctly), not as the operator
    // running the binary. Short-circuit before the generic
    // `connection/open` below so the agent's connection isn't shadowed by
    // a human-actor binding.
    if let Cmd::Mcp {
        sub: McpCmd::Announcement { actor_id, server },
    } = &args.cmd
    {
        return cmd::mcp_announcement::run(actor_id.clone(), server.clone()).await;
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
        Cmd::Say {
            text,
            r#in,
            channel,
            reply,
        } => cmd::say::run(client, cfg.actor_id, r#in, channel, text, reply).await?,
        Cmd::Handoff {
            agent,
            r#in,
            channel,
            message,
        } => cmd::handoff::run(client, cfg.actor_id, agent, r#in, channel, message).await?,
        Cmd::Action { sub } => match sub {
            ActionCmd::Accept { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, true).await?
            }
            ActionCmd::Decline { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, false).await?
            }
        },
        Cmd::Agent { .. } => unreachable!("handled before client setup"),
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
            ActorCmd::Upsert {
                actor_id,
                kind,
                display,
            } => cmd::actor::upsert(client, actor_id, kind, display).await?,
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
        Cmd::Service { .. } => unreachable!("handled before client setup"),
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
