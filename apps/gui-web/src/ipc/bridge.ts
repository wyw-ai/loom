import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AgentFileListResult,
  AgentFileReadResult,
  AgentFileRoot,
  AgentFileWriteResult,
  AgentInfo,
  AgentPromptPreviewResult,
  Actor,
  AudienceRef,
  Channel,
  DesktopConfig,
  DeliveryPolicy,
  DeliveryState,
  HumanAccount,
  InboxListEntry,
  MachineAgentProviderInfo,
  MachineListResult,
  Message,
  MessageIntent,
  Run,
  ScopeRef,
  StreamUpdate,
  Task,
  TaskAssignmentType,
  Thread,
  Workspace,
} from "./types";

export type LoginProvider = "google" | "github";
export type AccountLoginProviderStatus = {
  provider: LoginProvider;
  displayName: string;
  available: boolean;
  missingEnv?: string | null;
};
export type AccountAuthStatus = {
  providers: AccountLoginProviderStatus[];
};
export type AccountLocalDefaults = {
  userId: string;
  nickname: string;
  actorId: string;
};

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
    return Promise.reject(new Error(`Tauri runtime unavailable for ${command}`));
  }
  return tauriInvoke<T>(command, args);
}

function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) return Promise.resolve(() => {});
  return tauriListen<T>(event, handler);
}

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

export async function accountGet(): Promise<HumanAccount | null> {
  return invoke("account_get");
}

export async function accountAuthStatus(): Promise<AccountAuthStatus> {
  return invoke("account_auth_status");
}

export async function accountLocalDefaults(): Promise<AccountLocalDefaults> {
  return invoke("account_local_defaults");
}

export async function accountLogin(provider: LoginProvider): Promise<{
  account: HumanAccount;
  config: DesktopConfig;
}> {
  return invoke("account_login", { args: { provider } });
}

export async function accountSetLocal(args: {
  userId: string;
  nickname: string;
  actorId: string;
}): Promise<{
  account: HumanAccount;
  config: DesktopConfig;
}> {
  return invoke("account_set_local", { args });
}

export async function accountLogout(): Promise<DesktopConfig> {
  return invoke("account_logout");
}

export async function avatarCachedUrl(url: string): Promise<string> {
  return invoke("avatar_cached_url", { args: { url } });
}

export async function connect(workspaceId: string): Promise<{
  workspace: Workspace;
  open: unknown;
}> {
  return invoke("connect", { args: { workspaceId } });
}

export async function disconnect(): Promise<void> {
  await invoke("disconnect");
}

export async function channelList(): Promise<{ channels: Channel[] }> {
  return invoke("channel_list");
}

export async function channelCreate(params: {
  title: string;
  actorId?: string;
  topic?: string;
}): Promise<{ channel: Channel }> {
  return invoke("channel_create", { params });
}

export async function channelUpdate(params: {
  channelId: string;
  title: string;
  topic?: string;
}): Promise<{ channel: Channel }> {
  return invoke("channel_update", { params });
}

export async function channelDelete(params: {
  channelId: string;
  cascade?: boolean;
}): Promise<{ deleted: boolean; deletedThreads?: number }> {
  return invoke("channel_delete", { params });
}

export async function channelMembers(
  channelId: string,
): Promise<{ members: Actor[] }> {
  return invoke("channel_members", { params: { channelId } });
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

export async function threadCreate(params: {
  channelId: string;
  title: string;
  rootMessageId: string;
}): Promise<{ thread: Thread }> {
  return invoke("thread_create", { params });
}

export async function threadArchive(params: {
  threadId: string;
  archived?: boolean;
}): Promise<{ thread: Thread }> {
  return invoke("thread_archive", { params });
}

export async function scopeSubscribe(scope: ScopeRef): Promise<unknown> {
  return invoke("scope_subscribe", { params: { scope } });
}

export async function scopeUnsubscribe(scope: ScopeRef): Promise<unknown> {
  return invoke("scope_unsubscribe", { params: { scope } });
}

export async function messageList(params: {
  target: string;
  limit?: number;
  beforeMessageId?: string;
}): Promise<{ messages: Message[]; pageInfo?: { hasMore?: boolean } }> {
  return invoke("message_list", {
    params: {
      target: params.target,
      limit: params.limit ?? 100,
      ...(params.beforeMessageId
        ? { beforeMessageId: params.beforeMessageId }
        : {}),
    },
  });
}

export async function messageSend(params: {
  target: string;
  body: string;
  parentMessageId?: string;
  threadRootMessageId?: string;
  ifLatestMessageId?: string;
  audience?: AudienceRef[];
  intent?: MessageIntent;
  deliveryPolicy?: DeliveryPolicy;
  metadata?: Record<string, unknown>;
}): Promise<{ message: Message }> {
  return invoke("message_send", {
    params: {
      target: params.target,
      body: params.body,
      ...(params.parentMessageId
        ? { parentMessageId: params.parentMessageId }
        : {}),
      ...(params.threadRootMessageId
        ? { threadRootMessageId: params.threadRootMessageId }
        : {}),
      ...(params.ifLatestMessageId
        ? { ifLatestMessageId: params.ifLatestMessageId }
        : {}),
      audience: params.audience ?? [],
      intent: params.intent ?? "chat",
      deliveryPolicy: params.deliveryPolicy ?? "notify_only",
      metadata: params.metadata ?? {},
    },
  });
}

export async function messageRead(
  messageId: string,
): Promise<{ message: Message }> {
  return invoke("message_read", { params: { messageId } });
}

export async function messageReactionToggle(params: {
  messageId: string;
  emoji: string;
}): Promise<{ message: Message }> {
  return invoke("message_reaction_toggle", { params });
}

export async function runCancel(params: {
  runId: string;
  reason?: string;
}): Promise<{ run: Run; cancelMessage?: Message | null; scope?: ScopeRef | null }> {
  return invoke("run_cancel", { params });
}

export async function inboxList(params: {
  actorId: string;
  state?: DeliveryState;
  limit?: number;
  cursor?: string;
}): Promise<{ deliveries: InboxListEntry[]; nextCursor?: string }> {
  return invoke("inbox_list", { params });
}

export async function deliveryAck(params: {
  actorId: string;
  sourceId: string;
}): Promise<unknown> {
  return invoke("delivery_ack", { params });
}

export async function taskList(params?: {
  channelId?: string;
  sourceMessageId?: string;
  ownerActorId?: string;
  statuses?: Task["status"][];
}): Promise<{ tasks: Task[] }> {
  return invoke("task_list", { params: params ?? {} });
}

export async function taskCreate(params: {
  sourceMessageId: string;
  title?: string;
  description?: string;
  requesterActorId?: string;
  ownerActorId?: string;
  status?: string;
}): Promise<{ task: Task }> {
  return invoke("task_create", { params });
}

export async function taskAssignmentCreate(params: {
  taskId: string;
  fromActorId?: string;
  toActorId: string;
  assignmentType: TaskAssignmentType;
  instruction: string;
  contract?: Record<string, unknown>;
  idempotencyKey?: string;
}): Promise<unknown> {
  const { assignmentType, ...rest } = params;
  return invoke("task_assignment_create", {
    params: { ...rest, type: assignmentType },
  });
}

export async function actorList(): Promise<{ actors: Actor[] }> {
  return invoke("actor_list");
}

export async function machineList(): Promise<MachineListResult> {
  return invoke("machine_list");
}

export async function machineCheck(): Promise<MachineListResult> {
  return invoke("machine_check");
}

export async function localProviderCheck(): Promise<{
  providers: MachineAgentProviderInfo[];
}> {
  return invoke("local_provider_check");
}

export async function machineCreate(args: {
  name: string;
  dataRoot?: string;
}): Promise<MachineListResult> {
  return invoke("machine_create", { args });
}

export async function machineStart(machineId: string): Promise<MachineListResult & {
  pid: number;
}> {
  return invoke("machine_start", { args: { machineId } });
}

export async function machineRemove(machineId: string): Promise<MachineListResult> {
  return invoke("machine_remove", { args: { machineId } });
}

export async function machineAgentCreate(args: {
  machineId: string;
  providerId: string;
  actorId?: string;
  name: string;
  description?: string;
  instructions?: string;
  promptAssembly?: Record<string, unknown>;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
  avatarUrl?: string;
  env?: Record<string, string>;
}): Promise<MachineListResult> {
  return invoke("machine_agent_create", { args });
}

export async function agentUpdate(args: {
  machineId?: string;
  actorId: string;
  displayName?: string;
  description?: string;
  instructions?: string;
  promptAssembly?: Record<string, unknown> | null;
  providerId?: string;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
  avatarUrl?: string;
  env?: Record<string, string>;
}): Promise<AgentInfo> {
  return invoke("agent_update", { args });
}

export async function agentPromptPreview(args: {
  machineId: string;
  actorId: string;
  channelId?: string;
  scope?: Record<string, unknown>;
  sampleMessage?: string;
  promptAssembly?: Record<string, unknown>;
}): Promise<AgentPromptPreviewResult> {
  return invoke("agent_prompt_preview", { args });
}

export async function agentFileList(args: {
  machineId: string;
  actorId: string;
  root: AgentFileRoot;
  prefix?: string;
  channelId?: string;
  scope?: Record<string, unknown>;
}): Promise<AgentFileListResult> {
  return invoke("agent_file_list", { args });
}

export async function agentFileRead(args: {
  machineId: string;
  actorId: string;
  root: AgentFileRoot;
  path: string;
  channelId?: string;
  scope?: Record<string, unknown>;
  maxBytes?: number;
}): Promise<AgentFileReadResult> {
  return invoke("agent_file_read", { args });
}

export async function agentFileWrite(args: {
  machineId: string;
  actorId: string;
  root: AgentFileRoot;
  path: string;
  content: string;
  channelId?: string;
  scope?: Record<string, unknown>;
}): Promise<AgentFileWriteResult> {
  return invoke("agent_file_write", { args });
}

export async function providerAdd(args: {
  machineId: string;
  manifest: Record<string, unknown>;
  replace?: boolean;
}): Promise<unknown> {
  return invoke("provider_add", { args });
}

export async function providerRemove(args: {
  machineId: string;
  providerId: string;
}): Promise<unknown> {
  return invoke("provider_remove", { args });
}

export async function machineAgentRemove(params: {
  machineId: string;
  actorId: string;
}): Promise<MachineListResult> {
  return invoke("machine_agent_remove", { args: params });
}

export async function openLocalPath(path: string): Promise<void> {
  await invoke("open_local_path", { args: { path } });
}

export function onStream(cb: (u: StreamUpdate) => void): Promise<UnlistenFn> {
  return listen<StreamUpdate>("loom://stream", (e) => cb(e.payload));
}

export function onConnection(
  cb: (u: { state: "open" } | { state: "closed"; reason?: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ state: "open" | "closed"; reason?: string }>(
    "loom://connection",
    (e) => cb(e.payload),
  );
}
