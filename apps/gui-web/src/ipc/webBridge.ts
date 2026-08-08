import type { DesktopConfig, HumanAccount, Workspace } from "./types";

type JsonValue = unknown;
type Unlisten = () => void;
type ConnectionEvent = { state: "open" } | { state: "closed"; reason?: string };
type EventHandler<T> = (payload: T) => void;

export interface WebConnectionInput {
  serverUrl: string;
  actorId: string;
  displayName: string;
}

interface StoredWebConfig {
  serverUrl?: string;
  actorId?: string;
  displayName?: string;
  avatarUrl?: string;
  workspaceName?: string;
}

interface PendingRequest {
  resolve: (value: JsonValue) => void;
  reject: (reason: Error) => void;
  timeout: number;
}

interface RpcError {
  code: number;
  message: string;
  data?: unknown;
}

interface RpcResponse {
  id?: string | number | null;
  result?: JsonValue;
  error?: RpcError;
}

interface RpcNotification {
  method?: string;
  params?: JsonValue;
}

const storageKey = "loom.gui.web.connection.v1";
const defaultWorkspaceId = "web";
const requestTimeoutMs = 30_000;
const protocolVersion = "0.1";

const streamListeners = new Set<EventHandler<JsonValue>>();
const connectionListeners = new Set<EventHandler<ConnectionEvent>>();

let socket: WebSocket | null = null;
let currentConfig: Required<Pick<StoredWebConfig, "serverUrl" | "actorId" | "displayName">> | null = null;
let connectPromise: Promise<JsonValue> | null = null;
let reconnectTimer: number | null = null;
let reconnectAttempt = 0;
let wasOpened = false;
let manuallyClosed = false;
let nextRequestId = 1;

const pending = new Map<string, PendingRequest>();

function canUseStorage() {
  return typeof window !== "undefined" && Boolean(window.localStorage);
}

function readStored(): StoredWebConfig {
  if (!canUseStorage()) return {};
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as StoredWebConfig;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

function writeStored(next: StoredWebConfig) {
  if (!canUseStorage()) return;
  window.localStorage.setItem(storageKey, JSON.stringify(next));
}

function normalizeStored(input: WebConnectionInput, workspaceName = "Web") {
  const serverUrl = input.serverUrl.trim();
  const actorId = input.actorId.trim();
  const displayName = input.displayName.trim() || actorId;
  if (!serverUrl) throw new Error("Server URL is required");
  if (!actorId) throw new Error("Actor ID is required");
  return { serverUrl, actorId, displayName, workspaceName };
}

function webAccount(stored = readStored()): HumanAccount | null {
  if (!stored.actorId || !stored.displayName) return null;
  const staffId = stored.actorId.replace(/^actor_human_(web|local)_?/, "") || stored.actorId;
  return {
    provider: "web",
    staffId,
    nickname: stored.displayName,
    realName: "",
    email: "",
    actorId: stored.actorId,
    avatarUrl: stored.avatarUrl ?? "",
  };
}

function webWorkspace(stored = readStored()): Workspace | null {
  if (!stored.serverUrl || !stored.actorId || !stored.displayName) return null;
  return {
    id: defaultWorkspaceId,
    name: stored.workspaceName || "Web",
    serverUrl: stored.serverUrl,
    actorId: stored.actorId,
    displayName: stored.displayName,
  };
}

export function hasWebConnectionConfig() {
  return webWorkspace() !== null;
}

export function webConfig(): DesktopConfig {
  const stored = readStored();
  const workspace = webWorkspace(stored);
  return {
    active: workspace ? workspace.id : null,
    account: webAccount(stored),
    workspaces: workspace ? [workspace] : [],
  };
}

export function configureWebConnection(input: WebConnectionInput): DesktopConfig {
  const previous = readStored();
  const next = normalizeStored(input, previous.workspaceName || "Web");
  writeStored({ ...previous, ...next });
  return webConfig();
}

export function clearWebConnection(): DesktopConfig {
  disconnect();
  if (canUseStorage()) window.localStorage.removeItem(storageKey);
  return webConfig();
}

export function setWebAccount(input: {
  userId: string;
  nickname: string;
  actorId: string;
}): { account: HumanAccount; config: DesktopConfig } {
  const previous = readStored();
  const actorId = input.actorId.trim() || `actor_human_web_${input.userId.trim() || "user"}`;
  const displayName = input.nickname.trim() || input.userId.trim() || "Web User";
  writeStored({ ...previous, actorId, displayName });
  const account = webAccount();
  if (!account) throw new Error("Failed to save web identity");
  return { account, config: webConfig() };
}

export function setWebWorkspace(input: {
  name: string;
  serverUrl: string;
  activate?: boolean;
}): DesktopConfig {
  const previous = readStored();
  const actorId = previous.actorId || "actor_human_web_user";
  const displayName = previous.displayName || "Web User";
  writeStored({
    ...previous,
    serverUrl: input.serverUrl,
    workspaceName: input.name.trim() || "Web",
    actorId,
    displayName,
  });
  return webConfig();
}

export function removeWebWorkspace(id: string): DesktopConfig {
  if (id !== defaultWorkspaceId) return webConfig();
  const previous = readStored();
  writeStored({
    actorId: previous.actorId,
    displayName: previous.displayName,
    avatarUrl: previous.avatarUrl,
  });
  disconnect();
  return webConfig();
}

export function updateWebAvatar(avatarUrl: string): DesktopConfig {
  const previous = readStored();
  writeStored({ ...previous, avatarUrl });
  return webConfig();
}

export function accountLocalDefaults() {
  const stored = readStored();
  return {
    userId: "web_user",
    nickname: stored.displayName || "Web User",
    actorId: stored.actorId || "actor_human_web_user",
  };
}

export function setActiveWebWorkspace(id: string): DesktopConfig {
  if (id !== defaultWorkspaceId || !webWorkspace()) {
    throw new Error(`unknown workspace id: ${id}`);
  }
  return webConfig();
}

function emitConnection(event: ConnectionEvent) {
  for (const listener of [...connectionListeners]) listener(event);
}

function emitStream(payload: JsonValue) {
  for (const listener of [...streamListeners]) listener(payload);
}

export function onStream<T = JsonValue>(cb: EventHandler<T>): Unlisten {
  const handler = cb as EventHandler<JsonValue>;
  streamListeners.add(handler);
  return () => streamListeners.delete(handler);
}

export function onConnection(cb: EventHandler<ConnectionEvent>): Unlisten {
  connectionListeners.add(cb);
  return () => connectionListeners.delete(cb);
}

function idKey(id: string | number | null | undefined) {
  return id === undefined || id === null ? "_" : String(id);
}

function rejectPending(reason: Error) {
  for (const item of pending.values()) {
    window.clearTimeout(item.timeout);
    item.reject(reason);
  }
  pending.clear();
}

function websocketReady() {
  return socket?.readyState === WebSocket.OPEN;
}

function clearReconnectTimer() {
  if (reconnectTimer !== null) {
    window.clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
}

function reconnectDelayMs(attempt: number) {
  const base = Math.min(30_000, 500 * 2 ** Math.min(attempt, 6));
  return base + Math.floor(Math.random() * 250);
}

function scheduleReconnect(reason: string) {
  if (manuallyClosed || !currentConfig || reconnectTimer !== null) return;
  const delay = reconnectDelayMs(reconnectAttempt++);
  reconnectTimer = window.setTimeout(() => {
    reconnectTimer = null;
    void openSocket(currentConfig!).catch((err) => {
      emitConnection({ state: "closed", reason: err.message || reason });
      scheduleReconnect(err.message || reason);
    });
  }, delay);
}

function sendRaw(method: string, params?: JsonValue): Promise<JsonValue> {
  if (!websocketReady() || !socket) {
    return Promise.reject(new Error("WebSocket is not connected"));
  }
  const id = nextRequestId++;
  const key = idKey(id);
  const frame = JSON.stringify({
    jsonrpc: "2.0",
    id,
    method,
    ...(params === undefined ? {} : { params }),
  });
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      pending.delete(key);
      reject(new Error(`rpc \`${method}\` timed out`));
    }, requestTimeoutMs);
    pending.set(key, { resolve, reject, timeout });
    try {
      socket!.send(frame);
    } catch (err) {
      window.clearTimeout(timeout);
      pending.delete(key);
      reject(err instanceof Error ? err : new Error(String(err)));
    }
  });
}

function handleResponse(response: RpcResponse) {
  const key = idKey(response.id);
  const item = pending.get(key);
  if (!item) return;
  pending.delete(key);
  window.clearTimeout(item.timeout);
  if (response.error) {
    item.reject(
      new Error(
        `rpc failed: ${response.error.message} (code ${response.error.code})`,
      ),
    );
    return;
  }
  item.resolve(response.result ?? null);
}

function handleNotification(notification: RpcNotification) {
  if (notification.method === "stream/update") {
    emitStream(notification.params ?? null);
  }
}

function handleMessage(message: MessageEvent<string>) {
  let envelope: unknown;
  try {
    envelope = JSON.parse(message.data);
  } catch {
    return;
  }
  if (!envelope || typeof envelope !== "object") return;
  const record = envelope as Record<string, unknown>;
  if ("id" in record) {
    handleResponse(record as RpcResponse);
    return;
  }
  handleNotification(record as RpcNotification);
}

function closeExistingSocket() {
  if (!socket) return;
  socket.onopen = null;
  socket.onmessage = null;
  socket.onerror = null;
  socket.onclose = null;
  try {
    socket.close();
  } catch {
    /* ignore close failures */
  }
  socket = null;
}

async function openSocket(config: Required<Pick<StoredWebConfig, "serverUrl" | "actorId" | "displayName">>) {
  clearReconnectTimer();
  closeExistingSocket();
  rejectPending(new Error("WebSocket reconnecting"));
  manuallyClosed = false;
  currentConfig = config;

  const ws = new WebSocket(config.serverUrl);
  socket = ws;

  await new Promise<void>((resolve, reject) => {
    ws.onopen = () => resolve();
    ws.onerror = () => reject(new Error(`WebSocket connect failed: ${config.serverUrl}`));
  });

  ws.onmessage = handleMessage;
  ws.onclose = (event) => {
    if (socket === ws) socket = null;
    rejectPending(new Error("WebSocket closed"));
    if (manuallyClosed) return;
    const reason = event.reason || "connection lost";
    emitConnection({ state: "closed", reason });
    scheduleReconnect(reason);
  };
  ws.onerror = null;

  await sendRaw("initialize", {
    protocolVersion,
    clientInfo: {
      name: "loom-gui-web",
      title: "Loom Web",
      version: "0.1.1",
    },
  });
  const open = await sendRaw("connection/open", {
    actorId: config.actorId,
    actorKind: "human",
    displayName: config.displayName,
    claimInbox: false,
  });
  reconnectAttempt = 0;
  wasOpened = true;
  emitConnection({ state: "open" });
  return open;
}

export async function connect(workspaceId: string): Promise<{
  workspace: Workspace;
  open: JsonValue;
}> {
  const workspace = webWorkspace();
  if (!workspace || workspace.id !== workspaceId) {
    throw new Error(`unknown workspace id: ${workspaceId}`);
  }
  const config = {
    serverUrl: workspace.serverUrl,
    actorId: workspace.actorId,
    displayName: workspace.displayName,
  };
  if (connectPromise) {
    const open = await connectPromise;
    return { workspace, open };
  }
  const sameConnection =
    currentConfig?.serverUrl === config.serverUrl &&
    currentConfig.actorId === config.actorId &&
    currentConfig.displayName === config.displayName;
  if (websocketReady() && wasOpened && sameConnection) {
    return { workspace, open: null };
  }
  connectPromise = openSocket(config)
    .catch((err) => {
      if (err instanceof Error) scheduleReconnect(err.message);
      throw err;
    })
    .finally(() => {
      connectPromise = null;
    });
  const open = await connectPromise;
  return { workspace, open };
}

export function disconnect() {
  manuallyClosed = true;
  clearReconnectTimer();
  closeExistingSocket();
  rejectPending(new Error("WebSocket disconnected"));
  if (wasOpened) emitConnection({ state: "closed", reason: "disconnected" });
  wasOpened = false;
}

export async function callRpc<T = JsonValue>(
  method: string,
  params?: JsonValue,
): Promise<T> {
  if (!websocketReady()) {
    const workspace = webWorkspace();
    if (!workspace) throw new Error("Web connection is not configured");
    await connect(workspace.id);
  }
  return sendRaw(method, params) as Promise<T>;
}
