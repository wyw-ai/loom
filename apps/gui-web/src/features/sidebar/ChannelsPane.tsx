import { useEffect, useState, type MouseEvent } from "react";
import clsx from "clsx";
import {
  Archive,
  Bookmark,
  ChevronDown,
  ChevronRight,
  Hash,
  Inbox,
  type LucideIcon,
  MoreHorizontal,
  Plus,
  Search,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Channel, Thread } from "@/ipc/types";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useUI } from "@/store/ui";
import { openScope } from "@/features/chat/scopeActions";
import {
  openCreateChannel,
  openCreateThread,
  openDeleteChannel,
  openDeleteThread,
  openInviteToChannel,
  openRenameChannel,
  openRenameThread,
  archiveThread,
  restoreThread,
} from "./channelActions";

export function ChannelsPane() {
  const channels = useChannels((s) => s.channels);
  const view = useUI((s) => s.view);
  const setView = useUI((s) => s.setView);
  const openModal = useUI((s) => s.openModal);
  const [expanded, setExpanded] = useState<Record<string, boolean | undefined>>(
    {},
  );

  const title = view === "tasks" ? "Tasks" : view === "inbox" ? "Inbox" : "Chat";

  return (
    <aside className="hidden h-full min-h-0 w-60 shrink-0 select-none flex-col overflow-hidden border-r-2 border-black bg-brutal-cream text-black md:flex">
      <header className="flex h-panel-header shrink-0 items-center border-b-2 border-black px-5">
        <div className="text-lg font-black">{title}</div>
      </header>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto px-2 py-3">
        <SideButton icon={Search} label="Search" suffix="⌘K" onClick={() => openModal({ type: "quickSwitch" })} />
        <SideButton icon={Inbox} label="Inbox" onClick={() => setView("inbox")} />
        <SideButton icon={Bookmark} label="Saved" onClick={() => useUI.getState().pushToast("info", "Saved view is not backed by the current Joi protocol yet")} />

        <SectionHeader label="Channels" count={channels.length} onAdd={openCreateChannel} />
        {channels.length === 0 ? (
          <div className="border-2 border-dashed border-black/25 px-3 py-4 text-sm font-mono text-black/40">
            no channels yet
          </div>
        ) : (
          channels.map((c) => (
            <ChannelRow
              key={c.id}
              channel={c}
              expanded={expanded[c.id]}
              onToggle={(nextExpanded) =>
                setExpanded((s) => ({ ...s, [c.id]: nextExpanded }))
              }
            />
          ))
        )}

        <SectionHeader label="Direct Messages" count={0} />
      </div>
      <div className="hidden h-2 cursor-col-resize border-t-2 border-black md:block" />
    </aside>
  );
}

function SideButton({
  icon: Icon,
  label,
  suffix,
  count,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  suffix?: string;
  count?: number;
  onClick?: () => void;
}) {
  return (
    <button
      className="mb-1 flex w-full items-center gap-1.5 border-2 border-transparent px-2 py-1.5 text-left text-sm font-bold transition-colors hover:border-black hover:bg-white hover:shadow-brutal-sm"
      onClick={onClick}
    >
      <Icon size={14} className="shrink-0" />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {suffix && <span className="font-mono text-xs text-black/40">{suffix}</span>}
      {typeof count === "number" && (
        <span className="font-mono text-[10px] text-black/40">{count}</span>
      )}
    </button>
  );
}

function SectionHeader({
  label,
  count,
  onAdd,
}: {
  label: string;
  count: number;
  onAdd?: () => void;
}) {
  return (
    <div className="mb-1 mt-3 flex items-center justify-between px-2">
      <button
        type="button"
        className="flex items-center gap-1 text-xs font-black uppercase tracking-widest text-black hover:text-black/70"
      >
        <ChevronDown size={12} />
        {label}
        <span className="font-mono text-black/40">{count}</span>
      </button>
      {onAdd && (
        <button
          className="btn-brutal-sm bg-white p-0.5"
          aria-label={`New ${label.toLowerCase()}`}
          title={`New ${label.toLowerCase()}`}
          onClick={onAdd}
        >
          <Plus size={14} />
        </button>
      )}
    </div>
  );
}

function ChannelRow({
  channel,
  expanded,
  onToggle,
}: {
  channel: Channel;
  expanded?: boolean;
  onToggle: (nextExpanded: boolean) => void;
}) {
  const currentScope = useChannels((s) => s.currentScope);
  const threads = useChannels((s) => s.threadsByChannel[channel.id] ?? []);
  const archivedThreads = useChannels(
    (s) => s.archivedThreadsByChannel[channel.id] ?? [],
  );
  const replaceThreads = useChannels((s) => s.replaceThreads);
  const replaceArchivedThreads = useChannels((s) => s.replaceArchivedThreads);
  const openContextMenu = useUI((s) => s.openContextMenu);
  const pushToast = useUI((s) => s.pushToast);
  const [threadsLoaded, setThreadsLoaded] = useState(false);
  const [archiveExpanded, setArchiveExpanded] = useState(false);
  const [archiveLoaded, setArchiveLoaded] = useState(false);
  const channelBadge = useMessages(
    (s) => s.byScope[`channel:${channel.id}`]?.pendingActionIds.size ?? 0,
  );

  const isCurrent = (kind: "channel" | "thread", id: string) =>
    currentScope?.kind === kind && currentScope.id === id;
  const hasCurrentThread = threads.some((t) => isCurrent("thread", t.id));
  const showThreads = expanded ?? hasCurrentThread;

  useEffect(() => {
    if (!showThreads || threadsLoaded) return;
    (async () => {
      try {
        const r = await ipc.threadList(channel.id);
        replaceThreads(channel.id, r.threads);
        setThreadsLoaded(true);
      } catch {
        /* higher-level flows surface errors where they matter */
      }
    })();
  }, [showThreads, threadsLoaded, threads.length, channel.id, replaceThreads]);

  useEffect(() => {
    if (!archiveExpanded || archiveLoaded) return;
    (async () => {
      try {
        const [active, archived] = await Promise.all([
          ipc.threadList(channel.id),
          ipc.threadList(channel.id, { archived: true }),
        ]);
        replaceThreads(channel.id, active.threads);
        replaceArchivedThreads(channel.id, archived.threads);
        setThreadsLoaded(true);
        setArchiveLoaded(true);
      } catch {
        /* higher-level flows surface errors where they matter */
      }
    })();
  }, [
    archiveExpanded,
    archiveLoaded,
    channel.id,
    replaceArchivedThreads,
    replaceThreads,
  ]);

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
          label: "New thread...",
          onClick: () => openCreateThread(channel.id),
        },
        {
          kind: "item",
          label: "Invite actor...",
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
          label: "Edit channel...",
          onClick: () => openRenameChannel(channel),
        },
        {
          kind: "item",
          label: "Delete...",
          danger: true,
          onClick: () => openDeleteChannel(channel),
        },
      ],
    });
  };

  const channelContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    openChannelMenu(e.clientX, e.clientY);
  };

  return (
    <div className="mb-1">
      <div
        className={clsx(
          "group/channel flex w-full items-center gap-1 border-2 px-1 py-1 text-sm font-bold transition-colors",
          isCurrent("channel", channel.id)
            ? "border-black bg-brutal-pink shadow-brutal-sm"
            : "border-transparent hover:border-black hover:bg-white hover:shadow-brutal-sm",
        )}
        onContextMenu={channelContextMenu}
      >
        <button
          type="button"
          aria-label={showThreads ? "Collapse threads" : "Expand threads"}
          className="flex h-[22px] w-[18px] shrink-0 items-center justify-center text-black/70 hover:text-black"
          onClick={(e) => {
            e.stopPropagation();
            onToggle(!showThreads);
          }}
        >
          {showThreads ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
        </button>
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
          onClick={() => void openScope({ kind: "channel", id: channel.id })}
        >
          <Hash size={15} className="shrink-0" />
          <span className="min-w-0 flex-1 truncate">{channel.title}</span>
        </button>
        {channelBadge > 0 && (
          <span className="min-w-[18px] border border-black bg-danger px-1 text-center text-[10px] leading-[16px] text-black">
            {channelBadge}
          </span>
        )}
        <button
          type="button"
          aria-label="Channel actions"
          title="Channel actions"
          className="flex h-6 w-6 shrink-0 items-center justify-center opacity-0 hover:bg-white group-hover/channel:opacity-100"
          onClick={(e) => {
            e.stopPropagation();
            openChannelMenu(e.clientX, e.clientY);
          }}
        >
          <MoreHorizontal size={14} />
        </button>
      </div>

      {showThreads && (
        <div className="ml-5 mt-1 border-l-2 border-black/20 pl-2">
          {threads.length === 0 ? (
            <div className="py-1 pl-2 text-xs font-mono text-black/40">
              no threads
            </div>
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
                        label: "Rename...",
                        onClick: () => openRenameThread(t),
                      },
                      {
                        kind: "item",
                        label: "Archive",
                        onClick: () => void archiveThread(t),
                      },
                      {
                        kind: "item",
                        label: "Delete...",
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
            className="mt-1 flex h-7 w-full items-center gap-1.5 border-2 border-transparent px-2 text-xs font-bold text-black/55 hover:border-black hover:bg-white hover:text-black"
            onClick={() => openCreateThread(channel.id)}
          >
            <Plus size={12} /> Create thread
          </button>
          <div className="mt-1">
            <button
              className="flex h-7 w-full items-center gap-1.5 border-2 border-transparent px-2 text-xs font-bold text-black/55 hover:border-black hover:bg-white hover:text-black"
              onClick={() => setArchiveExpanded((v) => !v)}
            >
              {archiveExpanded ? (
                <ChevronDown size={12} />
              ) : (
                <ChevronRight size={12} />
              )}
              <Archive size={12} />
              Archive Box
              <span className="font-mono text-[10px] text-black/35">
                {archivedThreads.length}
              </span>
            </button>
            {archiveExpanded && (
              <div className="ml-3 border-l-2 border-black/10 pl-2">
                {archivedThreads.length === 0 ? (
                  <div className="py-1 pl-2 text-xs font-mono text-black/35">
                    no archived threads
                  </div>
                ) : (
                  archivedThreads.map((t) => (
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
                              label: "Restore",
                              onClick: () => void restoreThread(t),
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
              </div>
            )}
          </div>
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
        "group/thread mb-1 flex h-7 w-full items-center gap-1 border-2 px-1 text-sm font-bold transition-colors",
        current
          ? "border-black bg-brutal-yellow shadow-brutal-sm"
          : "border-transparent hover:border-black hover:bg-white",
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
        <span className="text-black/50">↳</span>
        <span className="flex-1 truncate">{thread.title}</span>
      </button>
      {badge > 0 && (
        <span className="min-w-[18px] border border-black bg-danger px-1 text-center text-[10px] leading-[16px] text-black">
          {badge}
        </span>
      )}
      <button
        type="button"
        aria-label="Thread actions"
        title="Thread actions"
        className="flex h-5 w-5 shrink-0 items-center justify-center opacity-0 hover:bg-white group-hover/thread:opacity-100"
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
