import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Actor,
  Channel,
  DesktopConfig,
  JoiEvent,
  ScopeRef,
  StreamUpdate,
  Thread,
  TurnStreamDelta,
} from "./types";

// ---- workspace profile management ----

export async function workspacesList(): Promise<DesktopConfig> {
  return invoke("workspaces_list");
}

export async function workspaceAdd(args: {
  name: string;
  serverUrl: string;
  actorId: string;
  displayName?: string;
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

// ---- connection ----

export async function connect(workspaceId: string): Promise<unknown> {
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
  rootEventId?: string;
}): Promise<{ thread: Thread }> {
  return invoke("thread_create", { params });
}

export async function threadUpdate(params: {
  threadId: string;
  title: string;
}): Promise<{ thread: Thread }> {
  return invoke("thread_update", { params });
}

export async function threadDelete(params: {
  threadId: string;
}): Promise<{ deleted: boolean }> {
  return invoke("thread_delete", { params });
}

export async function threadList(
  channelId?: string,
): Promise<{ threads: Thread[] }> {
  return invoke("thread_list", { params: channelId ? { channelId } : {} });
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

export async function turnClose(
  turnId: string,
  status: "closed" | "cancelled" = "cancelled",
): Promise<unknown> {
  return invoke("turn_close", { params: { turnId, status } });
}

export async function actorList(): Promise<{ actors: Actor[] }> {
  return invoke("actor_list");
}

export async function agentList(): Promise<{
  agents: Array<{ spec: { actor: Actor }; status: string; pid?: number }>;
}> {
  return invoke("agent_list");
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
