// Hand-written mirror of the relevant proto types. Keep in sync with
// crates/proto/src/types.rs and methods.rs.
//
// We don't attempt to wrap every field — only the shapes the front-end
// stores or renders.

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
}

export interface Thread {
  id: string;
  channelId: string;
  title: string;
  rootEventId?: string | null;
}

export type ScopeKind = "channel" | "thread";

export interface ScopeRef {
  kind: ScopeKind;
  id: string;
}

// NOTE: RelationKind is `#[serde(rename_all = "snake_case")]` in
// crates/proto/src/types.rs — the wire form is snake_case, not camelCase
// like most other proto enums. This mirror must match exactly or
// event/append calls carrying a relation silently round-trip to
// `Other("handsOffTo")` on the server and get rejected.
export type RelationKind =
  | "replies_to"
  | "hands_off_to"
  | "responds_to"
  | "attaches_artifact";

export type RefKind =
  | "actor"
  | "channel"
  | "thread"
  | "turn"
  | "event"
  | "artifact";

export interface Relation {
  kind: RelationKind;
  target: { kind: RefKind; id: string };
}

export interface JoiEvent {
  id: string;
  type: string;
  actorId: string;
  scope: ScopeRef;
  turnId?: string | null;
  seq: number;
  occurredAt: string;
  payload: unknown;
  relations: Relation[];
  _meta?: Record<string, unknown>;
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

export type TurnStatus = "open" | "closed" | "failed" | "cancelled";

export interface Turn {
  id: string;
  actorId: string;
  scope: ScopeRef;
  status: TurnStatus;
  openedAt: string;
  closedAt?: string | null;
  triggerEventId?: string | null;
}

export type ReminderStatus = "scheduled" | "fired" | "cancelled";

export interface Reminder {
  id: string;
  actorId: string;
  title: string;
  scope?: ScopeRef | null;
  msgId?: string | null;
  fireAt: string;
  repeat?: string | null;
  status: ReminderStatus;
  createdAt: string;
  updatedAt: string;
  lastFiredAt?: string | null;
  _meta?: Record<string, unknown>;
}

// ---- notification payloads the front-end consumes ----

export interface StreamUpdate {
  kind: string;
  scope: ScopeRef;
  data: Record<string, unknown>;
}

export interface TurnStreamDelta {
  turnId: string;
  scope: ScopeRef;
  actorId: string;
  seq: number;
  deltaText: string;
}

// ---- config ----

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

export interface MachineConfig {
  workspaceId?: string | null;
  ownerActorId?: string | null;
  id: string;
  name: string;
  kind: string;
  dataRoot: string;
  agents?: MachineAgentConfig[];
}

export interface MachineAgentConfig {
  providerId: string;
  actorId: string;
  name: string;
  description?: string;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
}

export interface DesktopConfig {
  active?: string | null;
  account?: HumanAccount | null;
  workspaces: Workspace[];
  machines?: MachineConfig[];
}

// ---- local agent / machine management ----

export interface AgentTransport {
  kind: string;
  command: string;
  args?: string[];
  env?: Record<string, string>;
  model?: string | null;
}

export interface AgentSpec {
  actor: Actor;
  transport: AgentTransport;
  autostart?: boolean;
  models?: {
    default?: string | null;
    choices?: Array<{ id: string; label?: string; description?: string }>;
  } | null;
  identity?: {
    description?: string | null;
  } | null;
}

export interface AgentInfo {
  spec: AgentSpec;
  status: string;
  pid?: number;
  sessionId?: string;
}

export interface AgentProviderSummary {
  id: string;
  name: string;
  transportKind: string;
  command: string;
  args?: string[];
  actorCount: number;
  defaultModel?: string | null;
  modelChoices?: Array<{ id: string; label?: string; description?: string }>;
}

export interface MachineInfo {
  id: string;
  name: string;
  kind: string;
  status: string;
  setupStatus: string;
  connectionStatus: string;
  connectionActorId: string;
  dataRoot: string;
  configDir: string;
  agentCount: number;
  onlineAgentCount: number;
  providers: AgentProviderSummary[];
  agents: AgentInfo[];
  serveCommand: string;
  setupScript: string;
}

// ---- bubble (front-end only) ----

export type BubbleKind = "stream" | "static" | "actionRequest" | "system";
export type DeliveryState = "na" | "pending" | "delivered";
export type ActionStatus = "pending" | "answered" | "accepted" | "declined";

export interface ActionChoice {
  id: string;
  label: string;
}

export interface Bubble {
  id: string;
  actorId: string;
  turnId?: string;
  kind: BubbleKind;
  text: string;
  ts: string;
  meta?: Record<string, unknown>;
  replyToEventId?: string;
  streaming: boolean;
  delivery: DeliveryState;
  handoffTarget?: string;
  attachmentIds?: string[];
  // action.request bubbles only
  requestType?: string;
  actionTitle?: string;
  actionReason?: string;
  actionCommand?: string;
  actionRawInput?: string;
  actionRequestId?: string;
  actionStatus?: ActionStatus;
  actionSelectedLabel?: string;
  choices?: ActionChoice[];
  acknowledged?: boolean;
}

// ---- scope key helpers ----

export function scopeKey(scope: ScopeRef): string {
  return `${scope.kind}:${scope.id}`;
}
