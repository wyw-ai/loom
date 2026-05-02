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
    /// Read/write files under a scope's workspace directory. Local-only —
    /// no server contact. See `docs/remove-dev-helper-migration-design.md`
    /// §4.1.
    Workspace {
        #[command(subcommand)]
        sub: WorkspaceCmd,
    },
}

#[derive(Subcommand, Debug)]
enum WorkspaceCmd {
    /// Print the resolved absolute path of the workspace (or a sub-path).
    Path {
        #[command(flatten)]
        target: WsTarget,
        /// Optional sub-path inside the workspace.
        sub: Option<String>,
    },
    /// Show metadata about the workspace (kind, ids, path, exists).
    Info {
        #[command(flatten)]
        target: WsTarget,
    },
    /// List entries in the workspace (or a sub-directory).
    List {
        #[command(flatten)]
        target: WsTarget,
        sub: Option<String>,
        /// List recursively (relative paths only).
        #[arg(long, short = 'r')]
        recursive: bool,
    },
    /// Read a file from the workspace to stdout.
    Read {
        #[command(flatten)]
        target: WsTarget,
        path: String,
        /// Cap the read at N bytes (default 4 MiB).
        #[arg(long, default_value_t = 4 * 1024 * 1024)]
        max_bytes: u64,
    },
    /// Write a file into the workspace. Body comes from --text, --file,
    /// or stdin (in that priority).
    Write {
        #[command(flatten)]
        target: WsTarget,
        path: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        /// Append to an existing file instead of replacing.
        #[arg(long)]
        append: bool,
    },
    /// Remove a file (or directory with --recursive).
    Rm {
        #[command(flatten)]
        target: WsTarget,
        path: String,
        #[arg(long, short = 'r')]
        recursive: bool,
    },
}

#[derive(clap::Args, Debug)]
struct WsTarget {
    /// Channel id. Required for channel-shared, and for actor workspaces.
    #[arg(long)]
    channel: Option<String>,
    /// Thread id. Required for thread-shared workspaces.
    #[arg(long = "in")]
    thread: Option<String>,
    /// Actor id (defaults to JOI_ACTOR / current actor). Use --actor to
    /// explicitly target a per-actor workspace.
    #[arg(long, env = "JOI_ACTOR")]
    actor: Option<String>,
    /// Target the channel-shared area (`channels/<cid>/shared/`).
    #[arg(long, conflicts_with_all = ["thread_shared", "actor_ws"])]
    channel_shared: bool,
    /// Target the thread-shared area (`channels/<cid>/threads/<tid>/shared/`).
    #[arg(long, conflicts_with_all = ["channel_shared", "actor_ws"])]
    thread_shared: bool,
    /// Target the per-actor workspace (default if --actor given).
    #[arg(long = "actor-ws", conflicts_with_all = ["channel_shared", "thread_shared"])]
    actor_ws: bool,
}

impl WsTarget {
    fn into_ref(self, default_actor: &str) -> Result<cmd::workspace::WsRef> {
        use cmd::workspace::{WsKind, WsRef};
        let kind = if self.channel_shared {
            WsKind::Channel
        } else if self.thread_shared {
            WsKind::Thread
        } else {
            // Default: actor workspace.
            WsKind::Actor
        };
        let channel_id = match (&self.channel, &self.thread, kind) {
            (Some(c), _, _) => c.clone(),
            (None, Some(_t), WsKind::Thread) => {
                anyhow::bail!("--in <tid> requires --channel <cid> too (thread workspaces are nested under their channel)");
            }
            _ => anyhow::bail!("--channel <cid> is required"),
        };
        let actor_id = self.actor.clone().or_else(|| {
            if matches!(kind, WsKind::Actor) {
                Some(default_actor.to_string())
            } else {
                None
            }
        });
        Ok(WsRef {
            kind,
            channel_id,
            thread_id: self.thread.clone(),
            actor_id,
        })
    }
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
    /// Inspect ServiceSpec JSON files on disk (no server contact).
    Spec {
        #[command(subcommand)]
        sub: ServiceSpecCmd,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceSpecCmd {
    /// List every ServiceSpec under the specs directory.
    List,
    /// Print one ServiceSpec by id (secrets redacted unless --raw).
    Get {
        service_id: String,
        #[arg(long)]
        raw: bool,
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
        /// Record this thread under `resident_threads.<role>` in the
        /// channel-shared scope.json so a router can address it by role.
        /// See design §4.7.1.
        #[arg(long = "resident-as")]
        resident_as: Option<String>,
        /// Read the artifact (id or `artifact://...` URI), derive a
        /// `mounts[]` array, and write it to the thread-shared
        /// scope.json so per-actor workspaces seed mounts on first
        /// dispatch. See design §4.7.2.
        #[arg(long = "bootstrap-artifact")]
        bootstrap_artifact: Option<String>,
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
    /// Alias for `event list` — kept for parity with the design doc and
    /// for the migrated services that prefer the `query` verb.
    Query {
        #[arg(long)]
        r#in: String,
        #[arg(long)]
        channel: bool,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        before: Option<String>,
    },
    /// Append an arbitrary event to a scope. Use --reply / --handoff /
    /// --artifact-link to attach the corresponding relations; use --text
    /// / --file / --stdin to provide the payload body. `joi say` /
    /// `joi handoff` remain as ergonomic shortcuts for content.add.
    Append {
        /// Scope id (thread id by default; pass --channel to write into a channel scope).
        #[arg(long)]
        r#in: String,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
        /// Event type discriminator (e.g. content.add, status.update).
        #[arg(long = "type", default_value = "content.add")]
        event_type: String,
        /// payload.contentType. Defaults to text/markdown to match
        /// content.add's convention; ignored if no body is supplied.
        #[arg(long = "content-type", default_value = "text/markdown")]
        content_type: String,
        /// Event body as inline text.
        #[arg(long)]
        text: Option<String>,
        /// Event body read from this file.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Event body read from stdin.
        #[arg(long)]
        stdin: bool,
        /// Add a `replies_to` relation pointing at this event id.
        #[arg(long = "reply")]
        reply: Option<String>,
        /// Add a `hands_off_to` relation pointing at this actor id.
        #[arg(long = "handoff")]
        handoff: Option<String>,
        /// Add one or more `links` relations targeting an artifact
        /// (`art_…` or `artifact://…`). May be repeated.
        #[arg(long = "artifact-link")]
        artifact_link: Vec<String>,
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
    /// over its own server connection.
    Serve {
        /// Override the directory of AgentSpec JSON files.
        #[arg(long)]
        specs: Option<PathBuf>,
        /// Comma-separated actor ids to load. Empty/omitted = load every
        /// AgentSpec under --specs.
        #[arg(long = "allow-actors", value_delimiter = ',')]
        allow_actors: Vec<String>,
    },
    /// Inspect AgentSpec JSON files on disk (no server contact).
    Spec {
        #[command(subcommand)]
        sub: AgentSpecCmd,
    },
    /// Inspect an installed agent's bundle directory (skills/, tools/, ...).
    Bundle {
        #[command(subcommand)]
        sub: AgentBundleCmd,
    },
}

#[derive(Subcommand, Debug)]
enum AgentSpecCmd {
    /// List every AgentSpec under the specs directory.
    List,
    /// Print one AgentSpec by actor id (secrets redacted unless --raw).
    Get {
        actor_id: String,
        /// Print the raw JSON without redacting token/secret/password fields.
        #[arg(long)]
        raw: bool,
    },
}

#[derive(Subcommand, Debug)]
enum AgentBundleCmd {
    /// Show bundle dir + optionally list/read a file inside it.
    Get {
        actor_id: String,
        /// Read this file (relative to the bundle dir) and print to stdout.
        #[arg(long)]
        file: Option<String>,
        /// List bundle contents (recursive) instead of just printing the dir.
        #[arg(long)]
        list: bool,
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
            AgentCmd::Remove { actor_id } => cmd::agent::remove(actor_id)?,
            AgentCmd::Start { actor_id } => cmd::agent::start(actor_id)?,
            AgentCmd::Stop { actor_id } => cmd::agent::stop(actor_id)?,
            AgentCmd::Log { actor_id, tail } => cmd::agent::log(actor_id, tail)?,
            AgentCmd::Serve { .. } => unreachable!("handled above"),
            AgentCmd::Spec { sub } => match sub {
                AgentSpecCmd::List => cmd::spec::agent_list()?,
                AgentSpecCmd::Get { actor_id, raw } => cmd::spec::agent_get(actor_id, raw)?,
            },
            AgentCmd::Bundle { sub } => match sub {
                AgentBundleCmd::Get {
                    actor_id,
                    file,
                    list,
                } => cmd::spec::bundle_get(actor_id, file, list)?,
            },
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

    // `agent spec` / `agent bundle` / `service spec` are local-only —
    // they read AgentSpec/ServiceSpec JSON files and bundle directories
    // off the operator's disk. Short-circuit before the websocket dance
    // so they work even when no joi-server is running.
    if let Cmd::Service {
        sub: ServiceCmd::Spec { sub },
    } = args.cmd
    {
        return match sub {
            ServiceSpecCmd::List => cmd::spec::service_list(),
            ServiceSpecCmd::Get { service_id, raw } => cmd::spec::service_get(service_id, raw),
        };
    }

    // Workspace commands operate purely on the local filesystem layout
    // shared with `agent serve` (see `cmd::workspace`). Never contact the
    // server.
    if let Cmd::Workspace { sub } = args.cmd {
        let actor_default = cfg.actor_id.clone();
        return match sub {
            WorkspaceCmd::Path { target, sub } => {
                let ws = target.into_ref(&actor_default)?;
                cmd::workspace::path(ws, sub)
            }
            WorkspaceCmd::Info { target } => {
                let ws = target.into_ref(&actor_default)?;
                cmd::workspace::info(ws)
            }
            WorkspaceCmd::List {
                target,
                sub,
                recursive,
            } => {
                let ws = target.into_ref(&actor_default)?;
                cmd::workspace::list(ws, sub, recursive)
            }
            WorkspaceCmd::Read {
                target,
                path,
                max_bytes,
            } => {
                let ws = target.into_ref(&actor_default)?;
                cmd::workspace::read(ws, path, max_bytes)
            }
            WorkspaceCmd::Write {
                target,
                path,
                text,
                file,
                append,
            } => {
                let ws = target.into_ref(&actor_default)?;
                cmd::workspace::write(ws, path, text, file, append)
            }
            WorkspaceCmd::Rm {
                target,
                path,
                recursive,
            } => {
                let ws = target.into_ref(&actor_default)?;
                cmd::workspace::rm(ws, path, recursive)
            }
        };
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
            ThreadCmd::Create {
                channel,
                title,
                resident_as,
                bootstrap_artifact,
            } => {
                cmd::thread::create(client, channel, title, resident_as, bootstrap_artifact)
                    .await?
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
            EventCmd::Query {
                r#in,
                channel,
                limit,
                before,
            } => cmd::event::list(client, r#in, channel, limit, before).await?,
            EventCmd::Append {
                r#in,
                channel,
                event_type,
                content_type,
                text,
                file,
                stdin,
                reply,
                handoff,
                artifact_link,
            } => {
                cmd::event::append(
                    client,
                    cmd::event::AppendArgs {
                        actor_id: cfg.actor_id,
                        scope_id: r#in,
                        is_channel: channel,
                        event_type,
                        content_type,
                        text,
                        file,
                        stdin,
                        reply_to: reply,
                        handoff_to: handoff,
                        artifact_links: artifact_link,
                    },
                )
                .await?
            }
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
        Cmd::Workspace { .. } => unreachable!("handled before client setup"),
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
