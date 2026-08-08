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
  Artifact,
  ArtifactReadResult,
  Channel,
  ChannelMemberConfig,
  DesktopConfig,
  DeliveryPolicy,
  DeliveryState,
  HumanAccount,
  InboxListEntry,
  MachineAgentProviderInfo,
  AgentBundleSkillSpec,
  MachineDirListResult,
  MachineListResult,
  Message,
  MessageContextParams,
  MessageContextResult,
  MessageIntent,
  MessageSearchParams,
  MessageSearchResult,
  Run,
  RunGetResult,
  RunListParams,
  RunListResult,
  ScopeRef,
  SkillEntry,
  StreamUpdate,
  Task,
  TaskAssignmentType,
  Thread,
  WakeSpec,
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

export async function accountUpdateAvatar(avatarUrl: string): Promise<DesktopConfig> {
  return invoke("account_update_avatar", { args: { avatarUrl } });
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

export async function channelMemberConfigList(
  channelId: string,
): Promise<{ configs: ChannelMemberConfig[] }> {
  return invoke("channel_member_config_list", { params: { channelId } });
}

export async function channelMemberConfigSet(params: {
  channelId: string;
  actorId: string;
  workspaceDir: string;
}): Promise<{ config: ChannelMemberConfig }> {
  return invoke("channel_member_config_set", { params });
}

export async function channelMemberConfigClear(params: {
  channelId: string;
  actorId: string;
}): Promise<{ cleared: boolean }> {
  return invoke("channel_member_config_clear", { params });
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
  // Wire shape is `page_info` (MessageListResult has no camelCase rename);
  // remap to the frontend's camelCase convention.
  const res = await invoke<{ messages: Message[]; page_info?: { hasMore?: boolean } }>(
    "message_list",
    {
      params: {
        target: params.target,
        limit: params.limit ?? 100,
        ...(params.beforeMessageId
          ? { beforeMessageId: params.beforeMessageId }
          : {}),
      },
    },
  );
  return { messages: res.messages, pageInfo: res.page_info };
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
  attachments?: string[];
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
      attachments: params.attachments ?? [],
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

export async function messageSearch(
  params: MessageSearchParams,
): Promise<MessageSearchResult> {
  return invoke("message_search", { params });
}

export async function messageContext(
  params: MessageContextParams,
): Promise<MessageContextResult> {
  return invoke("message_context", { params });
}

export async function runCancel(params: {
  runId: string;
  reason?: string;
}): Promise<{ run: Run; cancelMessage?: Message | null }> {
  return invoke("run_cancel", { params });
}

export async function runList(params: RunListParams): Promise<RunListResult> {
  return invoke("run_list", { params });
}

export async function runGet(params: { runId: string }): Promise<RunGetResult> {
  return invoke("run_get", { params });
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

export async function inboxStatus(params: {
  actorIds: string[];
  includeEntries?: boolean;
  entryLimit?: number;
}): Promise<import("./types").InboxStatusResult> {
  return invoke("inbox_status", { params });
}

export async function deliveryCancel(params: {
  actorId: string;
  sourceIds?: string[];
  allPending?: boolean;
}): Promise<{ cancelled: import("./types").Delivery[] }> {
  return invoke("delivery_cancel", { params });
}

export async function deliveryExpedite(params: {
  actorId: string;
  sourceId: string;
}): Promise<{ delivery: import("./types").Delivery }> {
  return invoke("delivery_expedite", { params });
}

export async function connectionList(params?: {
  actorIds?: string[];
}): Promise<{ actorIds: string[] }> {
  return invoke("connection_list", { params: params ?? {} });
}

export async function artifactGet(params: {
  artifactId?: string;
  artifactUri?: string;
}): Promise<{ artifact: Artifact }> {
  return invoke("artifact_get", { params });
}

export interface ScopeAttachment {
  artifact: Artifact;
  messageId: string;
}

/**
 * Run async tasks in bounded batches to limit concurrency.
 *
 * Preserves input order in the output array. Items that reject produce
 * `undefined` entries (callers decide how to handle). This keeps memory
 * and simultaneous IPC pressure bounded when resolving many artifacts.
 *
 * @param items Source items.
 * @param batchSize Max number of in-flight tasks per batch.
 * @param task Async function applied to each item.
 */
export async function mapBatched<T, R>(
  items: readonly T[],
  batchSize: number,
  task: (item: T, index: number) => Promise<R>,
): Promise<(R | undefined)[]> {
  const results: (R | undefined)[] = new Array(items.length).fill(undefined);
  for (let start = 0; start < items.length; start += batchSize) {
    const end = Math.min(start + batchSize, items.length);
    const batch = await Promise.all(
      items.slice(start, end).map((item, i) =>
        task(item, start + i).catch(() => undefined),
      ),
    );
    for (let i = 0; i < batch.length; i++) {
      results[start + i] = batch[i];
    }
  }
  return results;
}

/**
 * List all attachments in a channel or thread scope by:
 * 1. Fetching messages via messageList
 * 2. Extracting attachment URIs from each message
 * 3. Resolving artifact metadata via artifactGet
 * Returns a flat list of artifacts (deduplicated by artifact id).
 *
 * Artifact metadata resolution runs in bounded batches (default 8) to
 * cap concurrent IPC pressure. Functionally equivalent to unbounded
 * Promise.all: all attachments are still fetched, only concurrency is
 * limited.
 */
export async function listScopeAttachments(params: {
  target: string;
  limit?: number;
  /** Max concurrent artifactGet calls per batch. Defaults to 8. */
  resolveBatchSize?: number;
}): Promise<ScopeAttachment[]> {
  const { messages } = await messageList({
    target: params.target,
    limit: params.limit ?? 500,
  });

  // Collect unique attachment URIs with their source message
  const seen = new Map<string, string>(); // artifactUri -> messageId
  for (const msg of messages) {
    if (!msg.attachments || msg.attachments.length === 0) continue;
    for (const att of msg.attachments) {
      const clean = att.trim();
      if (!clean) continue;
      // Skip non-artifact attachments (e.g. .skill files, URLs)
      if (!clean.startsWith("artifact://") && !/^art_[A-Za-z0-9_-]+$/.test(clean)) continue;
      if (!seen.has(clean)) {
        seen.set(clean, msg.id);
      }
    }
  }

  // Resolve artifact metadata in bounded batches to limit concurrency.
  const entries = Array.from(seen.entries());
  const resolved = await mapBatched(entries, params.resolveBatchSize ?? 8, async ([uri, messageId]) => {
    const lookupParams = uri.startsWith("artifact://")
      ? { artifactUri: uri }
      : { artifactId: uri };
    const { artifact } = await artifactGet(lookupParams);
    return { artifact, messageId };
  });

  // Filter out failed resolutions (undefined entries from mapBatched).
  const results: ScopeAttachment[] = [];
  for (const item of resolved) {
    if (item) results.push(item);
  }

  return results;
}

export async function artifactRead(params: {
  artifactId: string;
  offset?: number;
  maxBytes?: number;
}): Promise<ArtifactReadResult> {
  return invoke("artifact_read", {
    params: {
      artifactId: params.artifactId,
      offset: params.offset ?? 0,
      maxBytes: params.maxBytes ?? 65536,
    },
  });
}

export async function artifactPublish(params: {
  ingress:
    | { kind: "inlineText"; name: string; mediaType?: string; text: string }
    | { kind: "fileBytes"; name: string; mediaType?: string; bytes: number[] };
  createdBy: string;
  scope?: ScopeRef;
}): Promise<{ artifact: Artifact }> {
  const kind = params.ingress.kind === "inlineText" ? "inline_text" : "file_bytes";
  const payload: Record<string, unknown> = {
    kind,
    name: params.ingress.name,
    mediaType: params.ingress.mediaType ?? (params.ingress.kind === "inlineText" ? "text/markdown" : "application/octet-stream"),
  };
  if (params.ingress.kind === "inlineText") {
    payload.text = params.ingress.text;
  } else {
    payload.bytes = params.ingress.bytes;
  }
  return invoke("artifact_publish", {
    params: {
      ingress: payload,
      createdBy: params.createdBy,
      ...(params.scope ? { scope: params.scope } : {}),
    },
  });
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

export async function machineDirList(args: {
  machineId: string;
  path?: string;
  includeFiles?: boolean;
}): Promise<MachineDirListResult> {
  return invoke("machine_dir_list", {
    args: {
      machineId: args.machineId,
      ...(args.path ? { path: args.path } : {}),
      ...(args.includeFiles ? { includeFiles: true } : {}),
    },
  });
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
  wake?: WakeSpec | null;
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
  wake?: WakeSpec | null;
  providerId?: string;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
  avatarUrl?: string;
  env?: Record<string, string>;
  bundleSkills?: AgentBundleSkillSpec[] | null;
}): Promise<AgentInfo> {
  return invoke("agent_update", { args });
}

export async function agentSkillAdd(args: {
  machineId: string;
  actorId: string;
  source: string;
}): Promise<AgentInfo> {
  return invoke("agent_skill_add", { args });
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

export async function saveFileDialog(fileName: string, bytes: Uint8Array): Promise<string | null> {
  return invoke<string | null>("save_file_dialog", {
    args: { fileName, bytes: Array.from(bytes) },
  });
}

export async function openFileDefault(path: string): Promise<void> {
  await invoke("open_file_default", { args: { path } });
}

export async function revealInFolder(path: string): Promise<void> {
  await invoke("reveal_in_folder", { args: { path } });
}

export async function pathExists(path: string): Promise<boolean> {
  return invoke<boolean>("path_exists", { args: { path } });
}

export async function artifactExists(args: { artifactId: string }): Promise<boolean> {
  return invoke<boolean>("artifact_exists", { args: { artifactId: args.artifactId } });
}

export async function downloadToTemp(args: {
  artifactId: string;
  suggestedName?: string;
}): Promise<string> {
  return invoke<string>("download_to_temp", {
    args: {
      artifactId: args.artifactId,
      ...(args.suggestedName ? { suggestedName: args.suggestedName } : {}),
    },
  });
}

/**
 * Download an artifact to the persistent cache directory
 * (data_dir/loom/cache/attachments/<artifactId>/).
 *
 * ARCH D3-r1: replaces downloadToTemp for image auto-download.
 * Returns the local file path. Persists across restarts.
 */
export async function downloadToCache(args: {
  artifactId: string;
  suggestedName?: string;
}): Promise<string> {
  return invoke<string>("download_to_cache", {
    args: {
      artifactId: args.artifactId,
      ...(args.suggestedName ? { suggestedName: args.suggestedName } : {}),
    },
  });
}

/**
 * Clear the entire attachment cache directory (disk files).
 * ARCH D3-r1: used by CacheManagementSection in settings.
 * Returns `{ freedBytes, clearedIds }` (AC-A6: symmetric with
 * clearAttachmentCacheByType) so the FE can batch-clear the
 * corresponding localStorage entries by artifact id.
 */
export async function clearAttachmentCache(): Promise<{
  freedBytes: number;
  clearedIds: string[];
}> {
  return invoke("clear_attachment_cache", { args: {} });
}

/**
 * Get cache breakdown by media type category.
 * ARCH D3 design: returns { images: {size,count}, other: {size,count}, total }.
 * ARCH TODO#2 Tier 2: also returns `cachedIds` - the list of artifactIds
 * present on disk, used to reconcile localStorage orphan mappings.
 */
export async function getAttachmentCacheBreakdown(): Promise<{
  images: { size: number; count: number };
  other: { size: number; count: number };
  total: { size: number; count: number };
  cachedIds: string[];
}> {
  return invoke("get_attachment_cache_breakdown", { args: {} });
}

/**
 * Clear attachment cache by category ("images" or "other").
 * ARCH D3 design: returns { freedBytes, clearedIds }.
 */
export async function clearAttachmentCacheByType(category: "images" | "other"): Promise<{
  freedBytes: number;
  clearedIds: string[];
}> {
  return invoke("clear_attachment_cache_by_type", { args: { category } });
}

/**
 * Open the attachment cache root directory in the system file manager.
 * ARCH design: opens data_dir/loom/cache/attachments/ (Win/Mac/Linux compatible).
 */
export async function openAttachmentCacheDirectory(): Promise<void> {
  await invoke("open_attachment_cache_directory", { args: {} });
}

/**
 * Read bytes from a local cached file (cache-hit path).
 * ARCH D3-r1: skips network download when file is already cached.
 */
export async function readLocalFileBytes(args: {
  path: string;
  offset?: number;
  maxBytes?: number;
}): Promise<{ bytes: number[]; truncated: boolean; nextOffset?: number }> {
  return invoke("read_local_file_bytes", {
    args: {
      path: args.path,
      ...(args.offset !== undefined ? { offset: args.offset } : {}),
      ...(args.maxBytes !== undefined ? { maxBytes: args.maxBytes } : {}),
    },
  });
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

// ---- channel/thread instructions ----

export async function channelGetInstruction(
  channelId: string,
): Promise<{ instructions: string | null }> {
  return invoke("channel_get_instruction", {
    params: { channelId },
  });
}

export async function channelSetInstruction(params: {
  channelId: string;
  instructions: string;
}): Promise<{ channel: Channel }> {
  return invoke("channel_set_instruction", { params });
}

export async function channelClearInstruction(
  channelId: string,
): Promise<{ cleared: boolean }> {
  return invoke("channel_clear_instruction", {
    params: { channelId },
  });
}

export async function threadGetInstruction(
  threadId: string,
): Promise<{ instructions: string | null }> {
  return invoke("thread_get_instruction", {
    params: { threadId },
  });
}

export async function threadSetInstruction(params: {
  threadId: string;
  instructions: string;
}): Promise<{ thread: Thread }> {
  return invoke("thread_set_instruction", { params });
}

export async function threadClearInstruction(
  threadId: string,
): Promise<{ cleared: boolean }> {
  return invoke("thread_clear_instruction", {
    params: { threadId },
  });
}

// ---- channel/thread skills (local file I/O) ----

export async function channelSkillList(
  channelId: string,
): Promise<{ skills: SkillEntry[] }> {
  return invoke("channel_skill_list", { args: { channelId } });
}

export async function channelSkillAdd(params: {
  channelId: string;
  source: string;
  skillId?: string;
}): Promise<{ skills: SkillEntry[] }> {
  return invoke("channel_skill_add", {
    args: {
      channelId: params.channelId,
      source: params.source,
      ...(params.skillId ? { skillId: params.skillId } : {}),
    },
  });
}

export async function channelSkillRemove(params: {
  channelId: string;
  skillId: string;
}): Promise<{ skills: SkillEntry[] }> {
  return invoke("channel_skill_remove", {
    args: { channelId: params.channelId, skillId: params.skillId },
  });
}

export async function threadSkillList(params: {
  channelId: string;
  threadId: string;
}): Promise<{ skills: SkillEntry[] }> {
  return invoke("thread_skill_list", { args: params });
}

export async function threadSkillAdd(params: {
  channelId: string;
  threadId: string;
  source: string;
  skillId?: string;
}): Promise<{ skills: SkillEntry[] }> {
  return invoke("thread_skill_add", {
    args: {
      channelId: params.channelId,
      threadId: params.threadId,
      source: params.source,
      ...(params.skillId ? { skillId: params.skillId } : {}),
    },
  });
}

export async function threadSkillRemove(params: {
  channelId: string;
  threadId: string;
  skillId: string;
}): Promise<{ skills: SkillEntry[] }> {
  return invoke("thread_skill_remove", {
    args: {
      channelId: params.channelId,
      threadId: params.threadId,
      skillId: params.skillId,
    },
  });
}
