export type ActorKind = "human" | "agent" | "service";

export interface Actor {
  id: string;
  kind: ActorKind;
  displayName?: string;
  capabilities?: unknown;
  _meta?: Record<string, unknown>;
}

export type ChannelVisibility = "public" | "private";

export interface Channel {
  id: string;
  title: string;
  topic?: string;
  visibility: ChannelVisibility;
  members: string[];
  _meta?: Record<string, unknown>;
}

export type ScopeKind = "channel" | "thread";

export interface ScopeRef {
  kind: ScopeKind;
  id: string;
}

export interface Thread {
  id: string;
  channelId: string;
  title: string;
  rootMessageId: string;
  archivedAt?: string | null;
  _meta?: Record<string, unknown>;
}

export type MessageKind =
  | "human"
  | "agent"
  | "system"
  | "attention"
  | "task_update"
  | "artifact";

export type MessageIntent =
  | "chat"
  | "ask"
  | "request_action"
  | "assign_task"
  | "status_update"
  | "review"
  | "notify";

export type DeliveryPolicy =
  | "notify_only"
  | "wake_agent"
  | "route_by_intent"
  | "silent";

export type AudienceKind = "actor" | "group" | "all" | "agents" | "humans";

export interface AudienceRef {
  kind: AudienceKind;
  id: string;
  display?: string;
}

export interface MessageMention {
  actorOrGroupId: string;
  kind: "actor" | "group" | "all" | "agents" | "humans";
  source: string;
  byteStart: number;
  byteEnd: number;
  display: string;
}

export interface MessageReaction {
  emoji: string;
  actorIds: string[];
}

export interface Message {
  id: string;
  scope: ScopeRef;
  target: string;
  authorActorId: string;
  createdAt: string;
  kind: MessageKind;
  body: string;
  mentions: MessageMention[];
  audience: AudienceRef[];
  intent: MessageIntent;
  deliveryPolicy: DeliveryPolicy;
  parentMessageId?: string | null;
  threadRootMessageId?: string | null;
  taskId?: string | null;
  attachments?: string[];
  reactions: MessageReaction[];
  metadata: Record<string, unknown>;
}

export type ArtifactKind = "file" | "directory";

export interface Artifact {
  id: string;
  uri: string;
  kind: ArtifactKind;
  name: string;
  mediaType: string;
  size: number;
  checksum: string;
  createdBy: string;
  createdAt: string;
  _meta?: Record<string, unknown>;
}

export interface ArtifactReadResult {
  artifactId: string;
  mediaType: string;
  offset: number;
  truncated: boolean;
  nextOffset?: number;
  content: string;
  bytes?: number[];
}

export type RunStatus =
  | "queued"
  | "preparing_context"
  | "running"
  | "waiting_tool"
  | "completed"
  | "failed"
  | "canceled";

export interface Run {
  id: string;
  actorId: string;
  scope: ScopeRef;
  deliveryId?: string | null;
  startReason?: string | null;
  agentConfigVersionId: string;
  status: RunStatus;
  openedAt: string;
  closedAt?: string | null;
  metadata: Record<string, unknown>;
}

export interface RunFrame {
  runId: string;
  seq: number;
  kind: string;
  payload: unknown;
  createdAt: string;
}

export type DeliveryState = "pending" | "delivered" | "failed";

export interface Delivery {
  sourceId: string;
  actorId: string;
  state: DeliveryState;
  updatedAt: string;
  _meta?: Record<string, unknown>;
}

export interface InboxListEntry {
  delivery: Delivery;
  message?: Message | null;
}

export type TaskStatus =
  | "todo"
  | "claimed"
  | "in_progress"
  | "waiting_review"
  | "done"
  | "failed"
  | "canceled";

export interface Task {
  id: string;
  number: number;
  channelId: string;
  sourceMessageId: string;
  canonicalThreadId: string;
  parentSourceMessageId?: string | null;
  parentTaskId?: string | null;
  title: string;
  description: string;
  requesterActorId: string;
  ownerActorId?: string | null;
  status: TaskStatus;
  resultSummary: string;
  artifactIds: string[];
  assignmentIds: string[];
  practiceContractEpoch?: string | null;
  createdAt: string;
  updatedAt: string;
  _meta?: Record<string, unknown>;
}

export interface Workspace {
  id: string;
  name: string;
  serverUrl: string;
  actorId: string;
  displayName: string;
}

export interface HumanAccount {
  provider: string;
  staffId: string;
  nickname: string;
  realName: string;
  email: string;
  actorId: string;
  avatarUrl: string;
}

export interface DesktopConfig {
  active?: string | null;
  account?: HumanAccount | null;
  workspaces: Workspace[];
}

export interface AgentModelChoice {
  id: string;
  label: string;
  description?: string | null;
}

export interface AgentModelSpec {
  default?: string | null;
  choices: AgentModelChoice[];
}

export interface AgentProviderRef {
  id: string;
  mode?: string | null;
  model?: string | null;
  reasoningEffort?: string | null;
  env?: Record<string, string>;
}

export interface MachineAgentProviderInfo {
  id: string;
  name: string;
  transportKind: string;
  command: string;
  args: string[];
  actorCount: number;
  defaultModel?: string | null;
  modelChoices: AgentModelChoice[];
}

export interface AgentSpec {
  actor: Actor;
  instructions?: string | null;
  providerRef: AgentProviderRef;
  models?: AgentModelSpec | null;
  autostart?: boolean | null;
  promptAssembly?: Record<string, unknown> | null;
  _meta?: Record<string, unknown>;
}

export interface AgentInfo {
  spec: AgentSpec;
  status: string;
  pid?: number | null;
  sessionId?: string | null;
}

export interface MachineAgentInfo extends AgentInfo {
  profilePath: string;
}

export type AgentFileRoot = "profile" | "scopeWorkspace" | "scope-workspace";

export interface AgentFileEntry {
  path: string;
  bytes: number;
  modified?: string | null;
}

export interface AgentFileListResult {
  root: string;
  prefix: string;
  files: AgentFileEntry[];
}

export interface AgentFileReadResult {
  root: string;
  path: string;
  content: string;
}

export interface AgentFileWriteResult {
  root: string;
  path: string;
  bytes: number;
}

export interface AgentPromptPreviewPart {
  key: string;
  title: string;
  source: string;
  bytes: number;
  empty: boolean;
  missing: boolean;
  content: string;
}

export interface AgentPromptPreviewResult {
  actorId: string;
  scope: Record<string, unknown>;
  parts: AgentPromptPreviewPart[];
  outputs: {
    system: string;
    user: string;
    full: string;
  };
  bindings: Record<string, unknown>;
  warnings: string[];
}

export interface MachineInfo {
  workspaceId?: string | null;
  ownerActorId?: string | null;
  id: string;
  name: string;
  kind: string;
  source: string;
  readOnly: boolean;
  canCommand: boolean;
  canOpenLocalPath: boolean;
  capabilities: string[];
  inventoryRevision: number;
  inventoryObservedAt?: string | null;
  status: string;
  setupStatus: string;
  connectionStatus: string;
  connectionActorId: string;
  dataRoot: string;
  configDir: string;
  agentCount: number;
  onlineAgentCount: number;
  serviceCount: number;
  providers: MachineAgentProviderInfo[];
  agents: MachineAgentInfo[];
  services: MachineServiceInfo[];
  serveCommand: string;
  setupScript: string;
}

export interface MachineServiceInfo {
  id: string;
  kind: string;
  displayName?: string;
  actor: Actor;
  lifecycle?: string;
  autostart?: boolean;
  [key: string]: unknown;
}

export interface MachineListResult {
  machines: MachineInfo[];
}

export type TaskAssignmentType =
  | "generate"
  | "review"
  | "investigate"
  | "fix"
  | "verify"
  | "other";

export interface StreamUpdate {
  kind: string;
  scope: ScopeRef;
  data: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Token usage (Iteration #4 — agent self-monitoring)
//
// Mirrors `crates/agent-runtime/src/adapter.rs::TokenUsage` (rename_all =
// snake_case). The agent worker (`crates/cli/src/cmd/agent_serve.rs::
// build_turn_meta`) attaches `TokenUsageMeta { increment, cumulative }` onto
// every assistant message under `metadata.token_usage`, plus a prompt
// breakdown under `metadata.prompt_breakdown`.
// ---------------------------------------------------------------------------

export interface TokenUsage {
  input_tokens?: number | null;
  output_tokens?: number | null;
  total_tokens?: number | null;
  cache_creation_input_tokens?: number | null;
  cache_read_input_tokens?: number | null;
  reasoning_tokens?: number | null;
  total_cost_usd?: number | null;
  estimated?: boolean;
}

export interface MessageTokenUsageMeta {
  increment: TokenUsage;
  cumulative: TokenUsage;
}

export interface PromptBreakdownSection {
  key: string;
  label: string;
  char_count?: number;
  byte_count?: number;
  approx_token_count: number;
  percentage?: number;
}

export interface PromptBreakdown {
  sections: PromptBreakdownSection[];
}

export function readMessageTokenUsage(
  meta: Record<string, unknown> | undefined | null,
): MessageTokenUsageMeta | null {
  if (!meta || typeof meta !== "object") return null;
  const raw = (meta as Record<string, unknown>)["token_usage"];
  if (!raw || typeof raw !== "object") return null;
  const obj = raw as Record<string, unknown>;
  const increment = obj["increment"];
  const cumulative = obj["cumulative"];
  if (!increment || typeof increment !== "object") return null;
  if (!cumulative || typeof cumulative !== "object") return null;
  return {
    increment: increment as TokenUsage,
    cumulative: cumulative as TokenUsage,
  };
}

export function readMessagePromptBreakdown(
  meta: Record<string, unknown> | undefined | null,
): PromptBreakdown | null {
  if (!meta || typeof meta !== "object") return null;
  const raw = (meta as Record<string, unknown>)["prompt_breakdown"];
  if (!raw || typeof raw !== "object") return null;
  const sections = (raw as Record<string, unknown>)["sections"];
  if (!Array.isArray(sections)) return null;
  return { sections: sections as PromptBreakdownSection[] };
}

export function scopeKey(scope: ScopeRef): string {
  return `${scope.kind}:${scope.id}`;
}

export function channelTarget(channelId: string): string {
  return `#${channelId}`;
}

export function threadTarget(thread: Thread): string {
  return `#${thread.channelId}:${thread.rootMessageId}`;
}
