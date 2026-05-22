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
  archivedAt?: string | null;
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
  | "attaches_artifact"
  | "relates_to_task";

export type RefKind =
  | "actor"
  | "channel"
  | "thread"
  | "turn"
  | "event"
  | "artifact"
  | "task";

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

export type TaskStatus =
  | "todo"
  | "claimed"
  | "in_progress"
  | "waiting_review"
  | "done"
  | "failed"
  | "canceled";

export type TaskAssignmentType =
  | "generate"
  | "review"
  | "investigate"
  | "fix"
  | "verify"
  | "other";

export type TaskAssignmentStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "canceled";

export interface Task {
  id: string;
  number: number;
  channelId: string;
  sourceEventId: string;
  canonicalThreadId: string;
  parentSourceEventId?: string | null;
  parentTaskId?: string | null;
  title: string;
  description: string;
  requesterActorId: string;
  ownerActorId?: string | null;
  status: TaskStatus;
  resultSummary: string;
  artifactIds?: string[];
  assignmentIds?: string[];
  practiceContractEpoch?: string | null;
  createdAt: string;
  updatedAt: string;
  _meta?: Record<string, unknown>;
}

export interface TaskAssignment {
  id: string;
  taskId: string;
  fromActorId: string;
  toActorId: string;
  type: TaskAssignmentType;
  instruction: string;
  status: TaskAssignmentStatus;
  resultEventId?: string | null;
  resultSummary: string;
  contract?: unknown;
  idempotencyKey?: string | null;
  leaseId?: string | null;
  resultArtifactIds?: string[];
  resultFactIds?: string[];
  evidenceRefs?: string[];
  resultEnvelope?: unknown;
  createdAt: string;
  updatedAt: string;
  _meta?: Record<string, unknown>;
}

export type TaskRefConfidence = "confirmed" | "inferred";
export type TaskRefStatus = "active" | "superseded" | "retired";

export interface TaskRef {
  id: string;
  taskId: string;
  channelId: string;
  kind: string;
  subtype: string;
  value: string;
  normalized: string;
  fields?: unknown;
  confidence: TaskRefConfidence;
  status: TaskRefStatus;
  supersededBy?: string | null;
  sourceEventId?: string | null;
  createdByActorId: string;
  createdAt: string;
  updatedAt: string;
}

export type TaskArtifactLinkStatus = "active" | "proposal" | "superseded" | "rejected";

export interface TaskArtifactLink {
  id: string;
  taskId: string;
  artifactId: string;
  schema: string;
  role: string;
  sequence: number;
  status: TaskArtifactLinkStatus;
  lineage?: unknown;
  binding?: unknown;
  createdByActorId: string;
  createdAt: string;
  updatedAt: string;
}

export type TaskFactStatus = "active" | "superseded" | "retracted" | "conflict";
export type TaskFactType = "observation" | "status" | "decision" | "action" | "user_defined";

export interface TaskFact {
  id: string;
  taskId: string;
  targetKey: string;
  kind: string;
  factType: TaskFactType;
  signature: string;
  status: TaskFactStatus;
  authority?: string;
  authorityBinding?: unknown;
  observedFields?: string[];
  unobservedFields?: string[];
  unavailableReason?: string | null;
  snapshotCompleteness?: "complete" | "partial" | "unknown" | null;
  producerId: string;
  summary: string;
  payloadSchema?: string;
  payload?: unknown;
  createdAt: string;
  updatedAt: string;
}

export type TaskProjectionHealth =
  | "fresh"
  | "stale"
  | "missing"
  | "invalid"
  | "repair_required";

export interface TaskProjection {
  id: string;
  taskId: string;
  projectionType: string;
  producerActorId: string;
  health: TaskProjectionHealth;
  watermark?: unknown;
  payloadSchema?: string;
  payload?: unknown;
  updatedAt: string;
}

export type TaskChangeDeliveryStatus = "pending" | "processing" | "handled" | "failed";
export type TaskChangeAckDisposition =
  | "assignment_created"
  | "assignment_reused"
  | "action_requested"
  | "fact_written"
  | "artifact_written"
  | "projection_repaired"
  | "blocked"
  | "noop_recorded"
  | "escalated";

export interface TaskChangeDelivery {
  change: {
    id: string;
    cursor: number;
    taskId: string;
    changeType: string;
    sourceIds?: string[];
    signature: string;
    summary: string;
    occurredAt: string;
    recipients?: string[];
    requiresAck: boolean;
  };
  recipientActorId: string;
  status: TaskChangeDeliveryStatus;
  disposition?: TaskChangeAckDisposition | null;
  resultRefIds?: string[];
  reason?: string;
  ackedAt?: string | null;
}

export type ActorDeliveryState = "pending" | "delivered" | "failed";

export interface Delivery {
  eventId: string;
  actorId: string;
  state: ActorDeliveryState;
  updatedAt: string;
  _meta?: Record<string, unknown>;
}

export interface DeliveryListEntry {
  delivery: Delivery;
  event?: JoiEvent | null;
}

export interface WorkspaceLease {
  id: string;
  resourceKey: string;
  holderAssignmentId: string;
  holderActorId: string;
  mode: "read" | "write";
  status: "active" | "released" | "expired" | "canceled";
  expiresAt: string;
  createdAt: string;
  updatedAt: string;
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

export interface MachineAgentInfo extends AgentInfo {
  profilePath: string;
  identityPath: string;
  soulPath: string;
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
  workspaceId?: string | null;
  ownerActorId?: string | null;
  id: string;
  name: string;
  kind: string;
  source: "local_config" | "server_inventory" | string;
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
  providers: AgentProviderSummary[];
  agents: MachineAgentInfo[];
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
