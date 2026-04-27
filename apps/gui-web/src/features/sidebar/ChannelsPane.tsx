import { useEffect, useState } from "react";
import clsx from "clsx";
import {
  ChevronDown,
  ChevronRight,
  Lock,
  MoreHorizontal,
  Plus,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Channel, Thread } from "@/ipc/types";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { openScope } from "@/features/chat/scopeActions";
import { ScopeIcon } from "@/features/common/ScopeIcon";
import {
  openCreateChannel,
  openCreateThread,
  openDeleteChannel,
  openDeleteThread,
  openInviteToChannel,
  openRenameChannel,
  openRenameThread,
} from "./channelActions";

export function ChannelsPane() {
  const channels = useChannels((s) => s.channels);
  const workspaceName = useSession((s) => s.workspace?.name);
  const toggleMembers = useUI((s) => s.toggleMembers);
  const view = useUI((s) => s.view);
  const setView = useUI((s) => s.setView);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  const toggleInbox = () =>
    setView(view === "inbox" ? "chat" : "inbox");

  return (
    <aside className="flex h-full w-60 shrink-0 flex-col overflow-hidden border-r border-border bg-sidebar">
      <header className="flex h-12 items-center justify-between border-b border-border/60 px-3">
        <h2 className="truncate text-sm font-semibold text-primary">
          {workspaceName ?? "Workspace"}
        </h2>
        <button
          aria-label="New channel"
          title="New channel"
          className="flex h-7 w-7 items-center justify-center rounded text-secondary hover:bg-hover hover:text-primary"
          onClick={openCreateChannel}
        >
          <Plus size={16} />
        </button>
      </header>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-scroll py-2">
        <div className="group flex items-center gap-1 px-2 pb-1">
          <div className="flex flex-1 items-center gap-1 text-[11px] font-semibold uppercase tracking-wider text-muted">
            <ChevronDown size={12} />
            Channels
          </div>
          <button
            aria-label="New channel"
            title="New channel"
            className="flex h-5 w-5 items-center justify-center rounded text-muted opacity-0 hover:bg-hover hover:text-primary group-hover:opacity-100"
            onClick={openCreateChannel}
          >
            <Plus size={13} />
          </button>
        </div>
        {channels.length === 0 ? (
          <div className="px-4 py-2 text-xs text-muted">no channels yet</div>
        ) : (
          channels.map((c) => (
            <ChannelRow
              key={c.id}
              channel={c}
              expanded={!!expanded[c.id]}
              onToggle={() =>
                setExpanded((s) => ({ ...s, [c.id]: !s[c.id] }))
              }
            />
          ))
        )}
      </div>

      <footer className="flex h-12 items-center gap-2 border-t border-border/60 bg-elevated px-3 text-xs text-secondary">
        <button
          className="hover:text-primary"
          onClick={() => {
            // Members only render on the chat view — if the user is in
            // Inbox and clicks Members, land them back on chat first so
            // the toggle has somewhere to draw.
            if (view !== "chat") setView("chat");
            toggleMembers();
          }}
        >
          Members
        </button>
        <span className="text-muted">·</span>
        <button
          className={clsx(
            "hover:text-primary",
            view === "inbox" && "text-warning",
          )}
          onClick={toggleInbox}
        >
          Inbox
        </button>
      </footer>
    </aside>
  );
}

function ChannelRow({
  channel,
  expanded,
  onToggle,
}: {
  channel: Channel;
  expanded: boolean;
  onToggle: () => void;
}) {
  const currentScope = useChannels((s) => s.currentScope);
  const threads = useChannels((s) => s.threadsByChannel[channel.id] ?? []);
  const replaceThreads = useChannels((s) => s.replaceThreads);
  const openContextMenu = useUI((s) => s.openContextMenu);
  const pushToast = useUI((s) => s.pushToast);

  // Narrow selector: only re-render this row when the pending-action count
  // for THIS channel changes. Thread rows subscribe to their own count
  // below, so a new action.request in channel B does not rerender channel A.
  const channelBadge = useMessages(
    (s) => s.byScope[`channel:${channel.id}`]?.pendingActionIds.size ?? 0,
  );

  const isCurrent = (kind: "channel" | "thread", id: string) =>
    currentScope?.kind === kind && currentScope.id === id;

  const hasCurrentThread = threads.some((t) => isCurrent("thread", t.id));
  const showThreads = expanded || hasCurrentThread;

  useEffect(() => {
    if (!showThreads) return;
    if (threads.length > 0) return;
    (async () => {
      try {
        const r = await ipc.threadList(channel.id);
        replaceThreads(channel.id, r.threads);
      } catch {
        /* ignore — toast surfaced at higher layer if it matters */
      }
    })();
  }, [showThreads, threads.length, channel.id, replaceThreads]);

  const copyId = async (kind: "channel" | "thread", id: string) => {
    try {
      await navigator.clipboard.writeText(id);
      pushToast("info", `${kind} id copied`);
    } catch {
      pushToast("info", id);
    }
  };

  const openChannelMenu = (x: number, y: number) => {
    openContextMenu({
      x,
      y,
      items: [
        {
          kind: "item",
          label: "New thread…",
          onClick: () => openCreateThread(channel.id),
        },
        {
          kind: "item",
          label: "Invite actor…",
          onClick: () => openInviteToChannel(channel),
        },
        {
          kind: "item",
          label: "Copy channel id",
          onClick: () => void copyId("channel", channel.id),
        },
        { kind: "divider" },
        {
          kind: "item",
          label: "Rename…",
          onClick: () => openRenameChannel(channel),
        },
        {
          kind: "item",
          label: "Delete…",
          danger: true,
          onClick: () => openDeleteChannel(channel),
        },
      ],
    });
  };

  const channelContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    openChannelMenu(e.clientX, e.clientY);
  };

  return (
    <div className="mb-0.5 px-2">
      <div
        className={clsx(
          "group/channel flex h-8 w-full items-center gap-1 rounded px-1 text-sm transition-colors",
          isCurrent("channel", channel.id)
            ? "bg-active text-primary"
            : "text-secondary hover:bg-hover hover:text-primary",
        )}
        onContextMenu={channelContextMenu}
      >
        <button
          type="button"
          aria-label={showThreads ? "Collapse threads" : "Expand threads"}
          className="flex h-6 w-5 shrink-0 items-center justify-center rounded text-muted hover:bg-active hover:text-secondary"
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
        >
          {showThreads ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
        </button>
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
          onClick={() => void openScope({ kind: "channel", id: channel.id })}
        >
          {channel.visibility === "private" ? (
            <Lock size={16} className="h-4 w-4 shrink-0 text-muted" />
          ) : (
            <ScopeIcon kind="channel" className="text-muted" />
          )}
          <span className="truncate">{channel.title}</span>
          {channel.visibility === "public" && (
            <span className="ml-1 rounded bg-elevated px-1 py-px text-[10px] uppercase leading-none text-muted">
              Public
            </span>
          )}
        </button>
        {channelBadge > 0 && (
          <span className="min-w-[18px] shrink-0 rounded-full bg-danger px-1 text-center text-[10px] leading-[18px] text-white">
            {channelBadge}
          </span>
        )}
        <button
          type="button"
          aria-label="New thread"
          title="New thread"
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted opacity-0 hover:bg-active hover:text-primary group-hover/channel:opacity-100"
          onClick={(e) => {
            e.stopPropagation();
            openCreateThread(channel.id);
          }}
        >
          <Plus size={14} />
        </button>
        <button
          type="button"
          aria-label="Channel actions"
          title="Channel actions"
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted opacity-0 hover:bg-active hover:text-primary group-hover/channel:opacity-100"
          onClick={(e) => {
            e.stopPropagation();
            openChannelMenu(e.clientX, e.clientY);
          }}
        >
          <MoreHorizontal size={14} />
        </button>
      </div>

      {showThreads && (
        <div className="ml-[1.15rem] mt-0.5 border-l border-border/80 pl-2">
          {threads.length === 0 ? (
            <div className="py-1 pl-2 text-xs text-muted">no threads</div>
          ) : (
            threads.map((t) => (
              <ThreadRow
                key={t.id}
                thread={t}
                current={isCurrent("thread", t.id)}
                onClick={() => void openScope({ kind: "thread", id: t.id })}
                onMenu={(x, y) =>
                  openContextMenu({
                    x,
                    y,
                    items: [
                      {
                        kind: "item",
                        label: "Rename…",
                        onClick: () => openRenameThread(t),
                      },
                      {
                        kind: "item",
                        label: "Delete…",
                        danger: true,
                        onClick: () => openDeleteThread(t),
                      },
                      { kind: "divider" },
                      {
                        kind: "item",
                        label: "Copy thread id",
                        onClick: () => void copyId("thread", t.id),
                      },
                    ],
                  })
                }
              />
            ))
          )}
          <button
            className="mt-0.5 flex h-7 w-full items-center gap-1.5 rounded px-2 text-xs text-muted hover:bg-hover hover:text-secondary"
            onClick={() => openCreateThread(channel.id)}
          >
            <Plus size={12} /> Create thread
          </button>
        </div>
      )}

    </div>
  );
}

function ThreadRow({
  thread,
  current,
  onClick,
  onMenu,
}: {
  thread: Thread;
  current: boolean;
  onClick: () => void;
  onMenu: (x: number, y: number) => void;
}) {
  const badge = useMessages(
    (s) => s.byScope[`thread:${thread.id}`]?.pendingActionIds.size ?? 0,
  );
  return (
    <div
      className={clsx(
        "group/thread flex h-7 w-full items-center gap-1.5 rounded px-2 text-sm transition-colors",
        current
          ? "bg-active text-primary"
          : "text-secondary hover:bg-hover hover:text-primary",
      )}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(e.clientX, e.clientY);
      }}
    >
      <button
        type="button"
        className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
        onClick={onClick}
      >
        <ScopeIcon kind="thread" className="text-muted" />
        <span className="flex-1 truncate text-left">{thread.title}</span>
      </button>
      {badge > 0 && (
        <span className="min-w-[18px] rounded-full bg-danger px-1 text-center text-[10px] leading-[18px] text-white">
          {badge}
        </span>
      )}
      <button
        type="button"
        aria-label="Thread actions"
        title="Thread actions"
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-muted opacity-0 transition-opacity hover:bg-active hover:text-primary group-hover/thread:opacity-100"
        onClick={(e) => {
          e.stopPropagation();
          onMenu(e.clientX, e.clientY);
        }}
      >
        <MoreHorizontal size={13} />
      </button>
    </div>
  );
}
