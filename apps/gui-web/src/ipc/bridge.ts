import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Actor,
  AgentInfo,
  Artifact,
  ArtifactReadResult,
  Channel,
  DesktopConfig,
  DeliveryListEntry,
  ActorDeliveryState,
  HumanAccount,
  JoiEvent,
  MachineInfo,
  Reminder,
  ReminderStatus,
  ScopeRef,
  StreamUpdate,
  Task,
  TaskArtifactLink,
  TaskAssignment,
  TaskAssignmentStatus,
  TaskAssignmentType,
  TaskChangeAckDisposition,
  TaskChangeDelivery,
  TaskFact,
  TaskProjection,
  TaskProjectionHealth,
  TaskRef,
  TaskStatus,
  Thread,
  TurnStreamDelta,
  WorkspaceLease,
  Workspace,
} from "./types";

function hasTauriRuntime() {
  return (
    typeof window !== "undefined" &&
    Boolean(
      (window as Window & { __TAURI_INTERNALS__?: unknown })
        .__TAURI_INTERNALS__,
    )
  );
}

function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!hasTauriRuntime()) {
    return Promise.reject(
      new Error(`Tauri runtime unavailable for ${command}`),
    );
  }
  return tauriInvoke<T>(command, args);
}

function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    return Promise.resolve(() => {});
  }
  return tauriListen<T>(event, handler);
}

// ---- workspace profile management ----

export async function workspacesList(): Promise<DesktopConfig> {
  return invoke("workspaces_list");
}

export async function workspaceAdd(args: {
  name: string;
  serverUrl: string;
  activate?: boolean;
}): Promise<DesktopConfig> {
  return invoke("workspace_add", { args });
}

export async function workspaceRemove(id: string): Promise<DesktopConfig> {
  return invoke("workspace_remove", { args: { id } });
}

export async function setActiveWorkspace(id: string): Promise<DesktopConfig> {
  return invoke("set_active_workspace", { args: { id } });
}

// ---- local human account ----

export async function accountGet(): Promise<HumanAccount | null> {
  return invoke("account_get");
}

export async function accountLogin(provider: "buc"): Promise<{
  account: HumanAccount;
  config: DesktopConfig;
}> {
  return invoke("account_login", { args: { provider } });
}

export async function accountLogout(): Promise<DesktopConfig> {
  return invoke("account_logout");
}

export async function avatarCachedUrl(url: string): Promise<string> {
  return invoke("avatar_cached_url", { args: { url } });
}

// ---- connection ----

export async function connect(workspaceId: string): Promise<{
  workspace: Workspace;
  open: unknown;
}> {
  return invoke("connect", { args: { workspaceId } });
}

export async function disconnect(): Promise<void> {
  await invoke("disconnect");
}

// ---- server RPC pass-through ----

export async function channelList(): Promise<{ channels: Channel[] }> {
  return invoke("channel_list");
}

export async function channelCreate(params: {
  title: string;
  actorId?: string;
}): Promise<{ channel: Channel }> {
  return invoke("channel_create", { params });
}

export async function channelUpdate(params: {
  channelId: string;
  title: string;
}): Promise<{ channel: Channel }> {
  return invoke("channel_update", { params });
}

export async function channelDelete(params: {
  channelId: string;
  cascade?: boolean;
}): Promise<{ deleted: boolean; deletedThreads?: number }> {
  return invoke("channel_delete", { params });
}

export async function channelInvite(params: {
  channelId: string;
  actorId: string;
}): Promise<{ channel: Channel }> {
  return invoke("channel_invite", { params });
}

export async function channelRevoke(params: {
  channelId: string;
  actorId: string;
}): Promise<{ channel: Channel }> {
  return invoke("channel_revoke", { params });
}

export async function threadCreate(params: {
  channelId: string;
  title: string;
  rootEventId: string;
}): Promise<{ thread: Thread }> {
  return invoke("thread_create", { params });
}

export async function threadUpdate(params: {
  threadId: string;
  title: string;
}): Promise<{ thread: Thread }> {
  return invoke("thread_update", { params });
}

export async function threadArchive(params: {
  threadId: string;
  archived?: boolean;
}): Promise<{ thread: Thread }> {
  return invoke("thread_archive", { params });
}

export async function threadDelete(params: {
  threadId: string;
}): Promise<{ deleted: boolean }> {
  return invoke("thread_delete", { params });
}

export async function threadList(
  channelId?: string,
  options?: { archived?: boolean },
): Promise<{ threads: Thread[] }> {
  return invoke("thread_list", {
    params: {
      ...(channelId ? { channelId } : {}),
      ...(options?.archived ? { archived: true } : {}),
    },
  });
}

export async function channelMembers(
  channelId: string,
): Promise<{ members: Actor[] }> {
  return invoke("channel_members", { params: { channelId } });
}

export async function scopeSubscribe(scope: ScopeRef): Promise<unknown> {
  return invoke("scope_subscribe", { params: { scope } });
}

export async function scopeUnsubscribe(scope: ScopeRef): Promise<unknown> {
  return invoke("scope_unsubscribe", { params: { scope } });
}

export async function scopeRead(
  scope: ScopeRef,
  limit = 100,
  beforeEventId?: string,
): Promise<{ events: JoiEvent[]; pageInfo: { hasMore: boolean } }> {
  return invoke("scope_read", {
    params: {
      scope,
      limit,
      ...(beforeEventId ? { beforeEventId } : {}),
    },
  });
}

export async function eventAppend(input: {
  type: string;
  actorId: string;
  scope: ScopeRef;
  turnId?: string;
  payload?: unknown;
  relations?: unknown[];
}): Promise<{ event: JoiEvent }> {
  return invoke("event_append", { params: { event: input } });
}

export async function taskCreate(params: {
  sourceEventId: string;
  title?: string;
  description?: string;
  requesterActorId?: string;
  ownerActorId?: string;
  status?: TaskStatus;
}): Promise<{ task: Task }> {
  return invoke("task_create", { params });
}

export async function taskList(params?: {
  channelId?: string;
  sourceEventId?: string;
  ownerActorId?: string;
  statuses?: TaskStatus[];
}): Promise<{ tasks: Task[] }> {
  return invoke("task_list", { params: params ?? {} });
}

export async function taskGet(taskId: string): Promise<{
  task: Task;
  assignments: TaskAssignment[];
  refs?: TaskRef[];
  artifactLinks?: TaskArtifactLink[];
  facts?: TaskFact[];
  projections?: TaskProjection[];
}> {
  return invoke("task_get", { params: { taskId } });
}

export async function taskUpdate(params: {
  taskId: string;
  status?: TaskStatus;
  ownerActorId?: string;
  resultSummary?: string;
  artifactIds?: string[];
  appendArtifactIds?: string[];
}): Promise<{ task: Task }> {
  return invoke("task_update", { params });
}

export async function taskAssignmentCreate(params: {
  taskId: string;
  fromActorId?: string;
  toActorId: string;
  type: TaskAssignmentType;
  instruction: string;
  contract?: unknown;
  idempotencyKey?: string;
}): Promise<{ assignment: TaskAssignment; task: Task; event: JoiEvent }> {
  return invoke("task_assignment_create", { params });
}

export async function taskAssignmentUpdate(params: {
  assignmentId: string;
  status?: TaskAssignmentStatus;
  resultEventId?: string;
  resultSummary?: string;
  resultEnvelope?: unknown;
  resultArtifactIds?: string[];
  resultFactIds?: string[];
  evidenceRefs?: string[];
}): Promise<{ assignment: TaskAssignment; task: Task }> {
  return invoke("task_assignment_update", { params });
}

export async function taskRefFind(params: {
  channelId?: string;
  kind: string;
  subtype?: string;
  normalized: string;
  confidence?: string;
  status?: string;
}): Promise<{ refs: TaskRef[]; tasks: Task[] }> {
  return invoke("task_ref_find", { params });
}

export async function taskArtifactList(params: {
  taskId: string;
  status?: string;
}): Promise<{ links: TaskArtifactLink[] }> {
  return invoke("task_artifact_list", { params });
}

export async function taskFactList(params: {
  taskId: string;
  kind?: string;
  status?: string;
  targetKey?: string;
}): Promise<{ facts: TaskFact[] }> {
  return invoke("task_fact_list", { params });
}

export async function taskProjectionGet(params: {
  taskId: string;
  projectionType?: string;
}): Promise<{ projection?: TaskProjection | null; health: TaskProjectionHealth }> {
  return invoke("task_projection_get", { params });
}

export async function taskAssignmentContext(
  assignmentId: string,
): Promise<{
  task: Task;
  assignment: TaskAssignment;
  refs: TaskRef[];
  artifactLinks: TaskArtifactLink[];
  facts: TaskFact[];
  projection?: TaskProjection | null;
  guards: unknown;
}> {
  return invoke("task_assignment_context", { params: { assignmentId } });
}

export async function taskAssignmentPreflight(params: {
  assignmentId: string;
  targetKey?: string;
  head?: string;
  effect?: string;
}): Promise<{ preflight: unknown }> {
  return invoke("task_assignment_preflight", { params });
}

export async function taskChangeList(params?: {
  taskId?: string;
  includeHandled?: boolean;
  afterCursor?: number;
  limit?: number;
}): Promise<{ deliveries: TaskChangeDelivery[] }> {
  return invoke("task_change_list", { params: params ?? {} });
}

export async function taskChangeAck(params: {
  changeId: string;
  disposition: TaskChangeAckDisposition;
  resultRefIds?: string[];
  reason?: string;
}): Promise<{ delivery: TaskChangeDelivery }> {
  return invoke("task_change_ack", { params });
}

export async function deliveryList(params: {
  actorId: string;
  state?: ActorDeliveryState;
  limit?: number;
  cursor?: string;
}): Promise<{ deliveries: DeliveryListEntry[]; nextCursor?: string }> {
  return invoke("delivery_list", { params });
}

export async function taskWorkspaceLeaseList(params?: {
  resourceKey?: string;
  assignmentId?: string;
  activeOnly?: boolean;
}): Promise<{ leases: WorkspaceLease[] }> {
  return invoke("task_workspace_lease_list", { params: params ?? {} });
}

export async function artifactPublish(params: {
  createdBy: string;
  scope?: ScopeRef;
  ingress: {
    kind: "inline_text" | "file_bytes";
    name: string;
    mediaType?: string;
    text?: string;
    bytes?: number[];
  };
}): Promise<{ artifact: Artifact }> {
  return invoke("artifact_publish", { params });
}

export async function artifactGet(params: {
  artifactId?: string;
  artifactUri?: string;
}): Promise<{ artifact: Artifact }> {
  return invoke("artifact_get", { params });
}

export async function artifactRead(params: {
  artifactId: string;
  offset?: number;
  maxBytes?: number;
}): Promise<ArtifactReadResult> {
  return invoke("artifact_read", { params });
}

export async function turnClose(
  turnId: string,
  status: "closed" | "cancelled" = "cancelled",
): Promise<unknown> {
  return invoke("turn_close", { params: { turnId, status } });
}

export async function reminderList(params: {
  actorId: string;
  statuses?: ReminderStatus[];
  all?: boolean;
}): Promise<{ reminders: Reminder[] }> {
  return invoke("reminder_list", {
    params: {
      actorId: params.actorId,
      statuses: params.statuses ?? [],
      all: params.all ?? false,
    },
  });
}

export async function actorList(): Promise<{ actors: Actor[] }> {
  return invoke("actor_list");
}

export async function actorUpsert(actor: Actor): Promise<{ actor: Actor }> {
  return invoke("actor_upsert", { params: { actor } });
}

export async function actorDelete(actorId: string): Promise<{ deleted: boolean }> {
  return invoke("actor_delete", { params: { actorId } });
}

export async function agentList(): Promise<{
  agents: AgentInfo[];
}> {
  return invoke("agent_list");
}

export async function agentCreate(args: {
  machineId?: string;
  providerId: string;
  actorId?: string;
  name: string;
  description?: string;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
}): Promise<AgentInfo> {
  return invoke("agent_create", { args });
}

export async function agentRemove(actorId: string): Promise<{
  agents: AgentInfo[];
}> {
  return invoke("agent_remove", { args: { actorId } });
}

export async function agentUpdate(args: {
  machineId?: string;
  actorId: string;
  displayName?: string;
  description?: string;
  providerId?: string;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
}): Promise<AgentInfo> {
  return invoke("agent_update", { args });
}

export async function agentProfileFileRead(args: {
  machineId: string;
  actorId: string;
  file: "identity" | "soul";
}): Promise<{ path: string; text: string; sha256?: string | null }> {
  return invoke("agent_profile_file_read", { args });
}

export async function agentProfileFileWrite(args: {
  machineId: string;
  actorId: string;
  file: "identity" | "soul";
  text: string;
  baseSha256?: string | null;
}): Promise<{ path: string; text: string; sha256?: string | null }> {
  return invoke("agent_profile_file_write", { args });
}

export async function machineList(): Promise<{ machines: MachineInfo[] }> {
  return invoke("machine_list");
}

export async function machineCheck(): Promise<{ machines: MachineInfo[] }> {
  return invoke("machine_check");
}

export async function openLocalPath(path: string): Promise<void> {
  return invoke("open_local_path", { args: { path } });
}

export async function machineCreate(args: {
  name: string;
  dataRoot?: string;
}): Promise<{ machines: MachineInfo[] }> {
  return invoke("machine_create", { args });
}

export async function machineRemove(machineId: string): Promise<{
  machines: MachineInfo[];
}> {
  return invoke("machine_remove", { args: { machineId } });
}

export async function machineAgentCreate(args: {
  machineId: string;
  providerId: string;
  actorId?: string;
  name: string;
  description?: string;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
}): Promise<{ machines: MachineInfo[] }> {
  return invoke("machine_agent_create", { args });
}

export async function machineAgentRemove(
  machineId: string,
  actorId: string,
): Promise<{ machines: MachineInfo[] }> {
  return invoke("machine_agent_remove", { args: { machineId, actorId } });
}

// ---- inbound (event listeners) ----

export function onStream(cb: (u: StreamUpdate) => void): Promise<UnlistenFn> {
  return listen<StreamUpdate>("joi://stream", (e) => cb(e.payload));
}

export function onStreamDelta(
  cb: (u: TurnStreamDelta) => void,
): Promise<UnlistenFn> {
  return listen<TurnStreamDelta>("joi://stream-delta", (e) => cb(e.payload));
}

export function onConnection(
  cb: (u: { state: "open" } | { state: "closed"; reason?: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ state: "open" | "closed"; reason?: string }>(
    "joi://connection",
    (e) => cb(e.payload),
  );
}
