import { useCallback, useEffect, useState, type ComponentType } from "react";
import {
  Check,
  FolderOpen,
  Hash,
  Search,
  Settings,
  Split,
  Users,
} from "lucide-react";
import type { Actor, Channel, ChannelVisibility } from "@/ipc/types";
import type { ChannelPanelTab } from "@/lib/types";
import type { ConnectionState } from "@/lib/types";
import { channelTopic, isDirectChannel } from "@/lib/channel-utils";
import { connectionLabel } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ScopeTokenSummary } from "@/components/layout/ScopeTokenSummary";
import { ChannelSettingsDialog } from "@/components/panels/ChannelSettingsDialog";
import { useI18n } from "@/lib/i18n";

export function ChatHeader({
  channel,
  target,
  connection,
  activePanel,
  onOpenPanel,
  searchOpen = false,
  onToggleSearch,
  scopeId,
  actors,
  onOpenFolder,
  onUpdateVisibility,
  currentActorId,
}: {
  channel: Channel | null;
  target: string | null;
  connection: ConnectionState;
  activePanel: ChannelPanelTab | null;
  onOpenPanel: (panel: ChannelPanelTab) => void;
  searchOpen?: boolean;
  onToggleSearch?: () => void;
  scopeId?: string | null;
  actors?: Record<string, Actor>;
  onOpenFolder?: () => void;
  onUpdateVisibility?: (
    channel: Channel,
    visibility: ChannelVisibility,
  ) => Promise<boolean>;
  currentActorId?: string | null;
}) {
  const { t } = useI18n();
  const topic = channelTopic(channel);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);

  useEffect(() => {
    setSettingsOpen(false);
  }, [channel?.id]);
  const panelActions: Array<{
    id: ChannelPanelTab;
    title: string;
    icon: ComponentType<{ size?: string | number; className?: string }>;
  }> = [
    { id: "threads", title: t("Threads"), icon: Split },
    { id: "members", title: t("Members"), icon: Users },
    { id: "tasks", title: t("Tasks"), icon: Check },
  ];
  return (
    <header className="flex h-[86px] shrink-0 items-center gap-4 border-b border-[#e2e6ef] bg-white px-6">
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-3">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center text-[#303849]">
            <Hash size={26} />
          </span>
          <h1 className="min-w-0 truncate text-[22px] font-bold leading-tight text-[#111827]">
            {channel ? channel.title : t("Space")}
          </h1>
        </div>
        <div className="mt-1 flex min-w-0 items-center gap-2 pl-11 text-sm text-[#485063]">
          <span className="truncate">{topic || target || t(connectionLabel(connection))}</span>
        </div>
      </div>
      {/* L1/L2 scope token summary — silent-hidden when null */}
      <ScopeTokenSummary scopeId={scopeId} actors={actors ?? {}} />
      <div className="flex shrink-0 items-center gap-1.5">
        {onToggleSearch && (
          <Button
            variant="outline"
            size="icon"
            title={t("Search messages")}
            aria-label={t("Search messages")}
            aria-pressed={searchOpen}
            onClick={onToggleSearch}
            className={cn(
              "relative h-9 w-9 shrink-0 rounded-lg",
              searchOpen && "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]",
            )}
          >
            <Search size={15} />
          </Button>
        )}
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
            title={t("View channel attachments")}
            aria-label={t("View channel attachments")}
            disabled={!channel}
            onClick={onOpenFolder}
            className="relative h-9 w-9 shrink-0 rounded-lg"
          >
            <FolderOpen size={15} />
          </Button>
        )}
        {channel && !isDirectChannel(channel) && (
          <>
            <Button
              variant="outline"
              size="icon"
              title={t("Channel settings")}
              aria-label={t("Channel settings")}
              aria-haspopup="dialog"
              aria-expanded={settingsOpen}
              onClick={() => setSettingsOpen(true)}
              className={cn(
                "relative h-9 w-9 shrink-0 rounded-lg",
                settingsOpen && "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]",
              )}
            >
              <Settings size={15} />
            </Button>
            {settingsOpen && (
              <ChannelSettingsDialog
                channel={channel}
                connectionOpen={connection === "open"}
                currentActorId={currentActorId}
                onClose={closeSettings}
                onUpdateVisibility={onUpdateVisibility}
              />
            )}
          </>
        )}
      </div>
    </header>
  );
}
