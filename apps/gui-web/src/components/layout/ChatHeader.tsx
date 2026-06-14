import type { ComponentType } from "react";
import { Check, Hash, Split, Users } from "lucide-react";
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

function formatActivityLabel(activity: ChannelAgentActivity): string {
  const { primaryAgentName, primaryStatus, activeCount, hasFailed } = activity;
  if (hasFailed && activeCount === 0) return `⚠ ${primaryAgentName} 运行异常`;
  if (activeCount === 1) {
    switch (primaryStatus) {
      case "running": return `${primaryAgentName} 正在思考…`;
      case "waiting_tool": return `${primaryAgentName} 正在等待工具…`;
      case "preparing_context": return `${primaryAgentName} 正在准备…`;
      case "queued": return `${primaryAgentName} 排队中…`;
      default: return `${primaryAgentName} 工作中…`;
    }
  }
  return `${primaryAgentName} 等 ${activeCount} 个 Agent 工作中…`;
}

export function ChatHeader({
  channel,
  target,
  connection,
  activePanel,
  onOpenPanel,
  runs,
  agentActors,
}: {
  channel: Channel | null;
  target: string | null;
  connection: ConnectionState;
  activePanel: ChannelPanelTab | null;
  onOpenPanel: (panel: ChannelPanelTab) => void;
  runs: Record<string, Run>;
  agentActors: Actor[];
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
      </div>
    </header>
  );
}
