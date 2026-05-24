import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Actor,
  AudienceRef,
  Channel,
  DesktopConfig,
  DeliveryPolicy,
  DeliveryState,
  HumanAccount,
  InboxListEntry,
  Message,
  MessageIntent,
  Run,
  ScopeRef,
  StreamUpdate,
  Task,
  Thread,
  Workspace,
} from "./types";

export type LoginProvider = "google" | "github";

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

export async function accountLogin(provider: LoginProvider): Promise<{
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
}): Promise<{ channel: Channel }> {
  return invoke("channel_create", { params });
}

export async function channelMembers(
  channelId: string,
): Promise<{ members: Actor[] }> {
  return invoke("channel_members", { params: { channelId } });
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
}): Promise<{ messages: Message[]; pageInfo: { hasMore: boolean } }> {
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
  ownerActorId?: string;
}): Promise<{ tasks: Task[] }> {
  return invoke("task_list", { params: params ?? {} });
}

export async function actorList(): Promise<{ actors: Actor[] }> {
  return invoke("actor_list");
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
