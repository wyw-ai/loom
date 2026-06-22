import { AgentProviderIcon, agentProviderIconKey } from "@/components/agent/AgentProviderIcon";
import type { AgentIdentityBadgeProps } from "@/components/agent/AgentIdentityBadge";
import type { AgentMemberEntry } from "@/lib/types";
import {
  agentAvatarValue,
  agentDisplayName,
  agentModelLabel,
  agentContextUsedLabel,
  agentContextRemainingLabel,
  providerForAgent,
  providerMark,
  runStatusFullLabel,
} from "@/lib/agent-utils";
import {
  isOnlinePresenceStatus,
} from "@/lib/format-utils";
import { metadataNumber } from "@/lib/message-utils";
import type { ActorRunContext } from "@/lib/agent-utils";
import type { AgentUsageSnapshot } from "@/store/usageStore";
import { buildUsageDisplay } from "@/lib/agent-usage-display";

export function agentIdentityBadgeProps(
  entry: AgentMemberEntry,
  runContext?: ActorRunContext | null,
  usage?: AgentUsageSnapshot | null,
): AgentIdentityBadgeProps {
  const provider = providerForAgent(entry.machine, entry.agent);
  const providerName = provider?.name || provider?.id || "AI Runtime";
  const iconKey = agentProviderIconKey(provider?.id, providerName);
  const working = runContext != null && !runContext.isTerminal;
  const workingLabel = runContext ? runStatusFullLabel(runContext) : undefined;
  const runStatus = runContext?.status ?? null;
  const modelName = agentModelLabel(entry.agent, provider);

  // Iteration #4 — derive live token usage display from the per-actor
  // snapshot when available; fall back to the historical static labels so
  // existing behavior (agents that have not produced usage yet) is unchanged.
  const meta = entry.agent.spec._meta;
  const contextWindow =
    metadataNumber(meta, ["contextWindowTokens", "totalTokens", "maxTokens"]) ?? null;
  const display = buildUsageDisplay(usage, contextWindow);

  return {
    avatarUrl: agentAvatarValue(entry.agent),
    agentName: agentDisplayName(entry.agent),
    providerName,
    providerMark: providerMark(providerName),
    providerIcon: iconKey ? <AgentProviderIcon iconKey={iconKey} /> : undefined,
    modelName,
    usedTokensLabel: display?.usedTokensLabel ?? agentContextUsedLabel(entry.agent),
    remainingLabel: display?.remainingLabel ?? agentContextRemainingLabel(entry.agent),
    segments: display?.segments,
    inputOutputLabel: display?.inputOutputLabel,
    costLabel: display?.costLabel,
    cacheBenefitLabel: display?.cacheBenefitLabel,
    tokensLeftLabel: display?.tokensLeftLabel,
    estimated: display?.estimated ?? false,
    hasUsageData: display != null,
    online: isOnlinePresenceStatus(entry.agent.status, entry.agent),
    working,
    workingLabel,
    runStatus,
    isStale: runContext?.isStale ?? false,
    isTerminal: runContext?.isTerminal ?? false,
  };
}
