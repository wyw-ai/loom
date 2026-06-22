import type {
  AgentIdentityBadgeSegment,
} from "@/components/agent/AgentIdentityBadge";
import type {
  PromptBreakdown,
  PromptBreakdownSection,
  TokenUsage,
} from "@/ipc/types";
import type { AgentUsageSnapshot } from "@/store/usageStore";

// ---------------------------------------------------------------------------
// 5-segment derivation per PRD art_18a2865ccbcd §2.2 / §5.2 and ARCH design
// art_f822814f9124 §7. The 5 PM-defined segments are:
//   SystemPrompt / ContextHistory / ToolDefinitions / ToolResults / Completions
// plus CacheBenefit as an overlay (not a segment).
//
// BE currently emits `cumulative` TokenUsage (8 fields) + `prompt_breakdown`
// sections per turn meta. ARCH §7.2 acknowledges providers don't split
// SystemPrompt / ToolDefinitions / ToolResults; we estimate them from the
// prompt_breakdown sections that ARE available, and leave ToolDefinitions /
// ToolResults at 0 when no signal exists (then auto-hide).
// ---------------------------------------------------------------------------

export type SegmentKind =
  | "system_prompt"
  | "context_history"
  | "tool_definitions"
  | "tool_results"
  | "completions";

export interface SegmentValue {
  kind: SegmentKind;
  label: string;
  detail: string;
  color: string;
  tokens: number;
}

const SEGMENT_META: Record<SegmentKind, { label: string; detail: string; color: string }> = {
  system_prompt: {
    label: "System Prompt",
    detail: "Agent persona & rules",
    color: "#ff737b",
  },
  context_history: {
    label: "Context History",
    detail: "Prior turns & messages",
    color: "#54afe8",
  },
  tool_definitions: {
    label: "Tool Definitions",
    detail: "Function & tool schemas",
    color: "#ff9d35",
  },
  tool_results: {
    label: "Tool Results",
    detail: "Tool call outputs",
    color: "#72c76a",
  },
  completions: {
    label: "Completions",
    detail: "Model output tokens",
    color: "#a981e6",
  },
};

const SYSTEM_PROMPT_KEYS = new Set([
  "agent_instructions",
  "actor_context",
  "runtime_context",
  "scope_bootstrap",
  "bootstrap_memory",
  "profile_prompt_files",
  "trigger_prefix",
]);

const CONTEXT_HISTORY_KEYS = new Set([
  "latest_message",
  "user_message",
  "assignment_context",
  "turn_input",
  "turn_memory",
]);

const TOOL_DEFINITIONS_KEYS = new Set(["tool_definitions", "tools_schema"]);
const TOOL_RESULTS_KEYS = new Set(["tool_results", "tool_outputs"]);

function sumSections(
  sections: PromptBreakdownSection[],
  keys: Set<string>,
): number {
  let total = 0;
  for (const section of sections) {
    if (keys.has(section.key)) {
      total += Math.max(0, Math.floor(section.approx_token_count ?? 0));
    }
  }
  return total;
}

function n(value: number | null | undefined): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
}

/**
 * Derive the 5-segment breakdown from a cumulative usage snapshot plus the
 * prompt_breakdown section list for the latest turn.
 *
 * Returns segments with zero tokens omitted, plus the computed total. When the
 * snapshot has no usage data at all, returns an empty array (caller renders
 * an "unavailable" state per PRD §4.2 anomaly rule).
 */
export function deriveSegments(
  cumulative: TokenUsage,
  promptBreakdown: PromptBreakdown | null | undefined,
): { segments: SegmentValue[]; total: number } {
  const sections = promptBreakdown?.sections ?? [];

  const systemPromptTokens = sumSections(sections, SYSTEM_PROMPT_KEYS);
  const contextHistorySections = sumSections(sections, CONTEXT_HISTORY_KEYS);
  const toolDefinitionsTokens = sumSections(sections, TOOL_DEFINITIONS_KEYS);
  const toolResultsTokens = sumSections(sections, TOOL_RESULTS_KEYS);
  const completionsTokens = n(cumulative.output_tokens);

  const inputTokens = n(cumulative.input_tokens);
  const cacheRead = n(cumulative.cache_read_input_tokens);
  // ContextHistory = max(prompt_breakdown context sections,
  //   input_tokens - system_prompt - cache_read).  This keeps the segment
  //   meaningful even when the prompt_breakdown has no explicit context entry.
  const contextHistoryFromInput = Math.max(
    0,
    inputTokens - systemPromptTokens - cacheRead,
  );
  const contextHistoryTokens = Math.max(contextHistorySections, contextHistoryFromInput);

  const raw: Array<[SegmentKind, number]> = [
    ["system_prompt", systemPromptTokens],
    ["context_history", contextHistoryTokens],
    ["tool_definitions", toolDefinitionsTokens],
    ["tool_results", toolResultsTokens],
    ["completions", completionsTokens],
  ];

  const segments: SegmentValue[] = [];
  let total = 0;
  for (const [kind, tokens] of raw) {
    if (tokens <= 0) continue;
    const meta = SEGMENT_META[kind];
    segments.push({ kind, label: meta.label, detail: meta.detail, color: meta.color, tokens });
    total += tokens;
  }
  return { segments, total };
}

function formatCompact(value: number): string {
  if (value >= 1000) return `${(value / 1000).toFixed(1)}K`;
  return `${Math.round(value)}`;
}

function withTildeIfEstimated(label: string, estimated: boolean): string {
  if (!estimated) return label;
  return label.startsWith("~") ? label : `~${label}`;
}

/**
 * Convert a SegmentValue to the AgentIdentityBadgeSegment shape (with width
 * percentage relative to the segment total). Width is rounded so the visual
 * bar reflects proportion, not absolute token count.
 */
export function segmentValuesToBadgeSegments(
  values: SegmentValue[],
  total: number,
  estimated: boolean,
): AgentIdentityBadgeSegment[] {
  if (total <= 0) return [];
  return values.map((value) => {
    const percent = (value.tokens / total) * 100;
    const tokenLabel = withTildeIfEstimated(formatCompact(value.tokens), estimated);
    return {
      id: value.kind,
      label: value.label,
      detail: value.detail,
      color: value.color,
      width: Math.max(2, Math.round(percent)),
      value: tokenLabel,
      percent: `${Math.round(percent)}%`,
    };
  });
}

/**
 * Compute the dynamic display labels from a usage snapshot. Returns `null`
 * when there is no usable data (caller should show the static placeholder /
 * "unavailable" state). All numeric labels carry a `~` prefix when the
 * snapshot is marked `estimated: true` (PRD AC-4 / AC-8).
 */
export interface UsageDisplay {
  segments: AgentIdentityBadgeSegment[];
  usedTokensLabel: string;
  remainingLabel: string;
  tokensLeftLabel: string;
  inputOutputLabel: string;
  /** Hidden when undefined (PRD AC-7). */
  costLabel?: string;
  /** Hidden when undefined (PRD AC-8 — cache_read missing). */
  cacheBenefitLabel?: string;
  estimated: boolean;
}

export function buildUsageDisplay(
  snapshot: AgentUsageSnapshot | null | undefined,
  contextWindow: number | null | undefined,
): UsageDisplay | null {
  if (!snapshot) return null;
  const { cumulative, promptBreakdown } = snapshot;
  const estimated = cumulative.estimated === true;
  const { segments: segmentValues, total: segmentTotal } = deriveSegments(
    cumulative,
    promptBreakdown,
  );

  const totalTokens = n(cumulative.total_tokens) ||
    n(cumulative.input_tokens) + n(cumulative.output_tokens);

  if (totalTokens <= 0 && segmentValues.length === 0) {
    return null;
  }

  const window =
    typeof contextWindow === "number" && contextWindow > 0 ? contextWindow : null;

  const usedCompact = withTildeIfEstimated(formatCompact(totalTokens), estimated);
  const usedTokensLabel = window
    ? `${usedCompact} / ${formatCompact(window)} tokens`
    : `${usedCompact} tokens`;

  let remainingLabel = "";
  let tokensLeftLabel = "";
  if (window) {
    const remaining = Math.max(0, window - totalTokens);
    const pct = Math.max(0, Math.round((remaining / window) * 100));
    remainingLabel = `${pct}%`;
    tokensLeftLabel = `${withTildeIfEstimated(formatCompact(remaining), estimated)} tokens left`;
  } else {
    remainingLabel = "";
    tokensLeftLabel = "";
  }

  const inputTokens = n(cumulative.input_tokens);
  const outputTokens = n(cumulative.output_tokens);
  const inputOutputLabel = `Input ${withTildeIfEstimated(
    formatCompact(inputTokens),
    estimated,
  )} · Output ${withTildeIfEstimated(formatCompact(outputTokens), estimated)}`;

  let costLabel: string | undefined;
  const cost = cumulative.total_cost_usd;
  if (typeof cost === "number" && Number.isFinite(cost)) {
    const formatted = cost >= 1 ? cost.toFixed(2) : cost.toFixed(4);
    costLabel = withTildeIfEstimated(`$${formatted}`, estimated);
  }

  let cacheBenefitLabel: string | undefined;
  const cacheRead = cumulative.cache_read_input_tokens;
  if (typeof cacheRead === "number" && cacheRead > 0) {
    cacheBenefitLabel = `Cache saved ${withTildeIfEstimated(
      formatCompact(cacheRead),
      estimated,
    )} tokens`;
  }

  return {
    segments: segmentValuesToBadgeSegments(segmentValues, segmentTotal, estimated),
    usedTokensLabel,
    remainingLabel,
    tokensLeftLabel,
    inputOutputLabel,
    costLabel,
    cacheBenefitLabel,
    estimated,
  };
}
