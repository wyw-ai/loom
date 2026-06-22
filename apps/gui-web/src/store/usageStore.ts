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
  /** Scope the usage was recorded in (message.scope.id). Iter#5: enables
   * channel/thread-level aggregation without a BE change. */
  scopeId: string;
  /** "channel" | "thread" — mirrors message.scope.kind. */
  scopeKind: string;
}

/**
 * Aggregated token usage for a single channel or thread scope (Iter#5 Part B).
 * Derived on the FE by summing per-actor snapshots whose `scopeId` matches and
 * whose `cumulative.totalTokens > 0`. Each actor's `cumulative` is the BE
 * authoritative scope-cumulative value (see ARCH Part B §B0), so summing them
 * yields the scope total with no drift risk.
 */
export interface ScopeUsageSummary {
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  byActor: Array<{
    actorId: string;
    totalTokens: number;
    inputTokens: number;
    outputTokens: number;
    updatedAt: string;
  }>;
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
      scopeId: message.scope?.id ?? "",
      scopeKind: message.scope?.kind ?? "",
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

/**
 * FE-side scope aggregation selector (Iter#5 Part B).
 *
 * Sums per-actor `cumulative` snapshots whose `scopeId === scopeId` and whose
 * `cumulative.totalTokens > 0`. Returns `null` when no actor in the scope has
 * meaningful usage yet, so callers can silent-hide the L1 summary.
 *
 * Each actor's `cumulative` is already the BE's scope-cumulative value
 * (agent_serve.rs `accumulate_usage(scope_id)`), so the sum is the scope total
 * with no double-counting: different actors in the same scope contribute
 * different, non-overlapping turn totals.
 */
export function useScopeUsageSummary(scopeId: string | null | undefined): ScopeUsageSummary | null {
  return useUsageStore((state) => {
    if (!scopeId) return null;
    const byActor: ScopeUsageSummary["byActor"] = [];
    let totalTokens = 0;
    let inputTokens = 0;
    let outputTokens = 0;
    let cacheReadTokens = 0;
    for (const [actorId, snap] of Object.entries(state.byActor)) {
      if (snap.scopeId !== scopeId) continue;
      const tot = snap.cumulative.total_tokens ?? 0;
      if (tot <= 0) continue;
      totalTokens += tot;
      inputTokens += snap.cumulative.input_tokens ?? 0;
      outputTokens += snap.cumulative.output_tokens ?? 0;
      cacheReadTokens += snap.cumulative.cache_read_input_tokens ?? 0;
      byActor.push({
        actorId,
        totalTokens: tot,
        inputTokens: snap.cumulative.input_tokens ?? 0,
        outputTokens: snap.cumulative.output_tokens ?? 0,
        updatedAt: snap.updatedAt,
      });
    }
    if (totalTokens <= 0) return null;
    byActor.sort((a, b) => b.totalTokens - a.totalTokens);
    return { totalTokens, inputTokens, outputTokens, cacheReadTokens, byActor };
  });
}

// Internal helper exported for tests.
export { shouldReplace as __shouldReplaceForTest };
export type { MessageTokenUsageMeta };
