import { useEffect, useMemo } from "react";
import { Lock, Users } from "lucide-react";

import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useUI } from "@/store/ui";
import { scopeKey } from "@/ipc/types";
import { MessageList } from "./MessageList";
import { openScope } from "./scopeActions";
import { ScopeIcon } from "@/features/common/ScopeIcon";
import { Prompt } from "@/features/prompt/Prompt";
import { AnnouncementBanner } from "./AnnouncementBanner";
import { StreamingStatusBar } from "./StreamingStatusBar";

export function ChatView() {
  const scope = useChannels((s) => s.currentScope);
  const channels = useChannels((s) => s.channels);
  const threads = useChannels((s) => s.threadsByChannel);
  const ensureScope = useMessages((s) => s.ensureScope);
  const scopeStoreAll = useMessages((s) => s.byScope);
  const ui = useUI();

  useEffect(() => {
    if (scope) ensureScope(scope);
  }, [scope, ensureScope]);

  const header = useMemo(() => {
    if (!scope) return null;
    if (scope.kind === "channel") {
      const ch = channels.find((c) => c.id === scope.id);
      return {
        icon:
          ch?.visibility === "private" ? (
            <Lock size={16} className="text-muted" />
          ) : (
            <ScopeIcon kind="channel" className="text-muted" />
          ),
        title: ch?.title ?? scope.id,
        subtitle: "common area",
        parentChannel: null,
      };
    }
    let title = scope.id;
    let parentChannel:
      | { id: string; title: string; visibility: "public" | "private" }
      | null = null;
    for (const [chId, ts] of Object.entries(threads)) {
      const t = ts.find((x) => x.id === scope.id);
      if (t) {
        title = t.title;
        const ch = channels.find((c) => c.id === chId);
        if (ch) {
          parentChannel = {
            id: ch.id,
            title: ch.title,
            visibility: ch.visibility,
          };
        }
        break;
      }
    }
    return {
      icon: <ScopeIcon kind="thread" className="text-muted" />,
      title,
      subtitle: "",
      parentChannel,
    };
  }, [scope, channels, threads]);

  if (!scope) {
    return (
      <div className="flex h-full min-h-0 min-w-0 flex-col items-center justify-center gap-2 text-muted">
        <p className="text-lg">Welcome to Joi Desktop</p>
        <p className="text-sm">Pick a channel or thread from the sidebar.</p>
      </div>
    );
  }

  const scopeStore = scopeStoreAll[scopeKey(scope)];

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-4">
        {header?.icon}
        <div className="flex min-w-0 flex-1 items-baseline gap-2">
          <h1 className="truncate text-sm font-semibold text-primary">
            {header?.title}
          </h1>
          {header?.parentChannel && (
            <button
              type="button"
              className="flex min-w-0 items-center gap-1 text-xs text-muted hover:text-secondary"
              onClick={() =>
                void openScope({
                  kind: "channel",
                  id: header.parentChannel!.id,
                })
              }
            >
              <span>in</span>
              {header.parentChannel.visibility === "private" ? (
                <Lock size={12} className="h-3 w-3 shrink-0" />
              ) : (
                <ScopeIcon kind="channel" className="h-3 w-3 shrink-0" />
              )}
              <span className="truncate">{header.parentChannel.title}</span>
            </button>
          )}
          {header?.subtitle && (
            <span className="shrink-0 text-xs text-muted">
              · {header.subtitle}
            </span>
          )}
        </div>
        <button
          aria-label="Toggle members"
          title="Toggle members"
          className="flex h-8 w-8 items-center justify-center rounded text-secondary hover:bg-hover hover:text-primary"
          onClick={() => ui.toggleMembers()}
        >
          <Users size={16} />
        </button>
      </header>

      {scopeStore?.announcement && (
        <AnnouncementBanner announcement={scopeStore.announcement} />
      )}

      <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
        <MessageList scope={scope} />
      </div>

      <StreamingStatusBar scope={scope} />
      <Prompt scope={scope} />
    </div>
  );
}
