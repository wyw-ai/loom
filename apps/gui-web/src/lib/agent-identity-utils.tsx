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
import type { ActorRunContext } from "@/lib/agent-utils";

export function agentIdentityBadgeProps(
  entry: AgentMemberEntry,
  runContext?: ActorRunContext | null,
): AgentIdentityBadgeProps {
  const provider = providerForAgent(entry.machine, entry.agent);
  const providerName = provider?.name || provider?.id || "AI Runtime";
  const iconKey = agentProviderIconKey(provider?.id, providerName);
  const working = runContext != null && !runContext.isTerminal;
  const workingLabel = runContext ? runStatusFullLabel(runContext) : undefined;
  const runStatus = runContext?.status ?? null;
  return {
    avatarUrl: agentAvatarValue(entry.agent),
    agentName: agentDisplayName(entry.agent),
    providerName,
    providerMark: providerMark(providerName),
    providerIcon: iconKey ? <AgentProviderIcon iconKey={iconKey} /> : undefined,
    modelName: agentModelLabel(entry.agent, provider),
    usedTokensLabel: agentContextUsedLabel(entry.agent),
    remainingLabel: agentContextRemainingLabel(entry.agent),
    online: isOnlinePresenceStatus(entry.agent.status, entry.agent),
    working,
    workingLabel,
    runStatus,
    isStale: runContext?.isStale ?? false,
    isTerminal: runContext?.isTerminal ?? false,
  };
}
