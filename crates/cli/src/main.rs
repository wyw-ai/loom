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
        /// Prompt prefix to place at the very beginning of the receiver's
        /// final provider prompt for this handoff only.
        #[arg(long = "handoff-prefix")]
        handoff_prefix: Option<String>,
        #[arg(long, short = 'm', default_value = "")]
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
    /// Manage per-actor memory records using the same store as MCP memory.
    Memory {
        #[command(subcommand)]
        sub: MemoryCmd,
    },
    /// Run joi as a stdio MCP server. Typically not invoked by humans —
    /// the runtime auto-injects this as a `session/new.mcpServers` entry
    /// when an agent's spec opts into `memory.delivery.mcp`.
    Mcp {
        #[command(subcommand)]
        sub: McpCmd,
    },
    /// Apply a lesson-plan to an on-disk AgentSpec / ServiceSpec
    /// after `approval.spec_apply` has been accepted.
    Spec {
        #[command(subcommand)]
        sub: SpecCmd,
    },
    /// Manage the long-lived service host (am bridge, scheduler, ...). See
    /// `docs/service-plugin-system-design.md` §11.
    Service {
        #[command(subcommand)]
        sub: ServiceCmd,
    },
    /// Read/write files under a scope's workspace directory. Local-only —
    /// no server contact.
    Workspace {
        #[command(subcommand)]
        sub: WorkspaceCmd,
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
enum SpecCmd {
    /// Apply the lesson-plan attached to the action.request that was
    /// accepted by the given `action.response` event. See
    /// `crates/cli/src/cmd/spec_apply.rs` for the frontmatter contract.
    Apply {
        /// Event id of the `action.response` (kind = accepted) that
        /// approved the lesson-plan.
        #[arg(long = "action")]
        action_event_id: String,
        /// Don't write spec/bundle files or bump the reload epoch.
        /// Prints the planned changes and exits.
        #[arg(long)]
        dry_run: bool,
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
    /// Bump the reload-epoch marker for `service_id` so a running
    /// `joi service serve` host re-reads the ServiceSpec and respawns
    /// the supervised plugin instance(s). See design §7.1.
    Reload { service_id: String },
    /// Inspect ServiceSpec JSON files on disk (no server contact).
    Spec {
        #[command(subcommand)]
        sub: ServiceSpecCmd,
    },
    /// Start a `lifecycle = thread_bound` instance by writing a
    /// per-instance `request.json`. A running `joi service serve`
    /// host watches the spec's `instances/` directory and dispatches
    /// the plugin task on observation. See design §4.7.3.
    Start {
        /// ServiceSpec id (must declare `lifecycle = thread_bound`).
        #[arg(long = "spec")]
        spec_id: String,
        /// Thread id to bind the instance to. v1's only supported
        /// `bind.scope` is `thread`, so this becomes the instance id.
        #[arg(long = "in")]
        thread: String,
        /// Optional channel id of the thread. Recorded in the request
        /// so the host can resolve `{channel.id}` placeholders.
        #[arg(long = "channel")]
        channel: Option<String>,
        /// JSON object validated against the spec's `params_schema`.
        /// Defaults to `{}`.
        #[arg(long = "params")]
        params: Option<String>,
        /// Override the specs directory (used to look up the spec for
        /// validation). Defaults to `~/.config/joi/services/`.
        #[arg(long)]
        specs: Option<PathBuf>,
    },
    /// Stop a thread-bound instance by removing its `request.json`.
    /// The host watcher tears down the plugin task on the next poll.
    Stop {
        #[arg(long = "spec")]
        spec_id: String,
        #[arg(long = "in")]
        thread: String,
    },
    /// List active thread-bound instances. With `--spec`, scopes the
    /// listing to a single ServiceSpec; without it, every spec under
    /// the host data root is walked.
    Status {
        #[arg(long = "spec")]
        spec_id: Option<String>,
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
enum MemoryCmd {
    Query {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        channel: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long = "include-non-accepted")]
        include_non_accepted: bool,
        #[arg(long = "profile-dir")]
        profile_dir: Option<PathBuf>,
    },
    Append {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        summary: String,
        #[arg(long = "source-channel")]
        source_channel: String,
        #[arg(long = "source-event")]
        source_event: Option<String>,
        #[arg(long, default_value = "pending")]
        status: String,
        #[arg(long = "type", default_value = "note")]
        record_type: String,
        #[arg(long, default_value = "medium")]
        confidence: String,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "profile-dir")]
        profile_dir: Option<PathBuf>,
    },
    Get {
        #[arg(long)]
        actor: String,
        memory_id: String,
        #[arg(long = "profile-dir")]
        profile_dir: Option<PathBuf>,
    },
    Update {
        #[arg(long)]
        actor: String,
        memory_id: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "source-event")]
        source_event: Option<String>,
        #[arg(long = "profile-dir")]
        profile_dir: Option<PathBuf>,
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
        /// Show the channel archive box instead of active threads.
        #[arg(long)]
        archived: bool,
    },
    /// Move a thread to its channel archive box.
    Archive { thread_id: String },
    /// Restore a thread from its channel archive box.
    #[command(alias = "restore", alias = "revert")]
    Unarchive { thread_id: String },
    /// List a channel's archive box, newest archived first.
    ArchiveList {
        #[arg(long)]
        channel: String,
    },
    /// Delete a thread by id. Thread-bound services watching this
    /// thread (`bind.auto_stop_on=["thread.closed"]`, §4.7.3) reap
    /// their instances on the next watcher tick.
    Delete { thread_id: String },
    /// Bootstrap an *existing* thread from a clone-manifest (or explicit
    /// mounts) artifact. Used when an already-open thread (e.g. a
    /// bug-fix loop's bugfix thread) needs target/reference repo
    /// worktrees added without creating a new thread. Writes
    /// `scope.json.mounts` on the thread-shared scope so per-actor
    /// `agent serve` workspaces seed mounts on next ensure_scope.
    Bootstrap {
        /// Thread id to bootstrap.
        #[arg(long = "in")]
        thread_id: String,
        /// Channel id the thread belongs to. Required for resolving the
        /// channel-rooted scope.json path on disk.
        #[arg(long)]
        channel: String,
        /// Artifact id or `artifact://...` URI carrying the clone-manifest
        /// or an explicit `mounts[]` payload.
        #[arg(long = "bootstrap-artifact")]
        bootstrap_artifact: String,
    },
}

#[derive(Subcommand, Debug)]
enum EventCmd {
    /// Read one event by id, with the same ACL as its containing scope.
    Get { event_id: String },
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
        /// Prompt prefix to place at the very beginning of the handoff
        /// receiver's final provider prompt for this event only.
        #[arg(long = "handoff-prefix")]
        handoff_prefix: Option<String>,
        /// Add one or more `attaches_artifact` relations targeting an artifact
        /// (`art_…` or `artifact://…`). May be repeated.
        #[arg(long = "artifact-link")]
        artifact_link: Vec<String>,
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
        #[arg(long = "parent-source-event")]
        parent_source_event: Option<String>,
        #[arg(long = "parent-task")]
        parent_task: Option<String>,
        #[arg(long = "practice-contract-epoch")]
        practice_contract_epoch: Option<String>,
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
        #[arg(long = "contract-json")]
        contract_json: Option<String>,
        #[arg(long = "contract-file")]
        contract_file: Option<PathBuf>,
        #[arg(long = "idempotency-key")]
        idempotency_key: Option<String>,
    },
    /// Attach, find, and list task identity references.
    Ref {
        #[command(subcommand)]
        sub: TaskRefCmd,
    },
    /// Attach, activate, and list typed task artifacts.
    Artifact {
        #[command(subcommand)]
        sub: TaskArtifactCmd,
    },
    /// Append and list typed task facts.
    Fact {
        #[command(subcommand)]
        sub: TaskFactCmd,
    },
    /// Put and read task projections.
    Projection {
        #[command(subcommand)]
        sub: TaskProjectionCmd,
    },
    /// Update a task assignment result.
    Assignment {
        #[command(subcommand)]
        sub: TaskAssignmentCmd,
    },
    /// List and ack durable task change deliveries.
    Change {
        #[command(subcommand)]
        sub: TaskChangeCmd,
    },
    /// Workspace/write-scope lease commands.
    Workspace {
        #[command(subcommand)]
        sub: TaskWorkspaceCmd,
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
        #[arg(long = "result-envelope-json")]
        result_envelope_json: Option<String>,
        #[arg(long = "result-artifact-id")]
        result_artifact_ids: Vec<String>,
        #[arg(long = "result-fact-id")]
        result_fact_ids: Vec<String>,
        #[arg(long = "evidence-ref")]
        evidence_refs: Vec<String>,
    },
    Context {
        assignment_id: String,
    },
    Preflight {
        assignment_id: String,
        #[arg(long = "target-key", default_value = "")]
        target_key: String,
        #[arg(long, default_value = "")]
        head: String,
        #[arg(long, default_value = "")]
        effect: String,
    },
}

#[derive(Subcommand, Debug)]
enum TaskRefCmd {
    Attach {
        task_id: String,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "")]
        subtype: String,
        #[arg(long)]
        value: String,
        #[arg(long, default_value = "")]
        normalized: String,
        #[arg(long, default_value = "inferred")]
        confidence: String,
        #[arg(long, default_value = "active")]
        status: String,
        #[arg(long = "source-event")]
        source_event: Option<String>,
        #[arg(long = "fields-json")]
        fields_json: Option<String>,
    },
    Find {
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "")]
        subtype: String,
        #[arg(long)]
        normalized: String,
        #[arg(long)]
        channel: Option<String>,
        #[arg(long)]
        confidence: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    List {
        task_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum TaskArtifactCmd {
    Attach {
        task_id: String,
        #[arg(long = "artifact-id")]
        artifact_id: String,
        #[arg(long, default_value = "")]
        schema: String,
        #[arg(long, default_value = "evidence")]
        role: String,
        #[arg(long, default_value = "active")]
        status: String,
        #[arg(long = "lineage-json")]
        lineage_json: Option<String>,
        #[arg(long = "binding-json")]
        binding_json: Option<String>,
    },
    Activate {
        link_id: String,
        #[arg(long = "supersede")]
        supersede_link_ids: Vec<String>,
    },
    List {
        task_id: String,
        #[arg(long)]
        status: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TaskFactCmd {
    Append {
        task_id: String,
        #[arg(long = "target-key", default_value = "")]
        target_key: String,
        #[arg(long)]
        kind: String,
        #[arg(long = "type", default_value = "user_defined")]
        fact_type: String,
        #[arg(long)]
        signature: Option<String>,
        #[arg(long, default_value = "active")]
        status: String,
        #[arg(long = "replaces")]
        replaces: Vec<String>,
        #[arg(long = "authority", default_value = "")]
        authority: String,
        #[arg(long = "authority-binding-json")]
        authority_binding_json: Option<String>,
        #[arg(long = "source-cursor")]
        source_cursor: Option<String>,
        #[arg(long = "source-snapshot-id")]
        source_snapshot_id: Option<String>,
        #[arg(long = "external-updated-at")]
        external_updated_at: Option<String>,
        #[arg(long = "observed-field")]
        observed_fields: Vec<String>,
        #[arg(long = "unobserved-field")]
        unobserved_fields: Vec<String>,
        #[arg(long = "unavailable-reason")]
        unavailable_reason: Option<String>,
        #[arg(long = "snapshot-completeness")]
        snapshot_completeness: Option<String>,
        #[arg(long = "producer-id")]
        producer_id: Option<String>,
        #[arg(long, default_value = "")]
        summary: String,
        #[arg(long = "raw-ref")]
        raw_refs: Vec<String>,
        #[arg(long = "artifact-id")]
        artifact_id: Option<String>,
        #[arg(long = "payload-schema", default_value = "")]
        payload_schema: String,
        #[arg(long = "subject-json")]
        subject_json: Option<String>,
        #[arg(long = "payload-json")]
        payload_json: Option<String>,
    },
    List {
        task_id: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long = "target-key")]
        target_key: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TaskProjectionCmd {
    Put {
        task_id: String,
        #[arg(long = "type", default_value = "summary")]
        projection_type: String,
        #[arg(long, default_value = "fresh")]
        health: String,
        #[arg(long = "producer-id")]
        producer_id: Option<String>,
        #[arg(long = "watermark-json")]
        watermark_json: Option<String>,
        #[arg(long = "payload-schema", default_value = "")]
        payload_schema: String,
        #[arg(long = "payload-json")]
        payload_json: Option<String>,
    },
    Get {
        task_id: String,
        #[arg(long = "type", default_value = "summary")]
        projection_type: String,
    },
    List {
        task_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum TaskChangeCmd {
    List {
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long = "include-handled")]
        include_handled: bool,
        #[arg(long = "after-cursor")]
        after_cursor: Option<u64>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Ack {
        change_id: String,
        #[arg(long)]
        disposition: String,
        #[arg(long = "result-ref")]
        result_ref_ids: Vec<String>,
        #[arg(long, default_value = "")]
        reason: String,
    },
}

#[derive(Subcommand, Debug)]
enum TaskWorkspaceCmd {
    Lease {
        #[command(subcommand)]
        sub: TaskWorkspaceLeaseCmd,
    },
}

#[derive(Subcommand, Debug)]
enum TaskWorkspaceLeaseCmd {
    Acquire {
        assignment_id: String,
        #[arg(long = "resource-key")]
        resource_key: String,
        #[arg(long, default_value = "write")]
        mode: String,
        #[arg(long = "ttl-seconds")]
        ttl_seconds: Option<i64>,
    },
    Release {
        lease_id: String,
    },
    List {
        #[arg(long = "resource-key")]
        resource_key: Option<String>,
        #[arg(long = "assignment-id")]
        assignment_id: Option<String>,
        #[arg(long = "active-only")]
        active_only: bool,
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
        /// Actor/service capability metadata as JSON.
        #[arg(long = "capabilities-json")]
        capabilities_json: Option<String>,
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
    /// Bump the reload-epoch marker for `actor_id` so a running
    /// `joi agent serve` host re-reads the AgentSpec + bundle and
    /// respawns the worker. See design §7.1.
    Reload { actor_id: String },
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
    if let Cmd::Memory { sub } = &args.cmd {
        return match sub {
            MemoryCmd::Query {
                actor,
                channel,
                text,
                limit,
                include_non_accepted,
                profile_dir,
            } => cmd::memory::query(
                actor.clone(),
                profile_dir.clone(),
                channel.clone(),
                text.clone(),
                *limit,
                *include_non_accepted,
                args.json,
            ),
            MemoryCmd::Append {
                actor,
                summary,
                source_channel,
                source_event,
                status,
                record_type,
                confidence,
                detail,
                tags,
                profile_dir,
            } => cmd::memory::append(
                actor.clone(),
                profile_dir.clone(),
                summary.clone(),
                source_channel.clone(),
                source_event.clone(),
                status.clone(),
                record_type.clone(),
                confidence.clone(),
                detail.clone(),
                tags.clone(),
                args.json,
            ),
            MemoryCmd::Get {
                actor,
                memory_id,
                profile_dir,
            } => cmd::memory::get(
                actor.clone(),
                profile_dir.clone(),
                memory_id.clone(),
                args.json,
            ),
            MemoryCmd::Update {
                actor,
                memory_id,
                status,
                reason,
                source_event,
                profile_dir,
            } => cmd::memory::update(
                actor.clone(),
                profile_dir.clone(),
                memory_id.clone(),
                status.clone(),
                reason.clone(),
                source_event.clone(),
                args.json,
            ),
        };
    }
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
    // local human actor. Keep it as a compatibility path for repo-bundled
    // AgentSpec actors while `joi daemon` hosts machine-configured agents.
    if let Cmd::Agent {
        sub: AgentCmd::Serve {
            specs,
            allow_actors,
        },
    } = args.cmd
    {
        return cmd::agent_serve::run(specs, cfg.server_url, allow_actors).await;
    }

    // `joi agent` local inspection commands should work without opening an
    // unused human connection.
    if let Cmd::Agent { sub } = args.cmd {
        match sub {
            AgentCmd::List => cmd::agent::list()?,
            AgentCmd::Reload { actor_id } => cmd::agent::reload(actor_id)?,
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

    // `service reload` is local-only — it bumps an on-disk marker that
    // the supervising `joi service serve` host polls. No server contact.
    if let Cmd::Service {
        sub: ServiceCmd::Reload { service_id },
    } = &args.cmd
    {
        return cmd::service::reload(service_id.clone());
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

    // `service start/stop/status` are local-only file-IO commands —
    // they read/write/list per-instance `request.json` files under the
    // host data root. The running `joi service serve` host is the
    // observer; these commands themselves never touch the server.
    if let Cmd::Service {
        sub:
            ServiceCmd::Start {
                spec_id,
                thread,
                channel,
                params,
                specs,
            },
    } = args.cmd
    {
        return cmd::service::start(spec_id, thread, channel, params, specs);
    }
    if let Cmd::Service {
        sub: ServiceCmd::Stop { spec_id, thread },
    } = &args.cmd
    {
        return cmd::service::stop(spec_id.clone(), thread.clone());
    }
    if let Cmd::Service {
        sub: ServiceCmd::Status { spec_id },
    } = &args.cmd
    {
        return cmd::service::status(spec_id.clone());
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
                resident_as,
                bootstrap_artifact,
            } => {
                cmd::thread::create(
                    client,
                    channel,
                    root_event,
                    title,
                    resident_as,
                    bootstrap_artifact,
                )
                .await?
            }
            ThreadCmd::List { channel, archived } => {
                cmd::thread::list(client, channel, archived).await?
            }
            ThreadCmd::Archive { thread_id } => {
                cmd::thread::archive(client, thread_id, true).await?
            }
            ThreadCmd::Unarchive { thread_id } => {
                cmd::thread::archive(client, thread_id, false).await?
            }
            ThreadCmd::ArchiveList { channel } => {
                cmd::thread::list(client, Some(channel), true).await?
            }
            ThreadCmd::Delete { thread_id } => cmd::thread::delete(client, thread_id).await?,
            ThreadCmd::Bootstrap {
                thread_id,
                channel,
                bootstrap_artifact,
            } => cmd::thread::bootstrap(client, channel, thread_id, bootstrap_artifact).await?,
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
            handoff_prefix,
            message,
        } => {
            cmd::handoff::run(
                client,
                cfg.actor_id,
                agent,
                r#in,
                channel,
                target,
                handoff_prefix,
                message,
            )
            .await?
        }
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
                parent_source_event,
                parent_task,
                practice_contract_epoch,
            } => {
                cmd::task::create(
                    client,
                    cfg.actor_id,
                    source_event,
                    title,
                    description,
                    owner,
                    status,
                    parent_source_event,
                    parent_task,
                    practice_contract_epoch,
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
                contract_json,
                contract_file,
                idempotency_key,
            } => {
                cmd::task::assign(
                    client,
                    cfg.actor_id,
                    task_id,
                    to,
                    assignment_type,
                    instruction,
                    contract_json,
                    contract_file,
                    idempotency_key,
                )
                .await?
            }
            TaskCmd::Ref { sub } => match sub {
                TaskRefCmd::Attach {
                    task_id,
                    kind,
                    subtype,
                    value,
                    normalized,
                    confidence,
                    status,
                    source_event,
                    fields_json,
                } => {
                    cmd::task::ref_attach(
                        client,
                        task_id,
                        kind,
                        subtype,
                        value,
                        normalized,
                        confidence,
                        status,
                        source_event,
                        fields_json,
                    )
                    .await?
                }
                TaskRefCmd::Find {
                    kind,
                    subtype,
                    normalized,
                    channel,
                    confidence,
                    status,
                } => {
                    cmd::task::ref_find(
                        client, kind, subtype, normalized, channel, confidence, status,
                    )
                    .await?
                }
                TaskRefCmd::List { task_id } => cmd::task::ref_list(client, task_id).await?,
            },
            TaskCmd::Artifact { sub } => match sub {
                TaskArtifactCmd::Attach {
                    task_id,
                    artifact_id,
                    schema,
                    role,
                    status,
                    lineage_json,
                    binding_json,
                } => {
                    cmd::task::artifact_attach(
                        client,
                        task_id,
                        artifact_id,
                        schema,
                        role,
                        status,
                        lineage_json,
                        binding_json,
                    )
                    .await?
                }
                TaskArtifactCmd::Activate {
                    link_id,
                    supersede_link_ids,
                } => cmd::task::artifact_activate(client, link_id, supersede_link_ids).await?,
                TaskArtifactCmd::List { task_id, status } => {
                    cmd::task::artifact_list(client, task_id, status).await?
                }
            },
            TaskCmd::Fact { sub } => match sub {
                TaskFactCmd::Append {
                    task_id,
                    target_key,
                    kind,
                    fact_type,
                    signature,
                    status,
                    replaces,
                    authority,
                    authority_binding_json,
                    source_cursor,
                    source_snapshot_id,
                    external_updated_at,
                    observed_fields,
                    unobserved_fields,
                    unavailable_reason,
                    snapshot_completeness,
                    producer_id,
                    summary,
                    raw_refs,
                    artifact_id,
                    payload_schema,
                    subject_json,
                    payload_json,
                } => {
                    cmd::task::fact_append(
                        client,
                        task_id,
                        target_key,
                        kind,
                        fact_type,
                        signature,
                        status,
                        replaces,
                        authority,
                        authority_binding_json,
                        source_cursor,
                        source_snapshot_id,
                        external_updated_at,
                        observed_fields,
                        unobserved_fields,
                        unavailable_reason,
                        snapshot_completeness,
                        producer_id,
                        summary,
                        raw_refs,
                        artifact_id,
                        payload_schema,
                        subject_json,
                        payload_json,
                    )
                    .await?
                }
                TaskFactCmd::List {
                    task_id,
                    kind,
                    status,
                    target_key,
                } => cmd::task::fact_list(client, task_id, kind, status, target_key).await?,
            },
            TaskCmd::Projection { sub } => match sub {
                TaskProjectionCmd::Put {
                    task_id,
                    projection_type,
                    health,
                    producer_id,
                    watermark_json,
                    payload_schema,
                    payload_json,
                } => {
                    cmd::task::projection_put(
                        client,
                        task_id,
                        projection_type,
                        health,
                        producer_id,
                        watermark_json,
                        payload_schema,
                        payload_json,
                    )
                    .await?
                }
                TaskProjectionCmd::Get {
                    task_id,
                    projection_type,
                } => cmd::task::projection_get(client, task_id, projection_type).await?,
                TaskProjectionCmd::List { task_id } => {
                    cmd::task::projection_list(client, task_id).await?
                }
            },
            TaskCmd::Assignment { sub } => match sub {
                TaskAssignmentCmd::Update {
                    assignment_id,
                    status,
                    result_event,
                    result,
                    result_envelope_json,
                    result_artifact_ids,
                    result_fact_ids,
                    evidence_refs,
                } => {
                    cmd::task::assignment_update(
                        client,
                        assignment_id,
                        status,
                        result_event,
                        result,
                        result_envelope_json,
                        result_artifact_ids,
                        result_fact_ids,
                        evidence_refs,
                    )
                    .await?
                }
                TaskAssignmentCmd::Context { assignment_id } => {
                    cmd::task::assignment_context(client, assignment_id).await?
                }
                TaskAssignmentCmd::Preflight {
                    assignment_id,
                    target_key,
                    head,
                    effect,
                } => {
                    cmd::task::assignment_preflight(client, assignment_id, target_key, head, effect)
                        .await?
                }
            },
            TaskCmd::Change { sub } => match sub {
                TaskChangeCmd::List {
                    task_id,
                    include_handled,
                    after_cursor,
                    limit,
                } => {
                    cmd::task::change_list(client, task_id, include_handled, after_cursor, limit)
                        .await?
                }
                TaskChangeCmd::Ack {
                    change_id,
                    disposition,
                    result_ref_ids,
                    reason,
                } => {
                    cmd::task::change_ack(client, change_id, disposition, result_ref_ids, reason)
                        .await?
                }
            },
            TaskCmd::Workspace { sub } => match sub {
                TaskWorkspaceCmd::Lease { sub } => match sub {
                    TaskWorkspaceLeaseCmd::Acquire {
                        assignment_id,
                        resource_key,
                        mode,
                        ttl_seconds,
                    } => {
                        cmd::task::lease_acquire(
                            client,
                            assignment_id,
                            resource_key,
                            mode,
                            ttl_seconds,
                        )
                        .await?
                    }
                    TaskWorkspaceLeaseCmd::Release { lease_id } => {
                        cmd::task::lease_release(client, lease_id).await?
                    }
                    TaskWorkspaceLeaseCmd::List {
                        resource_key,
                        assignment_id,
                        active_only,
                    } => {
                        cmd::task::lease_list(client, resource_key, assignment_id, active_only)
                            .await?
                    }
                },
            },
        },
        Cmd::Spec { sub } => match sub {
            SpecCmd::Apply {
                action_event_id,
                dry_run,
            } => {
                cmd::spec_apply::run(client, cfg.actor_id.clone(), action_event_id, dry_run).await?
            }
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
        Cmd::Memory { .. } => unreachable!("handled before client setup"),
        Cmd::Event { sub } => match sub {
            EventCmd::Get { event_id } => cmd::event::get(client, event_id).await?,
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
                handoff_prefix,
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
                        handoff_prefix,
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
                capabilities_json,
            } => cmd::actor::upsert(client, actor_id, kind, display, capabilities_json).await?,
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
        Cmd::Workspace { .. } => unreachable!("handled before client setup"),
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
