mod client;
mod cmd;
mod config;
mod daemon_ipc;
mod render;
mod service;

use std::path::PathBuf;

use anyhow::{Context, Result};
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
    /// Hand off the turn to an actor in a thread or channel.
    Handoff {
        /// Target actor id; omit to pick from a list of registered agents/humans.
        agent: Option<String>,
        /// Internal thread/channel scope id. Prefer --target when you have a channel/root event target.
        #[arg(long)]
        r#in: Option<String>,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
        /// Canonical destination target: #<channel_id> or #<channel_id>:<root_event_id>.
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value = "")]
        message: String,
    },
    /// Message commands using the canonical #channel/#channel:root-event/dm:actor grammar.
    Message {
        #[command(subcommand)]
        sub: MessageCmd,
    },
    /// Respond to an action.request event.
    Action {
        #[command(subcommand)]
        sub: ActionCmd,
    },
    /// Create, claim, update, and delegate message-anchored tasks.
    Task {
        #[command(subcommand)]
        sub: TaskCmd,
    },
    /// Ask the triggering human to choose or provide input, then return the answer to this process.
    AskUserQuestion {
        /// Max seconds to wait for action.response.
        #[arg(long = "timeout-seconds")]
        timeout_seconds: Option<u64>,
        /// Scope id. Defaults to JOI_SCOPE_ID inside daemon-managed agent turns.
        #[arg(long)]
        r#in: Option<String>,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
        /// Actor id that should answer. Defaults to JOI_TRIGGER_ACTOR inside daemon turns.
        #[arg(long)]
        to: Option<String>,
        /// Turn id to associate with the action.request. Defaults to JOI_TURN_ID.
        #[arg(long = "turn-id")]
        turn_id: Option<String>,
        /// Short title shown in clients. JSON stdin may also provide header/title.
        #[arg(long)]
        title: Option<String>,
        /// Question text. If omitted, read the full question payload as JSON from stdin.
        #[arg(long)]
        question: Option<String>,
        /// Choice formatted as id=label. Repeat for multiple choices.
        #[arg(long = "choice")]
        choices: Vec<String>,
        /// Mark the request as allowing free-form text for clients that support it.
        #[arg(long = "allow-freeform")]
        allow_freeform: bool,
    },
    /// Ask the triggering human to approve or reject a proposed action, then return the result.
    RequestApproval {
        /// Max seconds to wait for action.response.
        #[arg(long = "timeout-seconds")]
        timeout_seconds: Option<u64>,
        /// Scope id. Defaults to JOI_SCOPE_ID inside daemon-managed agent turns.
        #[arg(long)]
        r#in: Option<String>,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
        /// Actor id that should approve. Defaults to JOI_TRIGGER_ACTOR inside daemon turns.
        #[arg(long)]
        to: Option<String>,
        /// Turn id to associate with the action.request. Defaults to JOI_TURN_ID.
        #[arg(long = "turn-id")]
        turn_id: Option<String>,
        /// Short title shown in clients. JSON stdin may also provide title.
        #[arg(long)]
        title: Option<String>,
        /// What the agent wants approval to do. If omitted, read JSON from stdin.
        #[arg(long)]
        reason: Option<String>,
        /// Label for the approving option.
        #[arg(long = "approve-label")]
        approve_label: Option<String>,
        /// Label for the rejecting option.
        #[arg(long = "reject-label")]
        reject_label: Option<String>,
    },
    /// Inspect daemon-configured agents. Runtime hosting is done by `joi daemon`.
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
    /// Upload or download message attachments.
    Attachment {
        #[command(subcommand)]
        sub: AttachmentCmd,
    },
    /// Schedule and manage reminders.
    Reminder {
        #[command(subcommand)]
        sub: ReminderCmd,
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
    /// Run the machine-scoped daemon: auto-detect supported local agent CLIs
    /// and host agents configured on the selected machine.
    Daemon {
        /// Machine id from the desktop machine config. Defaults to the active
        /// workspace's first machine.
        #[arg(long = "machine-id", env = "JOI_MACHINE_ID")]
        machine_id: Option<String>,
        /// Override the daemon data root. Defaults to the machine data root.
        #[arg(long = "data-root", env = "JOI_AGENT_DATA_ROOT")]
        data_root: Option<PathBuf>,
        /// Comma-separated actor ids to load. Empty/omitted = load every
        /// agent configured on the machine.
        #[arg(long = "allow-actors", value_delimiter = ',')]
        allow_actors: Vec<String>,
        /// Print auto-detected local providers and exit.
        #[arg(long = "list-providers")]
        list_providers: bool,
        /// Override the directory of ServiceSpec JSON files loaded by the
        /// daemon. Defaults to `~/.config/joi/services/` or
        /// `$JOI_SERVICE_SPECS`.
        #[arg(long = "services")]
        services: Option<PathBuf>,
        /// Comma-separated service ids to load through the daemon. Empty or
        /// omitted means load every ServiceSpec under --services.
        #[arg(long = "allow-services", value_delimiter = ',')]
        allow_services: Vec<String>,
        /// Do not start the service host from this daemon.
        #[arg(long = "no-services")]
        no_services: bool,
        /// Unix socket used by local `joi` CLI clients to reach this daemon.
        #[arg(long = "socket", env = "JOI_DAEMON_SOCKET")]
        socket: Option<PathBuf>,
        /// Do not expose the local daemon IPC socket. Useful for tests where
        /// agents connect to the server directly.
        #[arg(long = "no-ipc")]
        no_ipc: bool,
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
        /// Channel-scope event that anchors the thread.
        #[arg(long = "root-event")]
        root_event: String,
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
enum TaskCmd {
    /// Create a task anchored to a top-level channel event.
    Create {
        #[arg(long = "source-event")]
        source_event: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// List visible tasks.
    List {
        #[arg(long)]
        channel: Option<String>,
        #[arg(long = "source-event")]
        source_event: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long = "status", value_delimiter = ',')]
        statuses: Vec<String>,
    },
    /// Show one task with assignment results.
    Show { task_id: String },
    /// Update status, owner, result summary, or attached artifact ids.
    Update {
        task_id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        result: Option<String>,
        #[arg(long = "artifact-id")]
        artifact_ids: Vec<String>,
    },
    /// Create an assignment and hand it off in the task's canonical thread.
    Assign {
        task_id: String,
        #[arg(long)]
        to: String,
        #[arg(long = "type", default_value = "other")]
        assignment_type: String,
        #[arg(long)]
        instruction: String,
    },
    /// Update a task assignment result.
    Assignment {
        #[command(subcommand)]
        sub: TaskAssignmentCmd,
    },
}

#[derive(Subcommand, Debug)]
enum TaskAssignmentCmd {
    Update {
        assignment_id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long = "result-event")]
        result_event: Option<String>,
        #[arg(long)]
        result: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum MessageCmd {
    /// Send a message to #channel, #channel:root-event, or dm:actor.
    Send {
        #[arg(long)]
        target: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long = "attachment-id")]
        attachment_ids: Vec<String>,
    },
    /// Read messages from #channel, #channel:root-event, or dm:actor.
    Read {
        #[arg(long)]
        target: String,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        before: Option<String>,
    },
    /// Drain this actor's pending directed inbox.
    Check {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long = "no-ack")]
        no_ack: bool,
    },
    /// Search visible message text.
    Search {
        #[arg(long)]
        query: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
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
    /// Delete an actor row from the server registry.
    Delete { actor_id: String },
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
        /// Optional scope id to associate with the artifact.
        #[arg(long)]
        r#in: Option<String>,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
    },
    /// Fetch artifact metadata by id (`art_…`) or by `artifact://` uri.
    Get { id_or_uri: String },
    /// Print artifact body (text only). Use --offset/--max-bytes for progressive reads.
    Read {
        artifact_id: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 65536)]
        max_bytes: u64,
    },
}

#[derive(Subcommand, Debug)]
enum AttachmentCmd {
    /// Upload a file and return an attachment/artifact id.
    Upload {
        #[arg(long)]
        target: String,
        #[arg(long)]
        path: PathBuf,
        #[arg(long = "mime-type")]
        mime_type: Option<String>,
    },
    /// Download an attachment/artifact body to a local file.
    View {
        #[arg(long)]
        id: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 10 * 1024 * 1024)]
        max_bytes: u64,
    },
    /// Download a whole attachment/artifact body in chunks.
    Download {
        #[arg(long)]
        id: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long = "chunk-bytes", default_value_t = 1024 * 1024)]
        chunk_bytes: u64,
    },
}

#[derive(Subcommand, Debug)]
enum ReminderCmd {
    Schedule {
        #[arg(long)]
        title: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long = "msg-id")]
        msg_id: Option<String>,
        #[arg(long = "delay-seconds")]
        delay_seconds: Option<i64>,
        #[arg(long = "fire-at")]
        fire_at: Option<String>,
        #[arg(long)]
        repeat: Option<String>,
    },
    List {
        #[arg(long = "status", value_delimiter = ',')]
        status: Vec<String>,
        #[arg(long)]
        all: bool,
    },
    Cancel {
        #[arg(long)]
        id: String,
    },
    Snooze {
        #[arg(long)]
        id: String,
        #[arg(long)]
        by: String,
    },
    Update {
        #[arg(long)]
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long = "in")]
        in_after: Option<String>,
        #[arg(long = "fire-at")]
        fire_at: Option<String>,
        #[arg(long = "cadence")]
        repeat: Option<String>,
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
    /// List agents configured for daemon-managed machines.
    List,
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

    // Agent runtime hosting is daemon-only. `joi agent` is kept as an offline
    // inspection namespace so it doesn't open an unused human connection.
    if let Cmd::Agent { sub } = args.cmd {
        match sub {
            AgentCmd::List => cmd::agent::list()?,
        }
        return Ok(());
    }

    // `service serve` follows the daemon worker shape: it opens
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

    if let Cmd::Daemon {
        machine_id,
        data_root,
        allow_actors,
        list_providers,
        services,
        allow_services,
        no_services,
        socket,
        no_ipc,
    } = args.cmd
    {
        return cmd::daemon::run(
            machine_id,
            data_root,
            allow_actors,
            list_providers,
            services,
            allow_services,
            no_services,
            socket,
            no_ipc,
            cfg.server_url,
        )
        .await;
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

    let client = connect_client(&cfg.server_url, explicit_server_arg_present()).await?;
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
                root_event,
                title,
            } => cmd::thread::create(client, channel, root_event, title).await?,
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
            target,
            message,
        } => cmd::handoff::run(client, cfg.actor_id, agent, r#in, channel, target, message).await?,
        Cmd::Message { sub } => match sub {
            MessageCmd::Send {
                target,
                text,
                attachment_ids,
            } => cmd::message::send(client, cfg.actor_id, target, text, attachment_ids).await?,
            MessageCmd::Read {
                target,
                limit,
                before,
            } => cmd::message::read(client, cfg.actor_id, target, limit, before).await?,
            MessageCmd::Check { limit, no_ack } => {
                cmd::message::check(client, cfg.actor_id, limit, !no_ack).await?
            }
            MessageCmd::Search {
                query,
                target,
                limit,
            } => cmd::message::search(client, cfg.actor_id, query, target, limit).await?,
        },
        Cmd::Action { sub } => match sub {
            ActionCmd::Accept { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, true).await?
            }
            ActionCmd::Decline { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, false).await?
            }
        },
        Cmd::Task { sub } => match sub {
            TaskCmd::Create {
                source_event,
                title,
                description,
                owner,
                status,
            } => {
                cmd::task::create(
                    client,
                    cfg.actor_id,
                    source_event,
                    title,
                    description,
                    owner,
                    status,
                )
                .await?
            }
            TaskCmd::List {
                channel,
                source_event,
                owner,
                statuses,
            } => cmd::task::list(client, channel, source_event, owner, statuses).await?,
            TaskCmd::Show { task_id } => cmd::task::show(client, task_id).await?,
            TaskCmd::Update {
                task_id,
                status,
                owner,
                result,
                artifact_ids,
            } => cmd::task::update(client, task_id, status, owner, result, artifact_ids).await?,
            TaskCmd::Assign {
                task_id,
                to,
                assignment_type,
                instruction,
            } => {
                cmd::task::assign(
                    client,
                    cfg.actor_id,
                    task_id,
                    to,
                    assignment_type,
                    instruction,
                )
                .await?
            }
            TaskCmd::Assignment { sub } => match sub {
                TaskAssignmentCmd::Update {
                    assignment_id,
                    status,
                    result_event,
                    result,
                } => {
                    cmd::task::assignment_update(
                        client,
                        assignment_id,
                        status,
                        result_event,
                        result,
                    )
                    .await?
                }
            },
        },
        Cmd::AskUserQuestion {
            timeout_seconds,
            r#in,
            channel,
            to,
            turn_id,
            title,
            question,
            choices,
            allow_freeform,
        } => {
            cmd::ask_user_question::run(
                client,
                cfg.actor_id,
                cmd::ask_user_question::RunArgs {
                    timeout_seconds,
                    scope_id: r#in,
                    is_channel: channel,
                    to,
                    turn_id,
                    title,
                    question,
                    choices,
                    allow_freeform,
                },
            )
            .await?
        }
        Cmd::RequestApproval {
            timeout_seconds,
            r#in,
            channel,
            to,
            turn_id,
            title,
            reason,
            approve_label,
            reject_label,
        } => {
            cmd::ask_user_question::run_approval(
                client,
                cfg.actor_id,
                cmd::ask_user_question::ApprovalArgs {
                    timeout_seconds,
                    scope_id: r#in,
                    is_channel: channel,
                    to,
                    turn_id,
                    title,
                    reason,
                    approve_label,
                    reject_label,
                },
            )
            .await?
        }
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
            ActorCmd::Delete { actor_id } => cmd::actor::delete(client, actor_id).await?,
        },
        Cmd::Artifact { sub } => match sub {
            ArtifactCmd::Publish {
                name,
                media_type,
                text,
                file,
                r#in,
                channel,
            } => {
                cmd::artifact::publish(
                    client,
                    cfg.actor_id,
                    name,
                    media_type,
                    text,
                    file,
                    r#in,
                    channel,
                )
                .await?
            }
            ArtifactCmd::Get { id_or_uri } => cmd::artifact::get(client, id_or_uri).await?,
            ArtifactCmd::Read {
                artifact_id,
                offset,
                max_bytes,
            } => cmd::artifact::read(client, artifact_id, offset, max_bytes).await?,
        },
        Cmd::Attachment { sub } => match sub {
            AttachmentCmd::Upload {
                target,
                path,
                mime_type,
            } => cmd::attachment::upload(client, cfg.actor_id, target, path, mime_type).await?,
            AttachmentCmd::View {
                id,
                output,
                offset,
                max_bytes,
            } => cmd::attachment::view(client, id, output, offset, max_bytes).await?,
            AttachmentCmd::Download {
                id,
                output,
                chunk_bytes,
            } => cmd::attachment::download(client, id, output, chunk_bytes).await?,
        },
        Cmd::Reminder { sub } => match sub {
            ReminderCmd::Schedule {
                title,
                target,
                msg_id,
                delay_seconds,
                fire_at,
                repeat,
            } => {
                cmd::reminder::schedule(
                    client,
                    cfg.actor_id,
                    title,
                    target,
                    msg_id,
                    delay_seconds,
                    fire_at,
                    repeat,
                )
                .await?
            }
            ReminderCmd::List { status, all } => {
                cmd::reminder::list(client, cfg.actor_id, status, all).await?
            }
            ReminderCmd::Cancel { id } => cmd::reminder::cancel(client, cfg.actor_id, id).await?,
            ReminderCmd::Snooze { id, by } => {
                cmd::reminder::snooze(client, cfg.actor_id, id, by).await?
            }
            ReminderCmd::Update {
                id,
                title,
                in_after,
                fire_at,
                repeat,
            } => {
                cmd::reminder::update(client, cfg.actor_id, id, title, in_after, fire_at, repeat)
                    .await?
            }
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
        Cmd::Daemon { .. } => unreachable!("handled before client setup"),
    }
    Ok(())
}

async fn connect_client(
    server_url: &str,
    explicit_server_arg: bool,
) -> Result<std::sync::Arc<Client>> {
    if !explicit_server_arg && !daemon_ipc::daemon_disabled() {
        if let Some(socket) = daemon_ipc::resolve_socket() {
            let server_matches = socket
                .server_url
                .as_deref()
                .map(|url| url == server_url)
                .unwrap_or(socket.source == daemon_ipc::SocketSource::Env);
            if server_matches {
                match Client::connect_daemon_socket(&socket.path).await {
                    Ok(client) => return Ok(client),
                    Err(err) if socket.source == daemon_ipc::SocketSource::Discovery => {
                        tracing::warn!(
                            error = %err,
                            socket = %socket.path.display(),
                            "daemon socket unavailable; falling back to server URL"
                        );
                    }
                    Err(err) => {
                        return Err(err).with_context(|| {
                            format!("connect daemon socket {}", socket.path.display())
                        });
                    }
                }
            }
        }
    }

    Client::connect(server_url).await
}

fn explicit_server_arg_present() -> bool {
    std::env::args_os().skip(1).any(|arg| {
        arg == "--server"
            || arg
                .to_str()
                .map(|value| value.starts_with("--server="))
                .unwrap_or(false)
    })
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
