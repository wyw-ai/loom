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
  attachments: string[];
  metadata: Record<string, unknown>;
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

export interface StreamUpdate {
  kind: string;
  scope: ScopeRef;
  data: Record<string, unknown>;
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
