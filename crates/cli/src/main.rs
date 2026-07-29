use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use loom_cli::client::Client;
use loom_cli::render::OutputMode;
use loom_cli::{cmd, config, daemon_ipc, render};
use proto::methods::AgentSpec;
use proto::types::ActorKind;

/// Resolve the instruction payload for `set-instruction` CLI commands.
/// Exactly one of `file` or `text` must be provided.
fn read_instruction_payload(file: Option<String>, text: Option<String>) -> Result<String> {
    match (file, text) {
        (Some(path), None) => {
            let body = std::fs::read_to_string(&path)
                .with_context(|| format!("read instruction file {path}"))?;
            Ok(body)
        }
        (None, Some(value)) => Ok(value),
        (Some(_), Some(_)) => Err(anyhow!(
            "pass either --file or --text to set-instruction, not both"
        )),
        (None, None) => Err(anyhow!("set-instruction requires either --file or --text")),
    }
}

#[derive(Parser, Debug)]
#[command(name = "loom", about = "Loom multi-actor collaboration CLI")]
struct Args {
    /// Override the configured server URL (defaults to ws://127.0.0.1:7878/rpc).
    #[arg(long, global = true, env = "LOOM_SERVER")]
    server: Option<String>,
    /// Override the configured local actor id.
    #[arg(long = "as", global = true, env = "LOOM_ACTOR")]
    actor: Option<String>,
    /// Override the configured local display name.
    #[arg(long = "display", global = true, env = "LOOM_DISPLAY")]
    display: Option<String>,
    /// Bind the CLI connection with an explicit actor kind.
    #[arg(
        long = "actor-kind",
        global = true,
        env = "LOOM_ACTOR_KIND",
        value_enum
    )]
    actor_kind: Option<ConnectionActorKind>,
    /// Bind as an observer without claiming the actor inbox.
    #[arg(long, global = true, env = "LOOM_OBSERVER")]
    observer: bool,
    /// Emit machine-readable JSON instead of human-friendly text.
    #[arg(long, global = true, env = "LOOM_JSON")]
    json: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ConnectionActorKind {
    Human,
    Agent,
    Service,
}

impl ConnectionActorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
            Self::Service => "service",
        }
    }
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
    /// Message commands using the canonical #channel/#channel:root-message/dm:@actor grammar.
    Message {
        #[command(subcommand)]
        sub: MessageCmd,
    },
    /// List and acknowledge this actor's deterministic attention inbox.
    Inbox {
        #[command(subcommand)]
        sub: InboxCmd,
    },
    /// Respond to an action.request message.
    Action {
        #[command(subcommand)]
        sub: ActionCmd,
    },
    /// Create, claim, update, and delegate message-anchored tasks.
    Task {
        #[command(subcommand)]
        sub: TaskCmd,
    },
    /// Record an agent execution run.
    Run {
        #[command(subcommand)]
        sub: RunCmd,
    },
    /// Coordinate explicit multi-actor work with baton/revision checks.
    Coordination {
        #[command(subcommand)]
        sub: CoordinationCmd,
    },
    /// Publish and activate immutable agent config versions.
    AgentConfig {
        #[command(subcommand)]
        sub: AgentConfigCmd,
    },
    /// Ask the triggering human to choose or provide input, then return the answer to this process.
    AskUserQuestion {
        /// Max seconds to wait for action.response.
        #[arg(long = "timeout-seconds")]
        timeout_seconds: Option<u64>,
        /// Scope id. Defaults to LOOM_SCOPE_ID inside daemon-managed agent turns.
        #[arg(long)]
        r#in: Option<String>,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
        /// Actor id that should answer. Defaults to LOOM_TRIGGER_ACTOR inside daemon turns.
        #[arg(long)]
        to: Option<String>,
        /// Run id to associate with the action.request. Defaults to LOOM_RUN_ID.
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
        /// Scope id. Defaults to LOOM_SCOPE_ID inside daemon-managed agent turns.
        #[arg(long)]
        r#in: Option<String>,
        /// Treat --in as a channel id instead of a thread id.
        #[arg(long)]
        channel: bool,
        /// Actor id that should approve. Defaults to LOOM_TRIGGER_ACTOR inside daemon turns.
        #[arg(long)]
        to: Option<String>,
        /// Run id to associate with the action.request. Defaults to LOOM_RUN_ID.
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
    /// Inspect daemon-configured agents. Runtime hosting is done by `loom-daemon`.
    Agent {
        #[command(subcommand)]
        sub: AgentCmd,
    },
    /// Manage provider manifests used to launch agent CLIs.
    Provider {
        #[command(subcommand)]
        sub: ProviderCmd,
    },
    /// Read or update the official Loom operating guide.
    Guide {
        #[command(subcommand)]
        sub: GuideCmd,
    },
    /// Materialize embedded official Loom skills for external runtimes.
    Skill {
        #[command(subcommand)]
        sub: SkillCmd,
    },
    /// Manage daemon machines through server-routed machine commands.
    Machine {
        #[command(subcommand)]
        sub: MachineCmd,
    },
    /// Inspect actors known to the server.
    Actor {
        #[command(subcommand)]
        sub: ActorCmd,
    },
    /// Manage channel-scoped actor groups used by @group mentions.
    Group {
        #[command(subcommand)]
        sub: GroupCmd,
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
    /// Run loom as a stdio MCP server. Typically not invoked by humans —
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
}

#[derive(Subcommand, Debug)]
enum SpecCmd {
    /// Apply the lesson-plan attached to the action.request that was
    /// accepted by the given `action.response` message. See
    /// `crates/cli/src/cmd/spec_apply.rs` for the frontmatter contract.
    Apply {
        /// Message id of the `action.response` (responseKind = accepted) that
        /// approved the lesson-plan.
        #[arg(long = "action")]
        action_message_id: String,
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
    /// Actor id (defaults to LOOM_ACTOR / current actor). Use --actor to
    /// explicitly target a per-actor workspace.
    #[arg(long, env = "LOOM_ACTOR")]
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
        /// to `~/.config/loom/services/` (or `$LOOM_SERVICE_SPECS`).
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
    /// "loom service am-handler --service-id <id>"` once per DingTalk
    /// message. Replaces the removed Python bridge.
    AmHandler {
        /// ServiceSpec id under --specs (defaults to ~/.config/loom/services/).
        #[arg(long = "service-id")]
        service_id: String,
        /// Override the specs directory.
        #[arg(long)]
        specs: Option<PathBuf>,
        /// Internal: spawned by ourselves in async_send mode. Carries
        /// the JSON payload `{sourcePayload, triggerId, scopeKind, scopeId}`.
        #[arg(long = "async-reply", hide = true)]
        async_reply: Option<String>,
    },
    /// Bump the reload-epoch marker for `service_id` so a running
    /// `loom service serve` host re-reads the ServiceSpec and respawns
    /// the supervised plugin instance(s). See design §7.1.
    Reload { service_id: String },
    /// Inspect ServiceSpec JSON files on disk (no server contact).
    Spec {
        #[command(subcommand)]
        sub: ServiceSpecCmd,
    },
    /// Start a `lifecycle = thread_bound` instance by writing a
    /// per-instance `request.json`. A running `loom service serve`
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
        /// validation). Defaults to `~/.config/loom/services/`.
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
        /// supplied via the env (`LOOM_ACTOR`) but explicit takes precedence.
        #[arg(long = "actor-id", env = "LOOM_ACTOR")]
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
    /// back to the running loom-server over WebSocket and proxies each tool
    /// call to one `message.send`.
    Announcement {
        #[arg(long = "actor-id", env = "LOOM_ACTOR")]
        actor_id: String,
        /// loom-server WebSocket URL. Falls back to `LOOM_SERVER`; the
        /// runtime hands this over explicitly when spawning the MCP child.
        #[arg(long = "server", env = "LOOM_SERVER")]
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
        #[arg(long = "source-message")]
        source_message: Option<String>,
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
        #[arg(long = "source-message")]
        source_message: Option<String>,
        #[arg(long = "profile-dir")]
        profile_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum ChannelCmd {
    /// Create a new channel. Channels created via this CLI are private by
    /// default — the caller is the sole initial member; invite others
    /// with `loom channel invite`.
    Create {
        #[arg(long)]
        title: String,
        /// Create a public channel visible to every actor.
        #[arg(long)]
        public: bool,
    },
    /// List channels visible to this caller (public channels + private
    /// channels the caller is a member of).
    List,
    /// Lookup visible channels by exact title.
    Lookup {
        #[arg(long)]
        title: String,
    },
    /// Update channel metadata or visibility.
    Update {
        channel_id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        public: bool,
    },
    /// Delete a channel and its child threads.
    Delete {
        channel_id: String,
        /// Kept for compatibility; channel delete cascades by default.
        #[arg(long, hide = true)]
        cascade: bool,
    },
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
    /// List configured workspace overrides for a channel.
    MemberConfigList { channel_id: String },
    /// Show one member's workspace override.
    MemberConfigGet {
        channel_id: String,
        actor_id: String,
    },
    /// Set one member's workspace override and/or external mention ids.
    MemberConfigSet {
        channel_id: String,
        actor_id: String,
        #[arg(long = "workspace-dir")]
        workspace_dir: Option<String>,
        #[arg(long = "mention-id")]
        mention_ids: Vec<String>,
    },
    /// Resolve external mention ids against members of one channel.
    MemberResolve {
        channel_id: String,
        #[arg(long = "mention-id", required = true)]
        mention_ids: Vec<String>,
    },
    /// Clear one member's workspace override.
    MemberConfigClear {
        channel_id: String,
        actor_id: String,
    },
    /// Set the channel-level instructions projected into every member
    /// agent's AGENTS.md. Use `--file` to load from a path or `--text`
    /// for an inline value. Instructions are free-text with no size
    /// limit; common uses include shared context, conventions, or
    /// references to external shared layers (e.g. Obsidian vault paths).
    SetInstruction {
        channel_id: String,
        #[arg(long, conflicts_with = "text")]
        file: Option<String>,
        #[arg(long, conflicts_with = "file")]
        text: Option<String>,
    },
    /// Print the channel-level instructions (or "(none)").
    GetInstruction { channel_id: String },
    /// Clear the channel-level instructions.
    ClearInstruction { channel_id: String },
    /// Manage channel-level skills mounted into every member agent's workspace.
    #[command(subcommand)]
    Skill(ChannelSkillCmd),
}

#[derive(Subcommand, Debug)]
enum ChannelSkillCmd {
    /// Add a skill to the channel registry. The skill source is a
    /// directory path. Use `--id` to override the derived id.
    Add {
        channel_id: String,
        source: String,
        #[arg(long)]
        id: Option<String>,
    },
    /// Remove a skill from the channel registry.
    Remove {
        channel_id: String,
        skill_id: String,
    },
    /// List skills registered for the channel.
    List { channel_id: String },
}

#[derive(Subcommand, Debug)]
enum ThreadCmd {
    Create {
        #[arg(long)]
        channel: String,
        /// Channel-scope message that anchors the thread.
        #[arg(long = "root-message")]
        root_message: String,
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
    /// Follow a thread so replies enter this actor's inbox.
    Follow {
        thread_id: String,
        #[arg(long)]
        muted: bool,
    },
    /// Stop following a thread.
    Unfollow { thread_id: String },
    /// Set the thread-level instructions appended to AGENTS.md in this
    /// thread's scope. Use `--file` to load from a path or `--text` for
    /// an inline value. Instructions are free-text with no size limit;
    /// thread instructions override channel instructions for threads
    /// that set them.
    SetInstruction {
        thread_id: String,
        #[arg(long, conflicts_with = "text")]
        file: Option<String>,
        #[arg(long, conflicts_with = "file")]
        text: Option<String>,
    },
    /// Print the thread-level instructions (or "(none)").
    GetInstruction { thread_id: String },
    /// Clear the thread-level instructions.
    ClearInstruction { thread_id: String },
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
    /// Manage thread-level skills mounted into agent workspaces in this
    /// thread's scope. Thread skills override channel skills for the same id.
    #[command(subcommand)]
    Skill(ThreadSkillCmd),
}

#[derive(Subcommand, Debug)]
enum ThreadSkillCmd {
    /// Add a skill to the thread registry.
    Add {
        thread_id: String,
        source: String,
        #[arg(long)]
        id: Option<String>,
    },
    /// Remove a skill from the thread registry.
    Remove { thread_id: String, skill_id: String },
    /// List skills registered for the thread.
    List { thread_id: String },
}

#[derive(Subcommand, Debug)]
enum TaskCmd {
    /// Create a task anchored to a top-level channel message.
    Create {
        #[arg(long = "source-message")]
        source_message: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long = "parent-source-message")]
        parent_source_message: Option<String>,
        #[arg(long = "parent-task")]
        parent_task: Option<String>,
        #[arg(long = "practice-contract-epoch")]
        practice_contract_epoch: Option<String>,
    },
    /// List visible tasks.
    List {
        #[arg(long)]
        channel: Option<String>,
        #[arg(long = "source-message")]
        source_message: Option<String>,
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
    /// Claim a task for this actor or an explicit actor. Pass either a task id
    /// or --source-message to atomically create/claim the message-anchored task.
    Claim {
        task_id: Option<String>,
        #[arg(long = "source-message")]
        source_message: Option<String>,
        #[arg(long)]
        actor: Option<String>,
    },
    /// Mark a task done.
    Complete {
        task_id: String,
        #[arg(long)]
        result: Option<String>,
        #[arg(long = "artifact-id")]
        artifact_ids: Vec<String>,
    },
    /// Reopen a task, optionally assigning a new owner.
    Reopen {
        task_id: String,
        #[arg(long)]
        owner: Option<String>,
    },
    /// Cancel a task.
    Cancel {
        task_id: String,
        #[arg(long)]
        result: Option<String>,
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
        #[arg(long = "result-message")]
        result_message: Option<String>,
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
        #[arg(long = "source-message")]
        source_message: Option<String>,
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
    /// Send a message to #channel, #channel:root-message, or dm:@actor.
    Send {
        /// Destination target, for example #chan_123, #chan_123:msg_456, or dm:@actor_id.
        #[arg(long)]
        target: Option<String>,
        /// Destination thread id. Resolves to #channel:root-message before sending.
        #[arg(long, conflicts_with = "target")]
        thread: Option<String>,
        /// Direct-message recipient. Equivalent to --target dm:@<actor>.
        #[arg(long, conflicts_with_all = ["target", "thread"])]
        to: Option<String>,
        /// Same-scope private recipient. The message remains in the current channel/thread scope.
        #[arg(long = "private-to")]
        private_to: Vec<String>,
        #[arg(long)]
        text: Option<String>,
        /// Allow literal backslash-n sequences in --text from an agent run.
        #[arg(long = "allow-escaped-newlines")]
        allow_escaped_newlines: bool,
        /// Allow a deliberate message after this run was marked no-reply or
        /// completed its assignment handoff.
        #[arg(long = "allow-after-no-reply")]
        allow_after_no_reply: bool,
        /// Message intent: chat, ask, request_action, assign_task, status_update, review, notify.
        #[arg(long)]
        intent: Option<String>,
        /// Delivery policy: notify_only, wake_agent, route_by_intent, silent.
        #[arg(long = "delivery-policy")]
        delivery_policy: Option<String>,
        /// Only send if this is still the latest message in the target scope.
        #[arg(long = "if-latest")]
        if_latest: Option<String>,
        /// Deduplicate retries by caller and resolved channel/thread scope.
        #[arg(long = "idempotency-key")]
        idempotency_key: Option<String>,
        #[arg(long = "attachment-id")]
        attachment_ids: Vec<String>,
    },
    /// Ask one or more actors/groups to act and wake agent recipients.
    Ask {
        /// Recipients to ask, for example @actor_id, @all, @agents, @humans, or group:<id>.
        #[arg(required = true)]
        recipients: Vec<String>,
        /// Destination target, for example #chan_123 or #chan_123:msg_456.
        #[arg(long)]
        target: Option<String>,
        /// Destination thread id. Resolves to #channel:root-message before asking.
        #[arg(long, conflicts_with = "target")]
        thread: Option<String>,
        #[arg(long)]
        text: Option<String>,
        /// Allow literal backslash-n sequences in --text from an agent run.
        #[arg(long = "allow-escaped-newlines")]
        allow_escaped_newlines: bool,
        /// Allow a deliberate message after this run was marked no-reply or
        /// completed its assignment handoff.
        #[arg(long = "allow-after-no-reply")]
        allow_after_no_reply: bool,
        /// Only send if this is still the latest message in the target scope.
        #[arg(long = "if-latest")]
        if_latest: Option<String>,
        /// Deduplicate retries by caller and resolved channel/thread scope.
        #[arg(long = "idempotency-key")]
        idempotency_key: Option<String>,
        #[arg(long = "attachment-id")]
        attachment_ids: Vec<String>,
    },
    /// Read messages from #channel, #channel:root-message, or dm:@actor.
    Read {
        #[arg(long)]
        target: Option<String>,
        /// Thread id to read. Resolves to #channel:root-message.
        #[arg(long, conflicts_with = "target")]
        thread: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        before: Option<String>,
        /// Include same-scope private messages visible to this actor.
        #[arg(long = "include-private")]
        include_private: bool,
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
    /// Toggle this actor's emoji reaction on a message.
    React { message_id: String, emoji: String },
}

#[derive(Subcommand, Debug)]
enum InboxCmd {
    /// List this actor's pending directed deliveries and ack them unless --no-ack is set.
    List {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Delivery state to list: pending, delivered, failed, or all.
        #[arg(long, default_value = "pending")]
        state: String,
        #[arg(long = "no-ack")]
        no_ack: bool,
    },
}

#[derive(Subcommand, Debug)]
enum RunCmd {
    /// Open a run for the current actor in a channel or thread scope.
    Open {
        #[arg(long)]
        target: String,
        #[arg(long = "delivery-id")]
        delivery_id: Option<String>,
        #[arg(long = "start-reason")]
        start_reason: Option<String>,
        #[arg(long = "agent-config-version-id")]
        agent_config_version_id: String,
    },
    /// Append private execution progress to a run.
    Append {
        run_id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long = "frame-kind", default_value = "log")]
        frame_kind: String,
        #[arg(long = "payload-json")]
        payload_json: Option<String>,
    },
    /// Mark the current run as intentionally producing no visible reply.
    Ignore {
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Close a run with a terminal status.
    Close {
        run_id: String,
        #[arg(long, default_value = "completed")]
        status: String,
    },
}

#[derive(Subcommand, Debug)]
enum CoordinationCmd {
    /// Propose a coordination session.
    Propose {
        #[arg(long)]
        target: String,
        #[arg(long, default_value = "sequential")]
        mode: String,
        #[arg(long = "decision-rule", default_value = "owner_decides")]
        decision_rule: String,
        #[arg(long = "participant")]
        participants: Vec<String>,
        #[arg(long = "plan-json")]
        plan_json: Option<String>,
    },
    /// Commit a proposed session and deliver the first baton/slots.
    Commit { session_id: String },
    /// Ack or reject a proposed session.
    Respond {
        session_id: String,
        #[arg(long, conflicts_with = "reject")]
        accept: bool,
        #[arg(long, conflicts_with = "accept")]
        reject: bool,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Submit a coordination step.
    Step {
        session_id: String,
        #[arg(long = "base-revision")]
        base_revision: u64,
        #[arg(long = "step-type", default_value = "work")]
        step_type: String,
        #[arg(long = "output-json")]
        output_json: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    /// Skip the current baton.
    Skip {
        session_id: String,
        #[arg(long = "base-revision")]
        base_revision: u64,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reassign one participant/baton holder.
    Reassign {
        session_id: String,
        #[arg(long = "from")]
        from_actor_id: String,
        #[arg(long = "to")]
        to_actor_id: String,
        #[arg(long = "base-revision")]
        base_revision: u64,
    },
}

#[derive(Subcommand, Debug)]
enum AgentConfigCmd {
    /// Publish an immutable config version for an actor.
    Publish {
        actor_id: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        adapter: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long = "tools-json")]
        tools_json: Option<String>,
    },
    /// Activate a config version for an actor.
    Activate {
        actor_id: String,
        version_id: String,
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
enum GroupCmd {
    /// Create a channel-scoped mention group.
    Create {
        #[arg(long)]
        channel: String,
        name: String,
        #[arg(long)]
        display: Option<String>,
        #[arg(long = "member")]
        members: Vec<String>,
        #[arg(long = "wake-agents")]
        wake_agents: bool,
    },
    /// List groups visible to this actor.
    List {
        #[arg(long)]
        channel: Option<String>,
    },
    /// Add one actor to a group.
    AddMember { group_id: String, actor_id: String },
    /// Remove one actor from a group.
    RemoveMember { group_id: String, actor_id: String },
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
        message_id: String,
        #[arg(long, default_value = "allow")]
        option: String,
    },
    Decline {
        message_id: String,
        #[arg(long, default_value = "deny")]
        option: String,
    },
}

#[derive(Subcommand, Debug)]
enum AgentCmd {
    /// List agents configured for daemon-managed machines.
    List,
    /// Bump the reload-epoch marker for `actor_id` so a running
    /// `loom agent serve` host re-reads the AgentSpec + bundle and
    /// respawns the worker. See design §7.1.
    Reload { actor_id: String },
    /// Run as the v1 external agent client: load every AgentSpec under
    /// --specs (defaults to ~/.config/loom/agents) and supervise each agent
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

#[derive(Subcommand, Debug)]
enum ProviderCmd {
    /// Print a provider manifest example. Built-ins show the official runtime argv and default agent-scoped add-dir.
    Example {
        /// Print the built-in Claude Code provider manifest.
        #[arg(long)]
        claude: bool,
        /// Print the built-in Qoder CLI provider manifest.
        #[arg(long)]
        qoder: bool,
        /// Print the built-in GitHub Copilot CLI provider manifest.
        #[arg(long)]
        copilot: bool,
        /// Print the built-in Codex CLI provider manifest.
        #[arg(long)]
        codex: bool,
        /// Print the built-in OpenCode provider manifest.
        #[arg(long)]
        opencode: bool,
        /// Print the built-in Kimi Code CLI provider manifest.
        #[arg(long)]
        kimi: bool,
        /// Print the built-in ZCode provider manifest.
        #[arg(long)]
        zcode: bool,
    },
    /// Validate a provider manifest JSON file.
    Validate { path: PathBuf },
    /// Add a provider manifest into the current LOOM_CONFIG_DIR.
    Add {
        path: PathBuf,
        /// Overwrite an existing local provider manifest with the same id.
        #[arg(long)]
        replace: bool,
    },
    /// List built-in and locally installed providers.
    List,
    /// Show one resolved provider manifest.
    Show { provider_id: String },
    /// Remove a locally installed provider manifest.
    Remove { provider_id: String },
    /// Validate and check local command detection for one provider.
    Doctor { provider_id: String },
}

#[derive(Subcommand, Debug)]
enum GuideCmd {
    /// List available guide topics.
    List,
    /// Show one guide topic.
    Show { topic: String },
    /// Search guide topics.
    Search { query: String },
    /// Refresh the local guide cache from the official repository.
    Update,
}

#[derive(Subcommand, Debug)]
enum SkillCmd {
    /// Write the embedded official Loom skill into a dedicated directory.
    Materialize {
        /// Embedded skill id. Currently only `loom` is available.
        #[arg(value_name = "SKILL_ID", value_parser = ["loom"])]
        id: String,
        /// Dedicated skill directory to reconcile. Its contents are managed by Loom.
        #[arg(long, value_name = "SKILL_DIR")]
        output: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum MachineCmd {
    /// List daemon machines visible from the current server.
    List,
    /// Manage agents on a daemon-owned machine.
    Agent {
        #[command(subcommand)]
        sub: MachineAgentCmd,
    },
}

#[derive(Subcommand, Debug)]
enum MachineAgentCmd {
    /// Create an AgentSpec on the target daemon via machine/command.
    Create {
        #[arg(long)]
        machine: String,
        #[arg(long)]
        provider: String,
        #[arg(long = "actor-id")]
        actor_id: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long)]
        instructions: Option<String>,
        #[arg(long = "instructions-file")]
        instructions_file: Option<PathBuf>,
        /// Local source root whose AGENTS.md/CLAUDE.md and skill directories are daemon-managed.
        #[arg(long = "source-root")]
        source_root: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long = "reasoning-effort")]
        reasoning_effort: Option<String>,
        /// Coalesce compatible pending wake messages into one provider turn.
        #[arg(long = "wake-coalesce")]
        wake_coalesce: Option<bool>,
        /// Provider-facing Loom runtime awareness: native or hidden.
        #[arg(long = "runtime-awareness")]
        runtime_awareness: Option<String>,
        #[arg(long = "no-autostart")]
        no_autostart: bool,
    },
    /// Update an AgentSpec on the target daemon via machine/command.
    Update {
        #[arg(long)]
        machine: String,
        #[arg(long = "actor-id")]
        actor_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        instructions: Option<String>,
        #[arg(long = "instructions-file")]
        instructions_file: Option<PathBuf>,
        /// Local source root whose AGENTS.md/CLAUDE.md and skill directories are daemon-managed.
        #[arg(long = "source-root")]
        source_root: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long = "reasoning-effort")]
        reasoning_effort: Option<String>,
        /// Coalesce compatible pending wake messages into one provider turn.
        #[arg(long = "wake-coalesce")]
        wake_coalesce: Option<bool>,
        /// Provider-facing Loom runtime awareness: native or hidden.
        #[arg(long = "runtime-awareness")]
        runtime_awareness: Option<String>,
    },
    /// Remove an AgentSpec from the target daemon via machine/command.
    Remove {
        #[arg(long)]
        machine: String,
        #[arg(long = "actor-id")]
        actor_id: String,
    },
    /// View, add, or remove actor-local skill directories on the target daemon.
    Skill {
        #[command(subcommand)]
        sub: MachineAgentSkillCmd,
    },
}

#[derive(Subcommand, Debug)]
enum MachineAgentSkillCmd {
    /// List custom skill directories configured on an agent.
    List {
        #[arg(long)]
        machine: String,
        #[arg(long = "actor-id")]
        actor_id: String,
    },
    /// Add a skill directory. The target daemon validates that it contains SKILL.md.
    Add {
        #[arg(long)]
        machine: String,
        #[arg(long = "actor-id")]
        actor_id: String,
        /// Directory on the target daemon host that contains SKILL.md.
        source: PathBuf,
    },
    /// Remove a custom skill from this agent. The source directory is not deleted.
    Remove {
        #[arg(long)]
        machine: String,
        #[arg(long = "actor-id")]
        actor_id: String,
        /// Skill directory name to remove.
        skill: String,
    },
}

fn main() -> Result<()> {
    std::thread::Builder::new()
        .name("loom-main".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow::anyhow!("loom main thread panicked"))?
}

async fn async_main() -> Result<()> {
    init_tracing();
    // On Windows: if the binary was double-clicked (no arguments), show a
    // friendly help dialog instead of flashing a terminal and disappearing.
    if loom_platform::console::show_double_click_help() {
        return Ok(());
    }
    let args = Args::parse();
    render::set_output_mode(if args.json {
        OutputMode::Json
    } else {
        OutputMode::Pretty
    });
    if let Cmd::Skill { sub } = &args.cmd {
        return match sub {
            SkillCmd::Materialize { id, output } => cmd::skill::materialize(id, output),
        };
    }
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
                source_message,
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
                source_message.clone(),
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
                source_message,
                profile_dir,
            } => cmd::memory::update(
                actor.clone(),
                profile_dir.clone(),
                memory_id.clone(),
                status.clone(),
                reason.clone(),
                source_message.clone(),
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
    // AgentSpec actors while `loom-daemon` hosts machine-configured agents.
    if let Cmd::Agent {
        sub: AgentCmd::Serve {
            specs,
            allow_actors,
        },
    } = args.cmd
    {
        return cmd::agent_serve::run(specs, cfg.server_url, allow_actors).await;
    }

    // `loom agent` local inspection commands should work without opening an
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

    if let Cmd::Provider { sub } = args.cmd {
        match sub {
            ProviderCmd::Example {
                claude,
                qoder,
                copilot,
                codex,
                opencode,
                kimi,
                zcode,
            } => cmd::provider::example(cmd::provider::ExampleSelection {
                claude,
                qoder,
                copilot,
                codex,
                opencode,
                kimi,
                zcode,
            })?,
            ProviderCmd::Validate { path } => cmd::provider::validate(path)?,
            ProviderCmd::Add { path, replace } => cmd::provider::add(path, replace)?,
            ProviderCmd::List => cmd::provider::list()?,
            ProviderCmd::Show { provider_id } => cmd::provider::show(provider_id)?,
            ProviderCmd::Remove { provider_id } => cmd::provider::remove(provider_id)?,
            ProviderCmd::Doctor { provider_id } => cmd::provider::doctor(provider_id)?,
        }
        return Ok(());
    }

    if let Cmd::Guide { sub } = &args.cmd {
        match sub {
            GuideCmd::List => cmd::guide::list()?,
            GuideCmd::Show { topic } => cmd::guide::show(topic)?,
            GuideCmd::Search { query } => cmd::guide::search(query)?,
            GuideCmd::Update => cmd::guide::update()?,
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

    // `service validate` is offline — no server contact needed.
    if let Cmd::Service {
        sub: ServiceCmd::Validate { path },
    } = &args.cmd
    {
        return cmd::service::validate(path.clone());
    }

    // `service reload` is local-only — it bumps an on-disk marker that
    // the supervising `loom service serve` host polls. No server contact.
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
    // so they work even when no loom-server is running.
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
    // host data root. The running `loom service serve` host is the
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

    // `mcp memory` never talks to the loom server — it's spawned by the ACP
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
    let inferred_actor_kind = local_agent_spec_actor_kind(&cfg.actor_id)?;
    let connection_actor_kind = args.actor_kind.or(inferred_actor_kind);
    let observer = args.observer || (args.actor_kind.is_none() && inferred_actor_kind.is_some());
    let _ = match (connection_actor_kind, observer) {
        (Some(kind), true) => {
            client
                .open_observer_connection_as(&cfg.actor_id, kind.as_str(), Some(&cfg.display_name))
                .await?
        }
        (Some(kind), false) => {
            client
                .open_connection_as(&cfg.actor_id, kind.as_str(), Some(&cfg.display_name))
                .await?
        }
        (None, true) => {
            client
                .open_observer_connection_as(&cfg.actor_id, "human", Some(&cfg.display_name))
                .await?
        }
        (None, false) => {
            client
                .open_connection(&cfg.actor_id, Some(&cfg.display_name))
                .await?
        }
    };

    match args.cmd {
        Cmd::Who => unreachable!(),
        Cmd::Channel { sub } => match sub {
            ChannelCmd::Create { title, public } => {
                cmd::channel::create(client, cfg.actor_id.clone(), title, public).await?
            }
            ChannelCmd::List => cmd::channel::list(client).await?,
            ChannelCmd::Lookup { title } => cmd::channel::lookup(client, title).await?,
            ChannelCmd::Update {
                channel_id,
                title,
                public,
            } => cmd::channel::update(client, channel_id, title, public).await?,
            ChannelCmd::Delete {
                channel_id,
                cascade: _,
            } => cmd::channel::delete(client, channel_id).await?,
            ChannelCmd::Invite {
                channel_id,
                actor_id,
            } => cmd::channel::invite(client, channel_id, actor_id).await?,
            ChannelCmd::Revoke {
                channel_id,
                actor_id,
            } => cmd::channel::revoke(client, channel_id, actor_id).await?,
            ChannelCmd::Members { channel_id } => cmd::channel::members(client, channel_id).await?,
            ChannelCmd::MemberConfigList { channel_id } => {
                cmd::channel::member_config_list(client, channel_id).await?
            }
            ChannelCmd::MemberConfigGet {
                channel_id,
                actor_id,
            } => cmd::channel::member_config_get(client, channel_id, actor_id).await?,
            ChannelCmd::MemberConfigSet {
                channel_id,
                actor_id,
                workspace_dir,
                mention_ids,
            } => {
                cmd::channel::member_config_set(
                    client,
                    channel_id,
                    actor_id,
                    workspace_dir,
                    mention_ids,
                )
                .await?
            }
            ChannelCmd::MemberResolve {
                channel_id,
                mention_ids,
            } => cmd::channel::member_resolve(client, channel_id, mention_ids).await?,
            ChannelCmd::MemberConfigClear {
                channel_id,
                actor_id,
            } => cmd::channel::member_config_clear(client, channel_id, actor_id).await?,
            ChannelCmd::SetInstruction {
                channel_id,
                file,
                text,
            } => {
                let instructions = read_instruction_payload(file, text)?;
                cmd::channel::set_instruction(client, channel_id, instructions).await?
            }
            ChannelCmd::GetInstruction { channel_id } => {
                cmd::channel::get_instruction(client, channel_id).await?
            }
            ChannelCmd::ClearInstruction { channel_id } => {
                cmd::channel::clear_instruction(client, channel_id).await?
            }
            ChannelCmd::Skill(skill_cmd) => match skill_cmd {
                ChannelSkillCmd::Add {
                    channel_id,
                    source,
                    id,
                } => cmd::channel::skill_add(channel_id, source, id).await?,
                ChannelSkillCmd::Remove {
                    channel_id,
                    skill_id,
                } => cmd::channel::skill_remove(channel_id, skill_id).await?,
                ChannelSkillCmd::List { channel_id } => {
                    cmd::channel::skill_list(channel_id).await?
                }
            },
        },
        Cmd::Thread { sub } => match sub {
            ThreadCmd::Create {
                channel,
                root_message,
                title,
                resident_as,
                bootstrap_artifact,
            } => {
                cmd::thread::create(
                    client,
                    channel,
                    root_message,
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
            ThreadCmd::Follow { thread_id, muted } => {
                cmd::thread::follow(client, thread_id, muted).await?
            }
            ThreadCmd::Unfollow { thread_id } => cmd::thread::unfollow(client, thread_id).await?,
            ThreadCmd::Bootstrap {
                thread_id,
                channel,
                bootstrap_artifact,
            } => cmd::thread::bootstrap(client, channel, thread_id, bootstrap_artifact).await?,
            ThreadCmd::SetInstruction {
                thread_id,
                file,
                text,
            } => {
                let instructions = read_instruction_payload(file, text)?;
                cmd::thread::set_instruction(client, thread_id, instructions).await?
            }
            ThreadCmd::GetInstruction { thread_id } => {
                cmd::thread::get_instruction(client, thread_id).await?
            }
            ThreadCmd::ClearInstruction { thread_id } => {
                cmd::thread::clear_instruction(client, thread_id).await?
            }
            ThreadCmd::Skill(skill_cmd) => match skill_cmd {
                ThreadSkillCmd::Add {
                    thread_id,
                    source,
                    id,
                } => cmd::thread::skill_add(client, thread_id, source, id).await?,
                ThreadSkillCmd::Remove {
                    thread_id,
                    skill_id,
                } => cmd::thread::skill_remove(client, thread_id, skill_id).await?,
                ThreadSkillCmd::List { thread_id } => {
                    cmd::thread::skill_list(client, thread_id).await?
                }
            },
        },
        Cmd::Message { sub } => match sub {
            MessageCmd::Send {
                target,
                thread,
                to,
                private_to,
                text,
                intent,
                delivery_policy,
                if_latest,
                idempotency_key,
                attachment_ids,
                allow_escaped_newlines,
                allow_after_no_reply,
            } => {
                cmd::message::send(
                    client,
                    cfg.actor_id,
                    target,
                    thread,
                    to,
                    private_to,
                    text,
                    intent,
                    delivery_policy,
                    if_latest,
                    idempotency_key,
                    attachment_ids,
                    allow_escaped_newlines,
                    allow_after_no_reply,
                )
                .await?
            }
            MessageCmd::Ask {
                recipients,
                target,
                thread,
                text,
                if_latest,
                idempotency_key,
                attachment_ids,
                allow_escaped_newlines,
                allow_after_no_reply,
            } => {
                cmd::message::ask(
                    client,
                    cfg.actor_id,
                    target,
                    thread,
                    recipients,
                    text,
                    if_latest,
                    idempotency_key,
                    attachment_ids,
                    allow_escaped_newlines,
                    allow_after_no_reply,
                )
                .await?
            }
            MessageCmd::Read {
                target,
                thread,
                limit,
                before,
                include_private,
            } => {
                cmd::message::read(
                    client,
                    cfg.actor_id,
                    target,
                    thread,
                    limit,
                    before,
                    include_private,
                )
                .await?
            }
            MessageCmd::Search {
                query,
                target,
                limit,
            } => cmd::message::search(client, cfg.actor_id, query, target, limit).await?,
            MessageCmd::React { message_id, emoji } => {
                cmd::message::reaction_toggle(client, cfg.actor_id, message_id, emoji).await?
            }
        },
        Cmd::Inbox { sub } => match sub {
            InboxCmd::List {
                limit,
                state,
                no_ack,
            } => cmd::message::inbox_list(client, cfg.actor_id, limit, !no_ack, state).await?,
        },
        Cmd::Action { sub } => match sub {
            ActionCmd::Accept { message_id, option } => {
                cmd::action::respond(client, cfg.actor_id, message_id, option, true).await?
            }
            ActionCmd::Decline { message_id, option } => {
                cmd::action::respond(client, cfg.actor_id, message_id, option, false).await?
            }
        },
        Cmd::Task { sub } => match sub {
            TaskCmd::Create {
                source_message,
                title,
                description,
                owner,
                status,
                parent_source_message,
                parent_task,
                practice_contract_epoch,
            } => {
                cmd::task::create(
                    client,
                    cfg.actor_id,
                    source_message,
                    title,
                    description,
                    owner,
                    status,
                    parent_source_message,
                    parent_task,
                    practice_contract_epoch,
                )
                .await?
            }
            TaskCmd::List {
                channel,
                source_message,
                owner,
                statuses,
            } => cmd::task::list(client, channel, source_message, owner, statuses).await?,
            TaskCmd::Show { task_id } => cmd::task::show(client, task_id).await?,
            TaskCmd::Update {
                task_id,
                status,
                owner,
                result,
                artifact_ids,
            } => cmd::task::update(client, task_id, status, owner, result, artifact_ids).await?,
            TaskCmd::Claim {
                task_id,
                source_message,
                actor,
            } => cmd::task::claim(client, task_id, source_message, actor).await?,
            TaskCmd::Complete {
                task_id,
                result,
                artifact_ids,
            } => cmd::task::complete(client, task_id, result, artifact_ids).await?,
            TaskCmd::Reopen { task_id, owner } => cmd::task::reopen(client, task_id, owner).await?,
            TaskCmd::Cancel { task_id, result } => {
                cmd::task::cancel(client, task_id, result).await?
            }
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
                    source_message,
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
                        source_message,
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
                    result_message,
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
                        result_message,
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
        Cmd::Run { sub } => match sub {
            RunCmd::Open {
                target,
                delivery_id,
                start_reason,
                agent_config_version_id,
            } => {
                cmd::run::open(
                    client,
                    cfg.actor_id,
                    target,
                    delivery_id,
                    start_reason,
                    agent_config_version_id,
                )
                .await?
            }
            RunCmd::Append {
                run_id,
                status,
                frame_kind,
                payload_json,
            } => cmd::run::append(client, run_id, status, frame_kind, payload_json).await?,
            RunCmd::Ignore { run_id, reason } => cmd::run::ignore(client, run_id, reason).await?,
            RunCmd::Close { run_id, status } => cmd::run::close(client, run_id, status).await?,
        },
        Cmd::Coordination { sub } => match sub {
            CoordinationCmd::Propose {
                target,
                mode,
                decision_rule,
                participants,
                plan_json,
            } => {
                cmd::coordination::propose(
                    client,
                    target,
                    mode,
                    decision_rule,
                    participants,
                    plan_json,
                )
                .await?
            }
            CoordinationCmd::Commit { session_id } => {
                cmd::coordination::commit(client, session_id).await?
            }
            CoordinationCmd::Respond {
                session_id,
                accept,
                reject,
                reason,
            } => cmd::coordination::respond(client, session_id, accept || !reject, reason).await?,
            CoordinationCmd::Step {
                session_id,
                base_revision,
                step_type,
                output_json,
                message,
            } => {
                cmd::coordination::step(
                    client,
                    session_id,
                    base_revision,
                    step_type,
                    output_json,
                    message,
                )
                .await?
            }
            CoordinationCmd::Skip {
                session_id,
                base_revision,
                reason,
            } => cmd::coordination::skip(client, session_id, base_revision, reason).await?,
            CoordinationCmd::Reassign {
                session_id,
                from_actor_id,
                to_actor_id,
                base_revision,
            } => {
                cmd::coordination::reassign(
                    client,
                    session_id,
                    from_actor_id,
                    to_actor_id,
                    base_revision,
                )
                .await?
            }
        },
        Cmd::AgentConfig { sub } => match sub {
            AgentConfigCmd::Publish {
                actor_id,
                version,
                model,
                adapter,
                prompt,
                tools_json,
            } => {
                cmd::agent_config::publish(
                    client, actor_id, version, model, adapter, prompt, tools_json,
                )
                .await?
            }
            AgentConfigCmd::Activate {
                actor_id,
                version_id,
            } => cmd::agent_config::activate(client, actor_id, version_id).await?,
        },
        Cmd::Spec { sub } => match sub {
            SpecCmd::Apply {
                action_message_id,
                dry_run,
            } => {
                cmd::spec_apply::run(client, cfg.actor_id.clone(), action_message_id, dry_run)
                    .await?
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
        Cmd::Provider { .. } => unreachable!("handled before client setup"),
        Cmd::Guide { .. } => unreachable!("handled before client setup"),
        Cmd::Skill { .. } => unreachable!("handled before client setup"),
        Cmd::Mcp { .. } => unreachable!("handled before client setup"),
        Cmd::Memory { .. } => unreachable!("handled before client setup"),
        Cmd::Machine { sub } => match sub {
            MachineCmd::List => cmd::machine::list(client).await?,
            MachineCmd::Agent { sub } => match sub {
                MachineAgentCmd::Create {
                    machine,
                    provider,
                    actor_id,
                    name,
                    instructions,
                    instructions_file,
                    source_root,
                    model,
                    reasoning_effort,
                    wake_coalesce,
                    runtime_awareness,
                    no_autostart,
                } => {
                    cmd::machine::agent_create(
                        client,
                        machine,
                        provider,
                        actor_id,
                        name,
                        instructions,
                        instructions_file,
                        source_root,
                        model,
                        reasoning_effort,
                        wake_coalesce,
                        runtime_awareness,
                        !no_autostart,
                    )
                    .await?
                }
                MachineAgentCmd::Update {
                    machine,
                    actor_id,
                    name,
                    instructions,
                    instructions_file,
                    source_root,
                    model,
                    reasoning_effort,
                    wake_coalesce,
                    runtime_awareness,
                } => {
                    cmd::machine::agent_update(
                        client,
                        machine,
                        actor_id,
                        name,
                        instructions,
                        instructions_file,
                        source_root,
                        model,
                        reasoning_effort,
                        wake_coalesce,
                        runtime_awareness,
                    )
                    .await?
                }
                MachineAgentCmd::Remove { machine, actor_id } => {
                    cmd::machine::agent_remove(client, machine, actor_id).await?
                }
                MachineAgentCmd::Skill { sub } => match sub {
                    MachineAgentSkillCmd::List { machine, actor_id } => {
                        cmd::machine::agent_skill_list(client, machine, actor_id).await?
                    }
                    MachineAgentSkillCmd::Add {
                        machine,
                        actor_id,
                        source,
                    } => cmd::machine::agent_skill_add(client, machine, actor_id, source).await?,
                    MachineAgentSkillCmd::Remove {
                        machine,
                        actor_id,
                        skill,
                    } => cmd::machine::agent_skill_remove(client, machine, actor_id, skill).await?,
                },
            },
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
        Cmd::Group { sub } => match sub {
            GroupCmd::Create {
                channel,
                name,
                display,
                members,
                wake_agents,
            } => cmd::group::create(client, channel, name, display, members, wake_agents).await?,
            GroupCmd::List { channel } => cmd::group::list(client, channel).await?,
            GroupCmd::AddMember { group_id, actor_id } => {
                cmd::group::add_member(client, group_id, actor_id).await?
            }
            GroupCmd::RemoveMember { group_id, actor_id } => {
                cmd::group::remove_member(client, group_id, actor_id).await?
            }
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

fn local_agent_spec_actor_kind(actor_id: &str) -> Result<Option<ConnectionActorKind>> {
    local_agent_spec_actor_kind_at(&config::config_dir(), actor_id)
}

fn local_agent_spec_actor_kind_at(
    config_dir: &Path,
    actor_id: &str,
) -> Result<Option<ConnectionActorKind>> {
    let actor_path = Path::new(actor_id);
    if actor_id.trim().is_empty()
        || actor_path.is_absolute()
        || actor_path.components().count() != 1
    {
        return Ok(None);
    }
    let agents_dir = config_dir.join("agents");
    let nested = agents_dir.join(actor_id).join("spec.json");
    let flat = agents_dir.join(format!("{actor_id}.json"));
    let path = if nested.is_file() {
        nested
    } else if flat.is_file() {
        flat
    } else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read local AgentSpec {}", path.display()))?;
    let spec: AgentSpec = serde_json::from_str(&text)
        .with_context(|| format!("parse local AgentSpec {}", path.display()))?;
    if spec.actor.id != actor_id {
        anyhow::bail!(
            "local AgentSpec {} declares actor `{}`, expected `{actor_id}`",
            path.display(),
            spec.actor.id
        );
    }
    Ok(Some(match spec.actor.kind {
        ActorKind::Human => ConnectionActorKind::Human,
        ActorKind::Agent => ConnectionActorKind::Agent,
        ActorKind::Service => ConnectionActorKind::Service,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_actor_flags_parse_globally() {
        let args = Args::try_parse_from([
            "loom",
            "--as",
            "am.robot-a",
            "--actor-kind",
            "agent",
            "--observer",
            "channel",
            "list",
        ])
        .expect("parse agent observer connection flags");

        assert_eq!(args.actor.as_deref(), Some("am.robot-a"));
        assert_eq!(args.actor_kind, Some(ConnectionActorKind::Agent));
        assert!(args.observer);
    }

    #[test]
    fn local_agent_spec_infers_agent_observer_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let spec_dir = temp.path().join("agents").join("am.robot-a");
        std::fs::create_dir_all(&spec_dir).expect("create spec dir");
        std::fs::write(
            spec_dir.join("spec.json"),
            r#"{
                "actor": {
                    "id": "am.robot-a",
                    "kind": "agent",
                    "displayName": "Robot A"
                },
                "providerRef": {
                    "id": "qoder",
                    "mode": "print"
                }
            }"#,
        )
        .expect("write AgentSpec");

        assert_eq!(
            local_agent_spec_actor_kind_at(temp.path(), "am.robot-a").expect("infer actor kind"),
            Some(ConnectionActorKind::Agent)
        );
    }

    #[test]
    fn malformed_local_agent_spec_fails_before_human_fallback() {
        let temp = tempfile::tempdir().expect("tempdir");
        let spec_dir = temp.path().join("agents").join("am.robot-a");
        std::fs::create_dir_all(&spec_dir).expect("create spec dir");
        std::fs::write(spec_dir.join("spec.json"), "{not-json").expect("write malformed spec");

        let error = local_agent_spec_actor_kind_at(temp.path(), "am.robot-a")
            .expect_err("malformed local AgentSpec must not fall back to human");

        assert!(error.to_string().contains("parse local AgentSpec"));
    }

    #[test]
    fn message_send_accepts_direct_recipient_and_delivery_options() {
        let args = Args::try_parse_from([
            "loom",
            "--json",
            "message",
            "send",
            "--to",
            "actor_reviewer",
            "--intent",
            "request_action",
            "--delivery-policy",
            "wake_agent",
            "--if-latest",
            "msg_latest",
            "--idempotency-key",
            "review-request-42",
            "--allow-after-no-reply",
            "--text",
            "please review",
        ])
        .expect("parse message send");

        match args.cmd {
            Cmd::Message {
                sub:
                    MessageCmd::Send {
                        target,
                        to,
                        private_to,
                        intent,
                        delivery_policy,
                        if_latest,
                        idempotency_key,
                        allow_after_no_reply,
                        text,
                        ..
                    },
            } => {
                assert_eq!(target, None);
                assert_eq!(to.as_deref(), Some("actor_reviewer"));
                assert!(private_to.is_empty());
                assert_eq!(intent.as_deref(), Some("request_action"));
                assert_eq!(delivery_policy.as_deref(), Some("wake_agent"));
                assert_eq!(if_latest.as_deref(), Some("msg_latest"));
                assert_eq!(idempotency_key.as_deref(), Some("review-request-42"));
                assert!(allow_after_no_reply);
                assert_eq!(text.as_deref(), Some("please review"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_ask_accepts_multiple_recipients_and_target_options() {
        let args = Args::try_parse_from([
            "loom",
            "--json",
            "message",
            "ask",
            "@actor_a",
            "@actor_b",
            "@all",
            "--target",
            "#chan_123:msg_root",
            "--if-latest",
            "msg_latest",
            "--idempotency-key",
            "ask-reviewers-42",
            "--allow-after-no-reply",
            "--text",
            "please respond",
        ])
        .expect("parse message ask");

        match args.cmd {
            Cmd::Message {
                sub:
                    MessageCmd::Ask {
                        recipients,
                        target,
                        text,
                        if_latest,
                        idempotency_key,
                        allow_after_no_reply,
                        ..
                    },
            } => {
                assert_eq!(recipients, vec!["@actor_a", "@actor_b", "@all"]);
                assert_eq!(target.as_deref(), Some("#chan_123:msg_root"));
                assert_eq!(if_latest.as_deref(), Some("msg_latest"));
                assert_eq!(idempotency_key.as_deref(), Some("ask-reviewers-42"));
                assert!(allow_after_no_reply);
                assert_eq!(text.as_deref(), Some("please respond"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_send_accepts_thread_destination() {
        let args = Args::try_parse_from([
            "loom",
            "--json",
            "message",
            "send",
            "--thread",
            "thread_abc",
            "--text",
            "reply in thread",
        ])
        .expect("parse message send");

        match args.cmd {
            Cmd::Message {
                sub:
                    MessageCmd::Send {
                        target,
                        thread,
                        text,
                        ..
                    },
            } => {
                assert_eq!(target, None);
                assert_eq!(thread.as_deref(), Some("thread_abc"));
                assert_eq!(text.as_deref(), Some("reply in thread"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_send_accepts_same_scope_private_recipient() {
        let args = Args::try_parse_from([
            "loom",
            "--json",
            "message",
            "send",
            "--private-to",
            "@actor_player",
            "--text",
            "your role is seer",
        ])
        .expect("parse message send");

        match args.cmd {
            Cmd::Message {
                sub:
                    MessageCmd::Send {
                        target,
                        to,
                        private_to,
                        text,
                        ..
                    },
            } => {
                assert_eq!(target, None);
                assert_eq!(to, None);
                assert_eq!(private_to, vec!["@actor_player"]);
                assert_eq!(text.as_deref(), Some("your role is seer"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_read_accepts_include_private() {
        let args = Args::try_parse_from([
            "loom",
            "--json",
            "message",
            "read",
            "--target",
            "#chan_123:msg_root",
            "--include-private",
        ])
        .expect("parse message read");

        match args.cmd {
            Cmd::Message {
                sub:
                    MessageCmd::Read {
                        target,
                        include_private,
                        ..
                    },
            } => {
                assert_eq!(target.as_deref(), Some("#chan_123:msg_root"));
                assert!(include_private);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_react_accepts_message_id_and_emoji() {
        let args = Args::try_parse_from(["loom", "message", "react", "msg_123", "✅"])
            .expect("parse message react");

        match args.cmd {
            Cmd::Message {
                sub: MessageCmd::React { message_id, emoji },
            } => {
                assert_eq!(message_id, "msg_123");
                assert_eq!(emoji, "✅");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn task_claim_accepts_source_message_guard() {
        let args = Args::try_parse_from([
            "loom",
            "task",
            "claim",
            "--source-message",
            "msg_root",
            "--actor",
            "actor_worker",
        ])
        .expect("parse task claim");

        match args.cmd {
            Cmd::Task {
                sub:
                    TaskCmd::Claim {
                        task_id,
                        source_message,
                        actor,
                    },
            } => {
                assert_eq!(task_id, None);
                assert_eq!(source_message.as_deref(), Some("msg_root"));
                assert_eq!(actor.as_deref(), Some("actor_worker"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn inbox_list_is_the_directed_inbox_surface() {
        let args = Args::try_parse_from([
            "loom", "inbox", "list", "--no-ack", "--limit", "7", "--state", "all",
        ])
        .expect("parse inbox list");

        match args.cmd {
            Cmd::Inbox {
                sub:
                    InboxCmd::List {
                        limit,
                        state,
                        no_ack,
                    },
            } => {
                assert_eq!(limit, 7);
                assert_eq!(state, "all");
                assert!(no_ack);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn channel_delete_parses_without_cascade_flag() {
        let args = Args::try_parse_from(["loom", "channel", "delete", "chan_123"])
            .expect("parse channel delete");

        match args.cmd {
            Cmd::Channel {
                sub:
                    ChannelCmd::Delete {
                        channel_id,
                        cascade,
                    },
            } => {
                assert_eq!(channel_id, "chan_123");
                assert!(!cascade);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn group_create_accepts_members_and_wake_policy() {
        let args = Args::try_parse_from([
            "loom",
            "group",
            "create",
            "--channel",
            "chan_123",
            "reviewers",
            "--member",
            "actor_alice",
            "--member",
            "actor_agent_reviewer",
            "--wake-agents",
        ])
        .expect("parse group create");

        match args.cmd {
            Cmd::Group {
                sub:
                    GroupCmd::Create {
                        channel,
                        name,
                        members,
                        wake_agents,
                        ..
                    },
            } => {
                assert_eq!(channel, "chan_123");
                assert_eq!(name, "reviewers");
                assert_eq!(members, vec!["actor_alice", "actor_agent_reviewer"]);
                assert!(wake_agents);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn thread_follow_and_task_shortcuts_parse() {
        let follow = Args::try_parse_from(["loom", "thread", "follow", "thread_123", "--muted"])
            .expect("parse thread follow");
        match follow.cmd {
            Cmd::Thread {
                sub: ThreadCmd::Follow { thread_id, muted },
            } => {
                assert_eq!(thread_id, "thread_123");
                assert!(muted);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let complete = Args::try_parse_from([
            "loom",
            "task",
            "complete",
            "task_123",
            "--result",
            "done",
            "--artifact-id",
            "art_1",
        ])
        .expect("parse task complete");
        match complete.cmd {
            Cmd::Task {
                sub:
                    TaskCmd::Complete {
                        task_id,
                        result,
                        artifact_ids,
                    },
            } => {
                assert_eq!(task_id, "task_123");
                assert_eq!(result.as_deref(), Some("done"));
                assert_eq!(artifact_ids, vec!["art_1"]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn run_open_accepts_explicit_local_start() {
        let args = Args::try_parse_from([
            "loom",
            "run",
            "open",
            "--target",
            "#chan_123",
            "--start-reason",
            "manual",
            "--agent-config-version-id",
            "cfg_v1",
        ])
        .expect("parse run open");

        match args.cmd {
            Cmd::Run {
                sub:
                    RunCmd::Open {
                        target,
                        start_reason,
                        agent_config_version_id,
                        ..
                    },
            } => {
                assert_eq!(target, "#chan_123");
                assert_eq!(start_reason.as_deref(), Some("manual"));
                assert_eq!(agent_config_version_id, "cfg_v1");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn run_ignore_accepts_optional_run_and_reason() {
        let args = Args::try_parse_from([
            "loom",
            "run",
            "ignore",
            "--run-id",
            "run_123",
            "--reason",
            "not directed at me",
        ])
        .expect("parse run ignore");

        match args.cmd {
            Cmd::Run {
                sub: RunCmd::Ignore { run_id, reason },
            } => {
                assert_eq!(run_id.as_deref(), Some("run_123"));
                assert_eq!(reason.as_deref(), Some("not directed at me"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn coordination_propose_accepts_participants_and_plan() {
        let args = Args::try_parse_from([
            "loom",
            "coordination",
            "propose",
            "--target",
            "#chan_123",
            "--mode",
            "sequential",
            "--participant",
            "actor_a",
            "--participant",
            "actor_b",
            "--plan-json",
            r#"{"goal":"count"}"#,
        ])
        .expect("parse coordination propose");

        match args.cmd {
            Cmd::Coordination {
                sub:
                    CoordinationCmd::Propose {
                        target,
                        mode,
                        participants,
                        plan_json,
                        ..
                    },
            } => {
                assert_eq!(target, "#chan_123");
                assert_eq!(mode, "sequential");
                assert_eq!(participants, vec!["actor_a", "actor_b"]);
                assert_eq!(plan_json.as_deref(), Some(r#"{"goal":"count"}"#));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn agent_config_publish_accepts_version_metadata() {
        let args = Args::try_parse_from([
            "loom",
            "agent-config",
            "publish",
            "actor_agent_bot",
            "--version",
            "v1",
            "--model",
            "noop",
            "--tools-json",
            "[]",
        ])
        .expect("parse agent-config publish");

        match args.cmd {
            Cmd::AgentConfig {
                sub:
                    AgentConfigCmd::Publish {
                        actor_id,
                        version,
                        model,
                        tools_json,
                        ..
                    },
            } => {
                assert_eq!(actor_id, "actor_agent_bot");
                assert_eq!(version.as_deref(), Some("v1"));
                assert_eq!(model.as_deref(), Some("noop"));
                assert_eq!(tools_json.as_deref(), Some("[]"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn provider_add_accepts_replace_flag() {
        let args = Args::try_parse_from(["loom", "provider", "add", "demo.json", "--replace"])
            .expect("parse provider add --replace");

        match args.cmd {
            Cmd::Provider {
                sub: ProviderCmd::Add { path, replace },
            } => {
                assert_eq!(path, PathBuf::from("demo.json"));
                assert!(replace);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn provider_example_accepts_builtin_flags() {
        let args = Args::try_parse_from([
            "loom",
            "provider",
            "example",
            "--claude",
            "--opencode",
            "--kimi",
            "--zcode",
        ])
        .expect("parse provider example");

        match args.cmd {
            Cmd::Provider {
                sub:
                    ProviderCmd::Example {
                        claude,
                        opencode,
                        kimi,
                        zcode,
                        ..
                    },
            } => {
                assert!(claude);
                assert!(opencode);
                assert!(kimi);
                assert!(zcode);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn skill_materialize_accepts_dedicated_output_directory() {
        let args = Args::try_parse_from([
            "loom",
            "--json",
            "skill",
            "materialize",
            "loom",
            "--output",
            "/tmp/loom-skill",
        ])
        .expect("parse skill materialize");

        assert!(args.json);
        match args.cmd {
            Cmd::Skill {
                sub: SkillCmd::Materialize { id, output },
            } => {
                assert_eq!(id, "loom");
                assert_eq!(output, PathBuf::from("/tmp/loom-skill"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn skill_materialize_rejects_unknown_embedded_skill() {
        let error = Args::try_parse_from([
            "loom",
            "skill",
            "materialize",
            "unknown",
            "--output",
            "/tmp/unknown-skill",
        ])
        .expect_err("unknown embedded skill should be rejected");

        assert!(error.to_string().contains("possible values: loom"));
    }

    #[test]
    fn machine_agent_skill_commands_parse() {
        let add_args = Args::try_parse_from([
            "loom",
            "machine",
            "agent",
            "skill",
            "add",
            "--machine",
            "macmini",
            "--actor-id",
            "actor_impl",
            "/tmp/cloud-dev",
        ])
        .expect("parse machine agent skill add");

        match add_args.cmd {
            Cmd::Machine {
                sub:
                    MachineCmd::Agent {
                        sub:
                            MachineAgentCmd::Skill {
                                sub:
                                    MachineAgentSkillCmd::Add {
                                        machine,
                                        actor_id,
                                        source,
                                    },
                            },
                    },
            } => {
                assert_eq!(machine, "macmini");
                assert_eq!(actor_id, "actor_impl");
                assert_eq!(source, PathBuf::from("/tmp/cloud-dev"));
            }
            other => panic!("unexpected command: {other:?}"),
        }

        Args::try_parse_from([
            "loom",
            "machine",
            "agent",
            "skill",
            "list",
            "--machine",
            "macmini",
            "--actor-id",
            "actor_impl",
        ])
        .expect("parse machine agent skill list");

        Args::try_parse_from([
            "loom",
            "machine",
            "agent",
            "skill",
            "remove",
            "--machine",
            "macmini",
            "--actor-id",
            "actor_impl",
            "cloud-dev",
        ])
        .expect("parse machine agent skill remove");
    }

    #[test]
    fn machine_agent_update_parses_all_options() {
        let args = Args::try_parse_from([
            "loom",
            "machine",
            "agent",
            "update",
            "--machine",
            "macmini",
            "--actor-id",
            "actor_impl",
            "--name",
            "Implementation Agent",
            "--instructions-file",
            "AGENTS.md",
            "--source-root",
            "/repo/qca",
            "--model",
            "gpt-5",
            "--reasoning-effort",
            "high",
            "--wake-coalesce",
            "false",
            "--runtime-awareness",
            "hidden",
        ])
        .expect("parse machine agent update");

        match args.cmd {
            Cmd::Machine {
                sub:
                    MachineCmd::Agent {
                        sub:
                            MachineAgentCmd::Update {
                                machine,
                                actor_id,
                                name,
                                instructions,
                                instructions_file,
                                source_root,
                                model,
                                reasoning_effort,
                                wake_coalesce,
                                runtime_awareness,
                            },
                    },
            } => {
                assert_eq!(machine, "macmini");
                assert_eq!(actor_id, "actor_impl");
                assert_eq!(name.as_deref(), Some("Implementation Agent"));
                assert_eq!(instructions, None);
                assert_eq!(instructions_file, Some(PathBuf::from("AGENTS.md")));
                assert_eq!(source_root, Some(PathBuf::from("/repo/qca")));
                assert_eq!(model.as_deref(), Some("gpt-5"));
                assert_eq!(reasoning_effort.as_deref(), Some("high"));
                assert_eq!(wake_coalesce, Some(false));
                assert_eq!(runtime_awareness.as_deref(), Some("hidden"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn machine_agent_update_requires_actor_id() {
        let error = Args::try_parse_from([
            "loom",
            "machine",
            "agent",
            "update",
            "--machine",
            "macmini",
            "--name",
            "Implementation Agent",
        ])
        .expect_err("machine agent update must require --actor-id");

        assert!(error.to_string().contains("--actor-id"));
    }

    #[test]
    fn retired_event_say_and_daemon_commands_are_not_public_cli() {
        let cases: &[&[&str]] = &[
            &["loom", "event", "list", "--in", "thread_1"],
            &["loom", "say", "--in", "thread_1", "hello"],
            &["loom", "daemon", "--list-providers"],
        ];
        for argv in cases {
            assert!(
                Args::try_parse_from(*argv).is_err(),
                "legacy command parsed unexpectedly: {argv:?}"
            );
        }
    }
}
