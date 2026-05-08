import { useEffect, useMemo, useState } from "react";
import clsx from "clsx";
import {
  Hash,
  ListTodo,
  type LucideIcon,
  MessageSquare,
  Plus,
  Settings,
  Square,
  Users,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { scopeKey } from "@/ipc/types";
import { MessageList } from "./MessageList";
import { openScope } from "./scopeActions";
import { Prompt } from "@/features/prompt/Prompt";
import { AnnouncementBanner } from "./AnnouncementBanner";
import { StreamingStatusBar } from "./StreamingStatusBar";
import { openRenameChannel } from "@/features/sidebar/channelActions";

type MainTab = "chat" | "tasks";
type TaskStatus = "todo" | "inProgress" | "inReview" | "done";

export function ChatView() {
  const scope = useChannels((s) => s.currentScope);
  const channels = useChannels((s) => s.channels);
  const threads = useChannels((s) => s.threadsByChannel);
  const membersByChannel = useChannels((s) => s.membersByChannel);
  const ensureScope = useMessages((s) => s.ensureScope);
  const scopeStoreAll = useMessages((s) => s.byScope);
  const ui = useUI();
  const [tab, setTab] = useState<MainTab>("chat");

  useEffect(() => {
    if (scope) ensureScope(scope);
  }, [scope, ensureScope]);

  useEffect(() => {
    setTab("chat");
  }, [scope?.kind, scope?.id]);

  const header = useMemo(() => {
    if (!scope) return null;
    if (scope.kind === "channel") {
      const ch = channels.find((c) => c.id === scope.id);
      return {
        kind: "channel" as const,
        title: ch?.title ?? scope.id,
        subtitle:
          ch?.title === "all"
            ? "General channel for all members"
            : ch?.visibility === "private"
              ? "Private channel"
              : "Channel workspace",
        parentChannel: null,
        channel: ch ?? null,
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
      kind: "thread" as const,
      title,
      subtitle: parentChannel ? `Thread in #${parentChannel.title}` : "Thread",
      parentChannel,
      channel: parentChannel
        ? channels.find((c) => c.id === parentChannel.id) ?? null
        : null,
    };
  }, [scope, channels, threads]);

  if (!scope) {
    return (
      <div className="flex h-full min-h-0 min-w-0 flex-col items-center justify-center bg-white text-lg font-black uppercase text-black/40">
        Select a channel
      </div>
    );
  }

  const scopeStore = scopeStoreAll[scopeKey(scope)];
  const memberCount =
    header?.channel?.members.length ??
    (header?.channel ? membersByChannel[header.channel.id]?.length : 0) ??
    0;

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-white text-black">
      <header className="flex h-panel-header shrink-0 items-center gap-3 border-b-2 border-black bg-white px-5">
        <div className="flex size-icon-header shrink-0 items-center justify-center border-2 border-black bg-brutal-yellow text-black">
          {header?.kind === "thread" ? (
            <MessageSquare size={18} />
          ) : (
            <Hash size={18} />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 truncate">
            <span className="shrink-0 text-base font-black leading-tight">
              {header?.title}
            </span>
            {header?.subtitle && (
              <span className="truncate text-sm leading-tight text-black/55">
                - {header.subtitle}
              </span>
            )}
          </div>
          {header?.parentChannel && (
            <button
              type="button"
              className="text-xs font-bold text-black/45 hover:text-black"
              onClick={() =>
                void openScope({
                  kind: "channel",
                  id: header.parentChannel!.id,
                })
              }
            >
              in #{header.parentChannel.title}
            </button>
          )}
        </div>
        <div className="flex items-center gap-1.5">
          <button
            className="btn-brutal-sm bg-white p-1.5"
            title="Stop all agents in this channel"
            onClick={() =>
              ui.openModal({
                type: "confirm",
                title: "Stop all agents in this channel?",
                body: "Agent processes are owned by `joi agent serve`; stop the host process that is serving the agents for this channel.",
                confirmLabel: "Got It",
                danger: true,
                onConfirm: () =>
                  ui.pushToast("warn", "agent process control belongs to joi agent serve"),
              })
            }
          >
            <Square size={14} />
          </button>
          {header?.channel && (
            <button
              className="btn-brutal-sm bg-white p-1.5"
              title="Edit channel"
              onClick={() => openRenameChannel(header.channel!)}
            >
              <Settings size={14} />
            </button>
          )}
          <button
            className="btn-brutal-sm gap-1 bg-white px-2 py-1.5 text-xs"
            title="View participants"
            onClick={() => ui.toggleMembers()}
          >
            <Users size={14} />
            {memberCount || 0}
          </button>
        </div>
      </header>

      <div className="flex shrink-0 overflow-x-auto border-b-2 border-black bg-white scrollbar-none">
        <TabButton
          active={tab === "chat"}
          icon={MessageSquare}
          label="Chat"
          onClick={() => setTab("chat")}
        />
        <TabButton
          active={tab === "tasks"}
          icon={ListTodo}
          label="Tasks"
          onClick={() => setTab("tasks")}
        />
      </div>

      {scopeStore?.announcement && tab === "chat" && (
        <AnnouncementBanner announcement={scopeStore.announcement} />
      )}

      {tab === "chat" ? (
        <>
          <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
            <MessageList scope={scope} />
          </div>
          <StreamingStatusBar scope={scope} />
          <Prompt scope={scope} />
        </>
      ) : (
        <ChannelTasksPanel />
      )}
    </div>
  );
}

function TabButton({
  active,
  icon: Icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: LucideIcon;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      className={clsx(
        "flex items-center gap-1.5 border-r-2 border-black px-4 py-1.5 text-xs font-black uppercase tracking-wide transition-colors",
        active ? "bg-brutal-yellow" : "bg-white hover:bg-black/5",
      )}
      onClick={onClick}
    >
      <Icon size={12} />
      {label}
    </button>
  );
}

function ChannelTasksPanel() {
  const [filter, setFilter] = useState<TaskStatus | "all">("all");
  const scope = useChannels((s) => s.currentScope);
  const selfId = useSession((s) => s.workspace?.actorId);
  const scopeStoreAll = useMessages((s) => s.byScope);
  const ui = useUI();
  const tasks = useMemo(() => {
    if (!scope) return [];
    const store = scopeStoreAll[scopeKey(scope)];
    return (store?.bubbles ?? [])
      .filter((bubble) => bubble.kind === "actionRequest")
      .map((bubble) => ({
        id: bubble.id,
        title: bubble.actionTitle || bubble.text.split("\n").find(Boolean) || "Untitled task",
        status:
          bubble.actionStatus === "accepted" || bubble.actionStatus === "declined"
            ? ("done" as TaskStatus)
            : ("todo" as TaskStatus),
      }))
      .filter((task) => filter === "all" || task.status === filter);
  }, [filter, scope, scopeStoreAll]);
  const filters: Array<[TaskStatus | "all", string]> = [
    ["all", "All"],
    ["todo", "Todo"],
    ["inProgress", "In Progress"],
    ["inReview", "In Review"],
    ["done", "Done"],
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden bg-white">
      <div className="flex items-center gap-2 border-b-2 border-black bg-white px-3 py-3">
        <div className="min-w-0 flex-1">
          <div
            role="radiogroup"
            aria-label="Filter tasks"
            className="inline-flex max-w-full flex-wrap items-center gap-1"
          >
            {filters.map(([value, label]) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={filter === value}
                className={clsx(
                  "flex h-7 min-w-0 shrink-0 items-center gap-1 border-2 border-black px-2 text-xs font-black transition-colors",
                  filter === value
                    ? "bg-brutal-yellow shadow-brutal-sm"
                    : "bg-white hover:bg-brutal-cream",
                )}
                onClick={() => setFilter(value)}
              >
                <span className="truncate">{label}</span>
              </button>
            ))}
          </div>
        </div>
        <button
          className="btn-brutal-sm flex h-7 shrink-0 items-center gap-1 whitespace-nowrap bg-brutal-pink px-2 text-xs"
          onClick={() =>
            ui.openModal({
              type: "taskCreate",
              onSubmit: async (titles) => {
                if (!scope || !selfId) {
                  ui.pushToast("error", "connect and select a channel first");
                  return;
                }
                await Promise.all(
                  titles.map((title) =>
                    ipc.eventAppend({
                      type: "action.request",
                      actorId: selfId,
                      scope,
                      payload: {
                        requestType: "task",
                        title,
                        description: title,
                        choices: [
                          { id: "done", label: "Done" },
                          { id: "cancel", label: "Cancel" },
                        ],
                      },
                      relations: [],
                    }),
                  ),
                );
                ui.pushToast("info", `${titles.length} task(s) created`);
              },
            })
          }
        >
          <Plus size={12} /> New Task
        </button>
      </div>
      <div className="stable-scrollbar flex-1 overflow-y-auto bg-white p-3">
        {tasks.length === 0 ? (
          <div className="px-3 py-8 text-center font-mono text-sm text-black/40">
            No tasks in this scope.
          </div>
        ) : (
          <div className="space-y-2">
            {tasks.map((task) => (
              <article
                key={task.id}
                className="border-2 border-black bg-brutal-cream p-3 shadow-brutal-sm"
              >
                <div className="mb-2 flex items-center gap-2 font-mono text-[11px] text-black/45">
                  <span>{task.id}</span>
                  <span>{task.status}</span>
                </div>
                <div className="text-sm font-black leading-6">{task.title}</div>
              </article>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
