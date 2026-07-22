import type { ComponentType } from "react";
import { Check, FolderOpen, Hash, Settings, Split, Users } from "lucide-react";
import type { Actor, Channel, Run } from "@/ipc/types";
import type { ChannelPanelTab } from "@/lib/types";
import type { ConnectionState } from "@/lib/types";
import { channelTopic } from "@/lib/channel-utils";
import { getChannelAgentActivity } from "@/lib/agent-utils";
import type { ChannelAgentActivity } from "@/lib/agent-utils";
import { useMemo } from "react";
import { connectionLabel } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ScopeTokenSummary } from "@/components/layout/ScopeTokenSummary";

function formatActivityLabel(activity: ChannelAgentActivity): string {
  const { primaryAgentName, primaryStatus, activeCount, hasFailed } = activity;
  if (hasFailed && activeCount === 0) return `⚠ ${primaryAgentName} encountered an error`;
  if (activeCount === 1) {
    switch (primaryStatus) {
      case "running": return `${primaryAgentName} is thinking…`;
      case "waiting_tool": return `${primaryAgentName} is running a tool…`;
      case "preparing_context": return `${primaryAgentName} is preparing context…`;
      case "queued": return `${primaryAgentName} is queued…`;
      default: return `${primaryAgentName} is working…`;
    }
  }
  return `${primaryAgentName} and ${activeCount} agents working…`;
}

export function ChatHeader({
  channel,
  target,
  connection,
  activePanel,
  onOpenPanel,
  runs,
  agentActors,
  scopeId,
  actors,
  onOpenFolder,
}: {
  channel: Channel | null;
  target: string | null;
  connection: ConnectionState;
  activePanel: ChannelPanelTab | null;
  onOpenPanel: (panel: ChannelPanelTab) => void;
  runs: Record<string, Run>;
  agentActors: Actor[];
  scopeId?: string | null;
  actors?: Record<string, Actor>;
  onOpenFolder?: () => void;
}) {
  const topic = channelTopic(channel);
  const activity = useMemo(
    () => getChannelAgentActivity(runs, agentActors),
    [runs, agentActors],
  );
  const panelActions: Array<{
    id: ChannelPanelTab;
    title: string;
    icon: ComponentType<{ size?: string | number; className?: string }>;
  }> = [
    { id: "threads", title: "Threads", icon: Split },
    { id: "members", title: "Members", icon: Users },
    { id: "tasks", title: "Tasks", icon: Check },
    { id: "configure", title: "Configure", icon: Settings },
  ];
  return (
    <header className="flex h-[86px] shrink-0 items-center gap-4 border-b border-[#e2e6ef] bg-white px-6">
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-3">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center text-[#303849]">
            <Hash size={26} />
          </span>
          <h1 className="min-w-0 truncate text-[22px] font-bold leading-tight text-[#111827]">
            {channel ? channel.title : "Space"}
          </h1>
        </div>
        <div className="mt-1 truncate pl-11 text-sm text-[#485063]">
          {activity && !activity.isIdle ? (
            <span className={cn(
              activity.hasFailed && activity.activeCount === 0 && "text-red-500",
              activity.primaryStatus === "running" && "text-purple-500 chat-header-activity-running",
              activity.primaryStatus === "waiting_tool" && "text-orange-500",
            )}>
              {formatActivityLabel(activity)}
            </span>
          ) : (
            topic || target || connectionLabel(connection)
          )}
        </div>
      </div>
      {/* L1/L2 scope token summary — silent-hidden when null */}
      <ScopeTokenSummary scopeId={scopeId} actors={actors ?? {}} />
      <div className="flex shrink-0 items-center gap-1.5">
        {panelActions.map((item) => {
          const Icon = item.icon;
          const selected = activePanel === item.id;
          return (
            <Button
              key={item.id}
              variant="outline"
              size="icon"
              title={item.title}
              aria-label={item.title}
              aria-pressed={selected}
              disabled={!channel}
              onClick={() => onOpenPanel(item.id)}
              className={cn(
                "relative h-9 w-9 shrink-0 rounded-lg",
                selected && "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]",
              )}
            >
              <Icon size={15} />
            </Button>
          );
        })}
        {onOpenFolder && (
          <Button
            variant="outline"
            size="icon"
            title="View channel attachments"
            aria-label="View channel attachments"
            disabled={!channel}
            onClick={onOpenFolder}
            className="relative h-9 w-9 shrink-0 rounded-lg"
          >
            <FolderOpen size={15} />
          </Button>
        )}
      </div>
    </header>
  );
}
