import { create } from "zustand";
import type {
  Message,
  MessageTokenUsageMeta,
  PromptBreakdown,
  TokenUsage,
} from "@/ipc/types";
import {
  readMessagePromptBreakdown,
  readMessageTokenUsage,
} from "@/ipc/types";

// ---------------------------------------------------------------------------
// AgentUsageSnapshot — per-actor latest token-usage view.
//
// PRD: art_18a2865ccbcd / ARCH design: art_f822814f9124 (D5 channel A).
// Single-agent MVP per PRD D3: keyed by `actorId` (the agent's actor id).
//
// Each assistant `message.created` event carries `metadata.token_usage =
// { increment, cumulative }` from BE `build_turn_meta`. We treat the value
// from the newest message (by `createdAt`) as authoritative; older messages
// arriving out of order are ignored.
//
// `promptBreakdown` is the BE `prompt_breakdown` sections (from the same
// turn meta), used by U8 to derive per-segment token counts.
// ---------------------------------------------------------------------------

const MAX_ENTRIES = 32;

export interface AgentUsageSnapshot {
  increment: TokenUsage;
  cumulative: TokenUsage;
  promptBreakdown?: PromptBreakdown | null;
  updatedAt: string;
  messageId: string;
}

export interface UsageStore {
  byActor: Record<string, AgentUsageSnapshot>;
  applyMessage: (message: Message) => void;
  reset: () => void;
}

function shouldReplace(prev: AgentUsageSnapshot | undefined, candidate: { updatedAt: string; messageId: string }): boolean {
  if (!prev) return true;
  if (candidate.updatedAt !== prev.updatedAt) return candidate.updatedAt > prev.updatedAt;
  return candidate.messageId > prev.messageId;
}

function trimEntries(record: Record<string, AgentUsageSnapshot>): Record<string, AgentUsageSnapshot> {
  const keys = Object.keys(record);
  if (keys.length <= MAX_ENTRIES) return record;
  const sorted = keys
    .map((id) => ({ id, snap: record[id] }))
    .sort((a, b) => b.snap.updatedAt.localeCompare(a.snap.updatedAt))
    .slice(0, MAX_ENTRIES);
  const next: Record<string, AgentUsageSnapshot> = {};
  for (const { id, snap } of sorted) next[id] = snap;
  return next;
}

export const useUsageStore = create<UsageStore>((set, get) => ({
  byActor: {},

  applyMessage: (message: Message) => {
    if (message.kind !== "agent") return;
    const usage = readMessageTokenUsage(message.metadata);
    if (!usage) return;
    const actorId = message.authorActorId;
    if (!actorId) return;
    const candidate: AgentUsageSnapshot = {
      increment: usage.increment,
      cumulative: usage.cumulative,
      promptBreakdown: readMessagePromptBreakdown(message.metadata),
      updatedAt: message.createdAt,
      messageId: message.id,
    };
    const prev = get().byActor[actorId];
    if (!shouldReplace(prev, candidate)) return;
    set((state) => ({
      byActor: trimEntries({ ...state.byActor, [actorId]: candidate }),
    }));
  },

  reset: () => set({ byActor: {} }),
}));

export function applyMessageToUsageStore(message: Message): void {
  useUsageStore.getState().applyMessage(message);
}

export function useAgentUsage(actorId: string | null | undefined): AgentUsageSnapshot | null {
  return useUsageStore((state) =>
    actorId ? state.byActor[actorId] ?? null : null,
  );
}

// Internal helper exported for tests.
export { shouldReplace as __shouldReplaceForTest };
export type { MessageTokenUsageMeta };
