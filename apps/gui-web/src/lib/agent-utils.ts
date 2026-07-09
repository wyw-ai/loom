import type {
  Actor,
  MachineAgentProviderInfo,
  MachineInfo,
  Run,
  RunStatus,
} from "@/ipc/types";
import type {
  AgentMemberEntry,
  AgentSettingsDraft,
  ChannelMemberPresence,
  ChannelPanelTab,
  ProviderAvailabilityGroup,
  ServiceMemberEntry,
} from "@/lib/types";
import { agentAvatarIndexes, avatarCount } from "@/lib/constants";
import { metadataString, metadataNumber } from "@/lib/message-utils";
import { normalizeWakeSpec } from "@/lib/wake-utils";
import {
  displayName,
  isOnlinePresenceStatus,
  findAgentMemberEntry,
} from "@/lib/format-utils";

// ---------------------------------------------------------------------------
// Agent member entries & provider availability
// ---------------------------------------------------------------------------

export function agentMemberEntries(machines: MachineInfo[]): AgentMemberEntry[] {
  return machines.flatMap((machine) =>
    machine.agents.map((agent) => ({ machine, agent })),
  );
}

export function serviceMemberEntries(machines: MachineInfo[]): ServiceMemberEntry[] {
  return machines.flatMap((machine) =>
    (machine.services || []).map((service) => ({ machine, service })),
  );
}

export function providerAvailabilityGroups(machines: MachineInfo[]): ProviderAvailabilityGroup[] {
  const groups = new Map<string, ProviderAvailabilityGroup>();
  for (const machine of machines) {
    for (const provider of machine.providers) {
      const key = `${provider.id}:${provider.transportKind}`;
      const current =
        groups.get(key) ??
        {
          key,
          id: provider.id,
          name: provider.name || provider.id,
          transportKind: provider.transportKind,
          actorCount: 0,
          defaultModels: [],
          hosts: [],
        };
      current.actorCount += provider.actorCount;
      if (
        provider.defaultModel &&
        !current.defaultModels.includes(provider.defaultModel)
      ) {
        current.defaultModels.push(provider.defaultModel);
      }
      current.hosts.push({ machine, provider });
      groups.set(key, current);
    }
  }
  return Array.from(groups.values()).sort((a, b) =>
    a.name.localeCompare(b.name) ||
    a.transportKind.localeCompare(b.transportKind) ||
    a.id.localeCompare(b.id),
  );
}

// ---------------------------------------------------------------------------
// Agent display helpers
// ---------------------------------------------------------------------------

export function agentDisplayName(agent: MachineInfo["agents"][number]) {
  return displayName(agent.spec.actor);
}

export function agentModelValue(agent: MachineInfo["agents"][number]) {
  return agent.spec.providerRef.model ?? agent.spec.models?.default ?? "";
}

export function agentDescriptionValue(agent: MachineInfo["agents"][number]) {
  const value = agent.spec.actor._meta?.description;
  return typeof value === "string" ? value : "";
}

export function agentInstructionsValue(agent: MachineInfo["agents"][number]) {
  return agent.spec.instructions ?? "";
}

export function agentReasoningEffort(agent: MachineInfo["agents"][number]) {
  return agent.spec.providerRef.reasoningEffort ?? "";
}

export function agentProviderId(agent: MachineInfo["agents"][number]) {
  return metadataString(agent.spec.actor._meta, ["providerId", "provider_id"]);
}

export function agentAvatarValue(agent: MachineInfo["agents"][number]) {
  return metadataAvatarUrl(agent.spec.actor._meta) ?? actorAvatarUrl(agent.spec.actor, agent.spec.actor.id);
}

export function providerForAgent(
  machine: MachineInfo,
  agent: MachineInfo["agents"][number],
  preferredProviderId?: string,
) {
  const providerId = preferredProviderId || agentProviderId(agent);
  const preferred = machine.providers.find((provider) => provider.id === providerId);
  if (preferred) return preferred;
  const providerRef = machine.providers.find(
    (provider) => provider.id === agent.spec.providerRef.id,
  );
  if (providerRef) return providerRef;
  const model = agentModelValue(agent);
  return (
    machine.providers.find(
      (provider) =>
        provider.defaultModel === model ||
        provider.modelChoices.some((choice) => choice.id === model),
    ) ??
    (machine.providers.length === 1 ? machine.providers[0] : undefined) ??
    machine.providers[0]
  );
}

export function agentSettingsDraft(
  machine: MachineInfo,
  agent: MachineInfo["agents"][number],
): AgentSettingsDraft {
  const providerId = agentProviderId(agent);
  const provider = providerForAgent(machine, agent, providerId ?? undefined);
  return {
    displayName: agentDisplayName(agent),
    description: agentDescriptionValue(agent),
    instructions: agentInstructionsValue(agent),
    providerId: providerId ?? provider?.id ?? "",
    model: agentModelValue(agent) || provider?.defaultModel || "",
    reasoningEffort: agentReasoningEffort(agent),
    autostart: Boolean(agent.spec.autostart),
    avatarUrl: agentAvatarValue(agent),
    env: { ...(agent.spec.providerRef.env ?? {}) },
    bundleSkills: [...(agent.spec.bundle?.skills ?? [])],
    wake: normalizeWakeSpec(agent.spec.wake),
  };
}

// ---------------------------------------------------------------------------
// ActorRunContext
// ---------------------------------------------------------------------------

export interface ActorRunContext {
  status: RunStatus;
  run: Run;
  isStale: boolean;
  staleThresholdSec: number;
  isTerminal: boolean;
  terminalExpired: boolean;
}

const STALE_THRESHOLDS: Record<string, number> = {
  queued: 300,
  preparing_context: 180,
  running: 600,
  waiting_tool: 120,
};
const TERMINAL_SHOW_SEC = 30;

export function getActorRunContext(runs: Record<string, Run>, actorId: string): ActorRunContext | null {
  const statusPriority: Record<string, number> = {
    running: 4,
    waiting_tool: 3,
    preparing_context: 2,
    queued: 1,
  };
  let best: { run: Run; priority: number } | null = null;
  for (const run of Object.values(runs)) {
    if (run.actorId !== actorId) continue;
    const p = statusPriority[run.status] ?? 0;
    if (p > (best?.priority ?? 0)) {
      best = { run, priority: p };
    }
  }

  if (!best) {
    const now = Date.now();
    for (const run of Object.values(runs)) {
      if (run.actorId !== actorId) continue;
      if (run.status === "failed" || run.status === "canceled") {
        if (run.closedAt) {
          const elapsed = (now - new Date(run.closedAt).getTime()) / 1000;
          if (elapsed <= TERMINAL_SHOW_SEC) {
            return {
              status: run.status,
              run,
              isStale: false,
              staleThresholdSec: 0,
              isTerminal: true,
              terminalExpired: false,
            };
          }
        }
      }
    }
    return null;
  }

  const { run } = best;
  const threshold = STALE_THRESHOLDS[run.status] ?? 600;
  const elapsed = (Date.now() - new Date(run.openedAt).getTime()) / 1000;
  const isStale = elapsed > threshold;

  return {
    status: run.status,
    run,
    isStale,
    staleThresholdSec: threshold,
    isTerminal: false,
    terminalExpired: false,
  };
}

export const runStatusLabel: Record<string, string> = {
  queued: "Queued",
  preparing_context: "Preparing",
  running: "Thinking",
  waiting_tool: "Running tool",
  failed: "Failed",
  canceled: "Canceled",
};

/**
 * Shared status phrase for ChatHeader activity label and AgentIdentityBadge
 * working label (Iter#5 Part C §C4 AC-S5).
 *
 * Returns the base action phrase WITHOUT the health suffix; callers that need
 * the full label (with health/timing) should use `runStatusFullLabel`.
 */
export function runStatusPhrase(ctx: ActorRunContext | null): string | undefined {
  if (!ctx) return undefined;

  if (ctx.isTerminal) {
    const meta = ctx.run.metadata ?? {};
    const error = typeof meta.error === "string" ? meta.error : undefined;
    const noReplyReason = typeof meta.noReplyReason === "string" ? meta.noReplyReason : undefined;
    const reason = error || noReplyReason || ctx.run.startReason;
    const base = runStatusLabel[ctx.status] ?? ctx.status;
    return reason ? `${base} · ${reason}` : base;
  }

  const base = runStatusLabel[ctx.status] ?? ctx.status;

  const reason = ctx.run.startReason;
  const meta = ctx.run.metadata ?? {};

  if (ctx.status === "queued" && reason) return `Queued · ${reason}`;
  if (ctx.status === "preparing_context" && reason) return `Preparing · ${reason}`;
  if (ctx.status === "running" && reason) return `Thinking · ${reason}`;
  if (ctx.status === "waiting_tool") {
    const toolName = typeof meta.toolName === "string" ? meta.toolName : undefined;
    if (toolName) return `Running tool · ${toolName}`;
  }

  return base;
}

/**
 * Full status label with 3-tier health suffix (Iter#5 Part C §C3 AC-S2).
 *
 * Replaces the old `⚠ {base} (timeout)` bracket pattern with:
 * - (a) normal: `{base}` (no suffix)
 * - (b) slow: `{base} · waiting {min}m · slow` (isStale && elapsed < threshold*1.5)
 * - (c) stuck: `{base} · waiting {min}m · possibly stuck` (isStale && elapsed >= threshold*1.5)
 * - (d) server-confirmed timeout: terminal failed/canceled with timeout reason
 */
export function runStatusFullLabel(ctx: ActorRunContext | null): string | undefined {
  if (!ctx) return undefined;

  if (ctx.isTerminal) {
    const meta = ctx.run.metadata ?? {};
    const error = typeof meta.error === "string" ? meta.error : undefined;
    const noReplyReason = typeof meta.noReplyReason === "string" ? meta.noReplyReason : undefined;
    const reason = error || noReplyReason || ctx.run.startReason;
    const base = runStatusLabel[ctx.status] ?? ctx.status;
    return reason ? `${base} · ${reason}` : base;
  }

  const base = runStatusPhrase(ctx) ?? runStatusLabel[ctx.status] ?? ctx.status;

  if (ctx.isStale) {
    const elapsedSec = (Date.now() - new Date(ctx.run.openedAt).getTime()) / 1000;
    const minutes = Math.floor(elapsedSec / 60);
    const elapsedLabel = minutes > 0 ? `${minutes}m` : `${Math.floor(elapsedSec)}s`;
    if (elapsedSec >= ctx.staleThresholdSec * 1.5) {
      return `${base} · waiting ${elapsedLabel} · possibly stuck`;
    }
    return `${base} · waiting ${elapsedLabel} · slow`;
  }

  return base;
}

export function runStatusDotClass(ctx: ActorRunContext | null): string {
  if (!ctx) return "bg-[#98a2b3]";

  if (ctx.isTerminal) {
    if (ctx.status === "failed") return "bg-red-500";
    return "bg-gray-500";
  }

  if (ctx.isStale) return "bg-yellow-400 shadow-[0_0_6px_rgba(250,204,21,0.5)]";

  switch (ctx.status) {
    case "queued": return "bg-gray-400";
    case "preparing_context": return "bg-blue-400";
    case "running": return "bg-purple-400 shadow-[0_0_6px_rgba(168,85,247,0.5)]";
    case "waiting_tool": return "bg-orange-400 shadow-[0_0_6px_rgba(251,146,60,0.5)]";
    default: return "bg-[#98a2b3]";
  }
}

export function runStatusAnimationName(ctx: ActorRunContext | null): string | undefined {
  if (!ctx) return undefined;
  if (ctx.isTerminal) {
    return ctx.status === "failed" ? "agent-status-failed" : "agent-status-canceled";
  }
  if (ctx.isStale) return "agent-status-stale-warning";
  return `agent-status-${ctx.status.replace(/_/g, "-")}`;
}

// ---------------------------------------------------------------------------
// Agent model labels & token display
// ---------------------------------------------------------------------------

export function agentModelLabel(
  agent: MachineInfo["agents"][number],
  provider?: MachineAgentProviderInfo,
) {
  const model = agentModelValue(agent);
  const modelChoices = [
    ...(provider?.modelChoices ?? []),
    ...(agent.spec.models?.choices ?? []),
  ];
  return modelChoices.find((choice) => choice.id === model)?.label || model || "Default model";
}

export function providerMark(providerName: string) {
  if (/anthropic|claude/i.test(providerName)) return "AI";
  const words = providerName.match(/[A-Za-z0-9]+/g) ?? [];
  if (words.length >= 2) {
    const first = words[0]?.[0] ?? "";
    const second = words[1]?.[0] ?? "";
    return `${first}${second}`.toUpperCase() || "AI";
  }
  return providerName.trim().slice(0, 2).toUpperCase() || "AI";
}

export function agentContextUsedLabel(agent: MachineInfo["agents"][number]) {
  const meta = agent.spec._meta;
  const explicit = metadataString(meta, ["contextUsedLabel", "usedTokensLabel"]);
  if (explicit) return explicit;
  const used = metadataNumber(meta, ["contextUsedTokens", "usedTokens"]);
  const total = metadataNumber(meta, ["contextWindowTokens", "totalTokens", "maxTokens"]);
  if (typeof used === "number" && typeof total === "number" && total > 0) {
    return `${compactTokenCount(used)} / ${compactTokenCount(total)} tokens`;
  }
  return "112.0K / 128.0K tokens";
}

export function agentContextRemainingLabel(agent: MachineInfo["agents"][number]) {
  const meta = agent.spec._meta;
  const explicit = metadataString(meta, ["contextRemainingLabel", "remainingLabel"]);
  if (explicit) return explicit;
  const remainingPercent = metadataNumber(meta, ["contextRemainingPercent", "remainingPercent"]);
  if (typeof remainingPercent === "number") {
    const normalized = remainingPercent <= 1 ? remainingPercent * 100 : remainingPercent;
    return `${Math.max(0, Math.round(normalized))}%`;
  }
  const used = metadataNumber(meta, ["contextUsedTokens", "usedTokens"]);
  const total = metadataNumber(meta, ["contextWindowTokens", "totalTokens", "maxTokens"]);
  if (typeof used === "number" && typeof total === "number" && total > 0) {
    return `${Math.max(0, Math.round(((total - used) / total) * 100))}%`;
  }
  return "13%";
}

export function compactTokenCount(value: number) {
  if (value >= 1000) return `${(value / 1000).toFixed(1)}K`;
  return `${Math.round(value)}`;
}

// ---------------------------------------------------------------------------
// Avatars
// ---------------------------------------------------------------------------

export function actorAvatarUrl(actor: Actor | undefined, fallback: string) {
  const metaAvatar = actor?._meta ? metadataAvatarUrl(actor._meta) : null;
  if (metaAvatar) return metaAvatar;
  const seed = `${actor?.kind ?? "actor"}:${actor?.id ?? fallback}:${actor ? displayName(actor) : ""}`;
  return avatarUrlForSeed(seed, actor?.kind);
}

export function metadataAvatarUrl(meta: unknown) {
  if (!meta || typeof meta !== "object") return null;
  const value = (meta as { avatarUrl?: unknown }).avatarUrl;
  return typeof value === "string" && value.trim() ? value : null;
}

export function avatarUrlForSeed(seed: string, kind: Actor["kind"] = "human") {
  const hash = stableHash(seed);
  const index =
    kind === "agent"
      ? agentAvatarIndexes[hash % agentAvatarIndexes.length]
      : (hash % avatarCount) + 1;
  return `/avatars/avatar-${String(index).padStart(2, "0")}.png`;
}

function stableHash(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash;
}

// ---------------------------------------------------------------------------
// Channel agent activity (aggregate across all agents)
// ---------------------------------------------------------------------------

export interface ChannelAgentActivity {
  /** Display name of the highest-priority active agent */
  primaryAgentName: string;
  /** Status of the highest-priority active agent */
  primaryStatus: RunStatus;
  /** Total number of active (non-terminal) agent runs */
  activeCount: number;
  /** Whether any agent has a failed run */
  hasFailed: boolean;
  /** Whether all agents are idle (no active runs) */
  isIdle: boolean;
}

const ACTIVITY_PRIORITY: Record<string, number> = {
  running: 4,
  waiting_tool: 3,
  preparing_context: 2,
  queued: 1,
};

export function getChannelAgentActivity(
  runs: Record<string, Run>,
  agentActors: Actor[],
): ChannelAgentActivity | null {
  let best: { actorId: string; status: RunStatus; priority: number } | null = null;
  let activeCount = 0;
  let hasFailed = false;

  for (const actor of agentActors) {
    const ctx = getActorRunContext(runs, actor.id);
    if (!ctx || ctx.isTerminal) {
      if (ctx?.status === "failed") hasFailed = true;
      continue;
    }
    activeCount++;
    const p = ACTIVITY_PRIORITY[ctx.status] ?? 0;
    if (p > (best?.priority ?? 0)) {
      best = { actorId: actor.id, status: ctx.status, priority: p };
    }
  }

  if (activeCount === 0 && !hasFailed) return null;

  const primaryAgentName = best
    ? displayName(agentActors.find((a) => a.id === best!.actorId) ?? agentActors[0])
    : "";

  return {
    primaryAgentName,
    primaryStatus: best?.status ?? "failed",
    activeCount,
    hasFailed,
    isIdle: false,
  };
}

// Channel panel helpers
// ---------------------------------------------------------------------------

export function channelPanelTitle(tab: ChannelPanelTab) {
  if (tab === "threads") return "线程";
  if (tab === "members") return "成员";
  return "任务";
}

export function channelPanelDetail(
  tab: ChannelPanelTab,
  threadCount: number,
  memberCount: number,
  taskCount: number,
) {
  if (tab === "threads") return `${threadCount} active threads`;
  if (tab === "members") return `${memberCount} members`;
  return `${taskCount} tasks`;
}

export function actorKindLabel(actor: Actor) {
  if (actor.kind === "agent") return "智能体";
  if (actor.kind === "service") return "服务";
  return "成员";
}

// ---------------------------------------------------------------------------
// Channel member helpers
// ---------------------------------------------------------------------------

export function memberPresence(
  actor: Actor,
  machines: MachineInfo[],
  currentActorId: string | null,
): ChannelMemberPresence {
  if (actor.id === currentActorId) {
    return { online: true, label: "在线", status: "online" };
  }
  if (actor.kind === "agent") {
    const entry = findAgentMemberEntry(machines, actor.id);
    if (!entry) return { online: false, label: "离线", status: "offline" };
    const rawStatus = entry.agent.status || "offline";
    const online = isOnlinePresenceStatus(rawStatus, entry.agent);
    return {
      online,
      label: online ? "在线" : "离线",
      status: rawStatus,
    };
  }
  return { online: false, label: "离线", status: "offline" };
}
