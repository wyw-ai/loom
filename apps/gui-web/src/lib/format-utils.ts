import type React from "react";
import type {
  AudienceRef,
  HumanAccount,
  MachineAgentProviderInfo,
  MachineInfo,
  Task,
} from "@/ipc/types";
import type { Actor } from "@/ipc/types";
import type { AgentFormState, AgentMemberEntry, PanelSizes } from "@/lib/types";
import {
  defaultPanelSizes,
  detailMaxWidth,
  detailMinWidth,
  detailPanelBreakpoint,
  mainMinWidth,
  panelLayoutStorageKey,
  railWidth,
  resizeHandleWidth,
  sidebarMaxWidth,
  sidebarMinWidth,
} from "@/lib/constants";
import type { Thread } from "@/ipc/types";
import type { MessageMention, Workspace } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";
import type { MarkdownNode } from "@/lib/message-utils";

// ---------------------------------------------------------------------------
// Viewport & panel layout
// ---------------------------------------------------------------------------

export function initialViewportWidth() {
  return typeof window === "undefined" ? 1440 : window.innerWidth;
}

export function loadPanelSizes(): PanelSizes {
  if (typeof window === "undefined") return defaultPanelSizes;
  try {
    const raw = window.localStorage.getItem(panelLayoutStorageKey);
    if (!raw) return defaultPanelSizes;
    const parsed = JSON.parse(raw) as Partial<PanelSizes>;
    return fitPanelSizes(
      {
        sidebar:
          typeof parsed.sidebar === "number"
            ? parsed.sidebar
            : defaultPanelSizes.sidebar,
        detail:
          typeof parsed.detail === "number"
            ? parsed.detail
            : defaultPanelSizes.detail,
      },
      initialViewportWidth(),
      initialViewportWidth() >= detailPanelBreakpoint,
    );
  } catch {
    return defaultPanelSizes;
  }
}

export function savePanelSizes(sizes: PanelSizes) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(panelLayoutStorageKey, JSON.stringify(sizes));
  } catch {
    /* local-only preference; ignore quota or privacy-mode failures */
  }
}

export function fitPanelSizes(
  sizes: PanelSizes,
  viewportWidth: number,
  detailVisible: boolean,
): PanelSizes {
  const sidebarMaxForViewport = detailVisible
    ? viewportWidth -
      railWidth -
      resizeHandleWidth * 2 -
      mainMinWidth -
      detailMinWidth
    : viewportWidth - railWidth - resizeHandleWidth - 240;
  const sidebar = clampNumber(
    sizes.sidebar,
    sidebarMinWidth,
    Math.max(sidebarMinWidth, Math.min(sidebarMaxWidth, sidebarMaxForViewport)),
  );
  const detailMaxForViewport =
    viewportWidth -
    railWidth -
    resizeHandleWidth * 2 -
    sidebar -
    mainMinWidth;
  const detail = clampNumber(
    sizes.detail,
    detailMinWidth,
    Math.max(detailMinWidth, Math.min(detailMaxWidth, detailMaxForViewport)),
  );
  return { sidebar, detail };
}

export function clampNumber(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

// ---------------------------------------------------------------------------
// Date helpers (used by both format & message utils)
// ---------------------------------------------------------------------------

export function parseMessageDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function startOfLocalDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

// ---------------------------------------------------------------------------
// Date formatting
// ---------------------------------------------------------------------------

export function formatShortDateTime(value: string) {
  const date = parseMessageDate(value);
  if (!date) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

// ---------------------------------------------------------------------------
// Reconnect
// ---------------------------------------------------------------------------

export function reconnectDelayMs(attempt: number) {
  // The first retry should be effectively immediate so a brief proxy/NAT
  // reset does not leave the composer disabled for a visible interval. Only
  // repeated failures enter exponential backoff.
  if (attempt <= 1) return 0;
  return Math.min(15_000, 500 * 2 ** Math.max(0, attempt - 2));
}

// ---------------------------------------------------------------------------
// Agent form helpers
// ---------------------------------------------------------------------------

export function normalizeAgentForm(form: AgentFormState, machines: MachineInfo[]): AgentFormState {
  const machine = resolveAgentMachine(form, machines);
  const provider = resolveAgentProvider(form, machine);
  return {
    ...form,
    machineId: machine?.id ?? "",
    providerId: provider?.id ?? "",
    model: form.model || provider?.defaultModel || "",
  };
}

export function agentFormForMachine(
  form: AgentFormState,
  machine?: MachineInfo,
): AgentFormState {
  const provider = resolveAgentProvider(form, machine);
  return {
    ...form,
    machineId: machine?.id ?? "",
    providerId: provider?.id ?? "",
    model: provider?.defaultModel || "",
  };
}

export function resolveAgentMachine(
  form: AgentFormState,
  machines: MachineInfo[],
): MachineInfo | undefined {
  const current = machines.find((item) => item.id === form.machineId);
  return (
    current ??
    machines.find((machine) => machineCanCreateAgent(machine) && machine.providers.length > 0) ??
    machines.find(machineCanCreateAgent) ??
    machines[0]
  );
}

export function resolveAgentProvider(
  form: AgentFormState,
  machine?: MachineInfo,
): MachineAgentProviderInfo | undefined {
  return (
    machine?.providers.find((item) => item.id === form.providerId) ??
    machine?.providers[0]
  );
}

export function machineCanCreateAgent(machine: MachineInfo) {
  return (
    machine.connectionStatus === "online" &&
    !machine.readOnly &&
    (machine.capabilities.includes("agent.create") ||
      (machine.canCommand && machine.capabilities.includes("machine.command")))
  );
}

export function machineCanRunCommands(machine: MachineInfo) {
  return (
    machine.connectionStatus === "online" &&
    !machine.readOnly &&
    machine.canCommand &&
    machine.capabilities.includes("machine.command")
  );
}

// ---------------------------------------------------------------------------
// Input event helpers
// ---------------------------------------------------------------------------

export function isComposingKeyEvent(event: React.KeyboardEvent<HTMLTextAreaElement>) {
  return event.nativeEvent.isComposing || event.keyCode === 229;
}

export function shouldSendOnEnter(event: React.KeyboardEvent<HTMLTextAreaElement>) {
  return event.key === "Enter" && !event.shiftKey && !isComposingKeyEvent(event);
}

// ---------------------------------------------------------------------------
// Mention helpers
// ---------------------------------------------------------------------------

export type ActiveMention = {
  start: number;
  end: number;
  query: string;
};

export type MentionOption = {
  kind: "all" | "actor";
  id: string;
  token: string;
  title: string;
  detail: string;
  start: number;
  end: number;
  actor?: Actor;
};

export function mentionAudience(
  body: string,
  mentionableActors: Actor[],
  selfActorId?: string,
): AudienceRef[] {
  const audience: AudienceRef[] = [];
  for (const rawToken of mentionTokens(body)) {
    const key = rawToken.toLowerCase();
    if (key === "all") {
      audience.push({ kind: "all", id: "all", display: "@all" });
      continue;
    }
    if (key === "agents") {
      audience.push({ kind: "agents", id: "agents", display: "@agents" });
      continue;
    }
    if (key === "humans") {
      audience.push({ kind: "humans", id: "humans", display: "@humans" });
      continue;
    }
    const actor = mentionableActors.find((candidate) => {
      if (candidate.id === selfActorId) return false;
      return (
        candidate.id.toLowerCase() === key ||
        displayName(candidate).toLowerCase() === key ||
        shortActorAlias(candidate.id).toLowerCase() === key
      );
    });
    if (actor) audience.push({ kind: "actor", id: actor.id, display: `@${rawToken}` });
  }
  return uniqueAudience(audience);
}

export function mentionTokens(body: string) {
  return Array.from(body.matchAll(/@([^\s,.;:!?()[\]{}<>"'`]+)/g), (match) => match[1]);
}

export function activeMentionQuery(body: string, caretIndex: number): ActiveMention | null {
  const prefix = body.slice(0, caretIndex);
  const match = /(^|\s)@([^\s@]*)$/.exec(prefix);
  if (!match) return null;
  const query = match[2] ?? "";
  return {
    start: match.index + match[1].length,
    end: caretIndex,
    query,
  };
}

export function mentionCandidates(agents: Actor[], active: ActiveMention): MentionOption[] {
  const query = active.query.toLowerCase();
  const options: MentionOption[] = [
    {
      kind: "all",
      id: "all",
      token: "@all",
      title: "All",
      detail: "Notify everyone in this channel",
      start: active.start,
      end: active.end,
    },
    ...agents.map((agent) => {
      const token = mentionTokenForActor(agent);
      return {
        kind: "actor" as const,
        id: agent.id,
        token,
        title: displayName(agent),
        detail: agent.id,
        start: active.start,
        end: active.end,
        actor: agent,
      };
    }),
  ];
  if (!query) return options;
  return options.filter((option) =>
    [option.token.slice(1), option.title, option.detail]
      .map((value) => value.toLowerCase())
      .some((value) => value.includes(query)),
  );
}

export function uniqueAudience(audience: AudienceRef[]) {
  const seen = new Set<string>();
  return audience.filter((entry) => {
    const key = `${entry.kind}:${entry.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function audienceWakesAgent(audience: AudienceRef, actors: Record<string, Actor>) {
  if (audience.kind === "all" || audience.kind === "agents") return true;
  if (audience.kind !== "actor") return false;
  return actors[audience.id]?.kind === "agent";
}

export function mentionTokenForActor(actor: Actor) {
  const name = displayName(actor).trim();
  if (name && !/[\s,.;:!?()[\]{}<>"'`@]/.test(name)) {
    return `@${name}`;
  }
  return `@${shortActorAlias(actor.id)}`;
}

// ---------------------------------------------------------------------------
// Actor identity helpers
// ---------------------------------------------------------------------------

export function shortActorAlias(actorId: string) {
  return (
    actorId.replace(
      /^(actor_agent_|actor_human_|actor_service_|actor_)/,
      "",
    ) || actorId
  );
}

export function fallbackActor(actorId: string): Actor {
  if (actorId.startsWith("actor_agent_")) return { id: actorId, kind: "agent" };
  if (actorId.startsWith("actor_service_")) return { id: actorId, kind: "service" };
  return { id: actorId, kind: "human" };
}

export function displayName(actor: Actor) {
  return actor.displayName || actor.id;
}

export function actorName(actors: Record<string, Actor>, actorId: string) {
  return actors[actorId] ? displayName(actors[actorId]) : actorId;
}

export function accountToActor(account: HumanAccount): Actor {
  return {
    id: account.actorId,
    kind: "human",
    displayName: accountName(account),
    _meta: { avatarUrl: account.avatarUrl },
  };
}

export function uniqueActorsById(actors: Actor[]) {
  const seen = new Set<string>();
  return actors.filter((actor) => {
    if (seen.has(actor.id)) return false;
    seen.add(actor.id);
    return true;
  });
}

// ---------------------------------------------------------------------------
// Online presence & status
// ---------------------------------------------------------------------------

export function isOnlinePresenceStatus(
  status: string,
  agent: MachineInfo["agents"][number],
) {
  const normalized = status.toLowerCase();
  if (["online", "connected", "running", "busy", "idle", "active"].includes(normalized)) {
    return true;
  }
  if (["offline", "stopped", "disconnected", "failed", "error", "exited"].includes(normalized)) {
    return false;
  }
  return Boolean(agent.pid || agent.sessionId);
}

export function statusDotClass(status: string) {
  const normalized = status.toLowerCase();
  if (["online", "connected", "running", "busy", "idle", "active"].includes(normalized)) {
    return "bg-emerald-400";
  }
  if (normalized === "connecting" || normalized === "pending") return "bg-amber-400";
  if (normalized === "error" || normalized === "failed") return "bg-red-400";
  return "bg-[#98a2b3]";
}

export function taskStatusBadgeClass(status: Task["status"]) {
  switch (status) {
    case "todo":
      return "border-slate-200 bg-slate-100 text-slate-700";
    case "claimed":
      return "border-violet-200 bg-violet-50 text-violet-700";
    case "in_progress":
      return "border-blue-200 bg-blue-50 text-blue-700";
    case "waiting_review":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "done":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "failed":
      return "border-red-200 bg-red-50 text-red-700";
    case "canceled":
      return "border-slate-200 bg-slate-100 text-slate-500";
  }
}

// ---------------------------------------------------------------------------
// Agent member lookup
// ---------------------------------------------------------------------------

export function findAgentMemberEntry(
  machines: MachineInfo[],
  actorId: string,
): AgentMemberEntry | null {
  for (const machine of machines) {
    const agent = machine.agents.find((item) => item.spec.actor.id === actorId);
    if (agent) return { machine, agent };
  }
  return null;
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

export function sortThreads(items: Thread[]) {
  return [...items]
    .filter((thread) => !thread.archivedAt)
    .sort((a, b) => a.title.localeCompare(b.title));
}

export function sortTasks(items: Task[]) {
  return [...items].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
}

// ---------------------------------------------------------------------------
// Generic utilities
// ---------------------------------------------------------------------------

export function upsert<T extends { id: string }>(items: T[], item: T) {
  const index = items.findIndex((candidate) => candidate.id === item.id);
  if (index === -1) return [...items, item];
  const next = [...items];
  next[index] = { ...next[index], ...item };
  return next;
}

export function uniqueStrings(values: string[]) {
  return Array.from(new Set(values.filter((value) => value.length > 0)));
}

export function errorText(err: unknown) {
  return err instanceof Error ? err.message : String(err);
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "Unknown size";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  for (const unit of units) {
    if (value < 1024) return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
    value /= 1024;
  }
  return `${value.toFixed(1)} PB`;
}

export function formatFileTimestamp(value: string | null | undefined): string {
  if (!value) return "";
  try {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "";
    const now = new Date();
    const sameDay =
      date.getFullYear() === now.getFullYear() &&
      date.getMonth() === now.getMonth() &&
      date.getDate() === now.getDate();
    if (sameDay) {
      return date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    }
    return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  } catch {
    return "";
  }
}

export function filterEmptyEnvKeys(env: Record<string, string>): Record<string, string> {
  const filtered: Record<string, string> = {};
  for (const [key, value] of Object.entries(env)) {
    if (key.trim()) {
      filtered[key.trim()] = value;
    }
  }
  return filtered;
}

export function accountName(account: HumanAccount) {
  return account.nickname || account.realName || account.email || account.staffId;
}

export function workspaceInitials(workspace: Workspace) {
  const words = workspace.name
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  const initials =
    words.length > 1
      ? `${words[0][0] ?? ""}${words[1][0] ?? ""}`
      : (words[0] ?? workspace.id).slice(0, 2);
  return initials.toUpperCase();
}

export function capitalize(value: string) {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}


// ---------------------------------------------------------------------------
// Actor mention / remark plugin
// ---------------------------------------------------------------------------

const actorMentionUrlPrefix = "https://loom.local/actors/";
const actorMentionPattern = /@([^\s,.;:!?()[\]{}<>"'\`]+)/g;

export function actorMentionRemarkPlugin(
  actors: Record<string, Actor>,
  mentions: MessageMention[] = [],
) {
  const explicitActorMentions = actorMentionLookup(mentions);
  return function transformActorMentions() {
    return (tree: MarkdownNode) => {
      transformMarkdownTextMentions(tree, actors, explicitActorMentions);
    };
  };
}

function transformMarkdownTextMentions(
  node: MarkdownNode,
  actors: Record<string, Actor>,
  explicitActorMentions: Map<string, string>,
) {
  if (node.type === "link" || node.type === "linkReference") return;
  if (!node.children) return;

  for (let index = 0; index < node.children.length; index += 1) {
    const child = node.children[index];
    if (child.type === "text" && typeof child.value === "string") {
      const replacement = actorMentionNodes(child.value, actors, explicitActorMentions);
      if (replacement) {
        node.children.splice(index, 1, ...replacement);
        index += replacement.length - 1;
      }
      continue;
    }
    transformMarkdownTextMentions(child, actors, explicitActorMentions);
  }
}

function actorMentionNodes(
  value: string,
  actors: Record<string, Actor>,
  explicitActorMentions: Map<string, string>,
): MarkdownNode[] | null {
  const nodes: MarkdownNode[] = [];
  let lastIndex = 0;
  let matched = false;
  actorMentionPattern.lastIndex = 0;

  for (const match of value.matchAll(actorMentionPattern)) {
    const token = match[1];
    const actorId = actorIdForMentionToken(token, actors, explicitActorMentions);
    if (!actorId) continue;
    const actor = actors[actorId];
    if (!actor || match.index === undefined) continue;

    if (match.index > lastIndex) {
      nodes.push({ type: "text", value: value.slice(lastIndex, match.index) });
    }
    nodes.push({
      type: "link",
      url: actorMentionUrl(actor.id),
      title: actor.id,
      children: [{ type: "text", value: `@${displayName(actor)}` }],
    });
    lastIndex = match.index + match[0].length;
    matched = true;
  }

  if (!matched) return null;
  if (lastIndex < value.length) {
    nodes.push({ type: "text", value: value.slice(lastIndex) });
  }
  return nodes;
}

function actorMentionLookup(mentions: MessageMention[]) {
  const lookup = new Map<string, string>();
  for (const mention of mentions) {
    if (mention.kind !== "actor") continue;
    const display = mention.display.trim();
    if (!display) continue;
    lookup.set(normalizeMentionToken(display), mention.actorOrGroupId);
  }
  return lookup;
}

function actorIdForMentionToken(
  token: string,
  actors: Record<string, Actor>,
  explicitActorMentions: Map<string, string>,
) {
  const key = normalizeMentionToken(token);
  if (key === "all" || key === "agents" || key === "humans") return null;
  const explicitActorId = explicitActorMentions.get(key);
  if (explicitActorId) return explicitActorId;
  return Object.values(actors).find((actor) => {
    return (
      normalizeMentionToken(actor.id) === key ||
      normalizeMentionToken(displayName(actor)) === key ||
      normalizeMentionToken(shortActorAlias(actor.id)) === key
    );
  })?.id ?? null;
}

function normalizeMentionToken(value: string) {
  return value.trim().replace(/^@+/, "").toLowerCase();
}

function actorMentionUrl(actorId: string) {
  return `${actorMentionUrlPrefix}${encodeURIComponent(actorId)}`;
}

export function actorMentionActorId(href: string) {
  if (!href.startsWith(actorMentionUrlPrefix)) return null;
  try {
    return decodeURIComponent(href.slice(actorMentionUrlPrefix.length));
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Connection / channel labels
// ---------------------------------------------------------------------------

export function connectionLabel(connection: ConnectionState) {
  if (connection === "open") return "Connected";
  if (connection === "connecting") return "Connecting";
  if (connection === "error") return "Connection error";
  if (connection === "closed") return "Disconnected";
  return "Idle";
}
