import type { ComponentType } from "react";
import { Check, Hash, Split, Users } from "lucide-react";
import type { Channel } from "@/ipc/types";
import type { ChannelPanelTab } from "@/lib/types";
import type { ConnectionState } from "@/lib/types";
import { channelTopic } from "@/lib/channel-utils";
import { connectionLabel } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";

export function ChatHeader({
  channel,
  target,
  connection,
  activePanel,
  onOpenPanel,
}: {
  channel: Channel | null;
  target: string | null;
  connection: ConnectionState;
  activePanel: ChannelPanelTab | null;
  onOpenPanel: (panel: ChannelPanelTab) => void;
}) {
  const topic = channelTopic(channel);
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
          {topic || target || connectionLabel(connection)}
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
