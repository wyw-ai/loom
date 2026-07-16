import type { Actor, AgentBundleSkillSpec, Channel, MachineAgentProviderInfo, MachineInfo, Thread, WakeSpec } from "@/ipc/types";

export type ConnectionState = "idle" | "connecting" | "open" | "closed" | "error";
export type View = "chat" | "threads" | "channels" | "direct" | "inbox" | "tasks" | "spaces" | "account" | "settings";
export type ChannelPanelTab = "threads" | "members" | "tasks" | "configure";
export type ChannelGroup = {
  id: string;
  title: string;
  channelIds: string[];
  collapsed: boolean;
};
export type ChannelGroupSection = {
  id: string;
  title: string;
  channels: Channel[];
  collapsed: boolean;
  local: boolean;
};
export type ThreadWithChannel = Thread & {
  channel: Channel;
};
export type ActionChoice = {
  id: string;
  label: string;
  accepted: boolean;
  votes?: number;
};
export type BodyPoll = {
  question: string;
  choices: ActionChoice[];
};
export type ChannelPointerDrag = {
  channelId: string;
  startX: number;
  startY: number;
  pointerId: number;
  dragging: boolean;
};
export type ChannelContextMenu = {
  channelId: string;
  x: number;
  y: number;
};
export type PanelResizeKind = "sidebar" | "detail";
export type PanelSizes = {
  sidebar: number;
  detail: number;
};
export type PanelResizeDrag = {
  kind: PanelResizeKind;
  startX: number;
  sidebar: number;
  detail: number;
};
export type ThreadActivityStats = {
  replyCount: number;
  replyMessageIds: string[];
  participantActorIds: string[];
  hasMoreReplies: boolean;
  lastReplyAt: string | null;
};
export type AgentFormState = {
  machineId: string;
  providerId: string;
  actorId: string;
  name: string;
  description: string;
  instructions: string;
  model: string;
  autostart: boolean;
  env: Record<string, string>;
  wake: WakeSpec;
};
export type WorkspaceFormState = {
  name: string;
  host: string;
  advanced: boolean;
  serverUrl: string;
};
export type AgentUpdatePatch = {
  machineId: string;
  actorId: string;
  displayName: string;
  description: string;
  instructions: string;
  providerId?: string;
  model: string;
  reasoningEffort: string;
  autostart: boolean;
  avatarUrl: string;
  env: Record<string, string>;
  bundleSkills: AgentBundleSkillSpec[];
  wake: WakeSpec;
};
export type AgentSettingsDraft = {
  displayName: string;
  description: string;
  instructions: string;
  providerId: string;
  model: string;
  reasoningEffort: string;
  autostart: boolean;
  avatarUrl: string;
  env: Record<string, string>;
  bundleSkills: AgentBundleSkillSpec[];
  wake: WakeSpec;
};
export type AgentMemberEntry = {
  machine: MachineInfo;
  agent: MachineInfo["agents"][number];
};
export type ServiceMemberEntry = {
  machine: MachineInfo;
  service: MachineInfo["services"][number];
};
export type ProviderAvailabilityGroup = {
  key: string;
  id: string;
  name: string;
  transportKind: string;
  actorCount: number;
  defaultModels: string[];
  hosts: Array<{
    machine: MachineInfo;
    provider: MachineAgentProviderInfo;
  }>;
};
export type ActorWorkspaceSection = "agents" | "hosts" | "services";
export type AgentDetailTab = "profile" | "prompt" | "skills" | "settings";
export type ServiceDetailTab = "overview" | "spec" | "config";
export type PromptTemplateDraft = {
  system: string;
  user: string;
};
export type PromptAssemblyBuildOptions = {
  includeAllFiles?: boolean;
};
export type ChannelMemberPresence = {
  online: boolean;
  label: string;
  status: string;
};
export type ChannelMemberPanelItem = {
  actor: Actor;
  presence: ChannelMemberPresence;
};
