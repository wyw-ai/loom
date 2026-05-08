import {
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import clsx from "clsx";
import {
  ChevronDown,
  Hash,
  List,
  type LucideIcon,
  Plus,
  SquareCheckBig,
  Trello,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { ScopeRef } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useInbox } from "@/store/inbox";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";

type Status = "todo" | "inProgress" | "inReview" | "done";

interface TaskItem {
  key: string;
  channel: string;
  title: string;
  creator: string;
  assignee: string;
  status: Status;
}

const statuses: Array<{
  id: Status;
  label: string;
  color: string;
}> = [
  { id: "todo", label: "Todo", color: "bg-brutal-orange" },
  { id: "inProgress", label: "In Progress", color: "bg-brutal-cyan" },
  { id: "inReview", label: "In Review", color: "bg-brutal-lavender" },
  { id: "done", label: "Done", color: "bg-brutal-lime" },
];

export function TasksPage() {
  const channels = useChannels((s) => s.channels);
  const currentScope = useChannels((s) => s.currentScope);
  const messageScopes = useMessages((s) => s.byScope);
  const inboxItems = useInbox((s) => s.items);
  const actors = useActors((s) => s.byId);
  const selfId = useSession((s) => s.workspace?.actorId);
  const openModal = useUI((s) => s.openModal);
  const pushToast = useUI((s) => s.pushToast);
  const [mode, setMode] = useState<"board" | "list">("board");
  const [hidden, setHidden] = useState<Record<Status, boolean>>({
    todo: false,
    inProgress: false,
    inReview: false,
    done: true,
  });

  const tasks = useMemo(
    () => deriveTasks(messageScopes, inboxItems, actors),
    [messageScopes, inboxItems, actors],
  );

  const createTasks = async (titles: string[]) => {
    if (!selfId) {
      pushToast("error", "connect a workspace before creating tasks");
      return;
    }
    if (!currentScope) {
      pushToast("error", "select a channel or thread before creating tasks");
      return;
    }
    await Promise.all(
      titles.map((title) =>
        ipc.eventAppend({
          type: "action.request",
          actorId: selfId,
          scope: currentScope,
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
    pushToast("info", `${titles.length} task(s) created`);
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-white text-black">
      <header className="flex h-panel-header shrink-0 items-center gap-3 border-b-2 border-black bg-white px-5">
        <div className="flex size-icon-header items-center justify-center border-2 border-black bg-brutal-yellow">
          <SquareCheckBig size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-base font-black leading-tight">Tasks</div>
          <div className="font-mono text-xs text-black/50">
            {tasks.length} task{tasks.length === 1 ? "" : "s"} from action requests
          </div>
        </div>
        <ModeButton
          active={mode === "board"}
          icon={Trello}
          label="Board"
          onClick={() => setMode("board")}
        />
        <ModeButton
          active={mode === "list"}
          icon={List}
          label="List"
          onClick={() => setMode("list")}
        />
      </header>

      <div className="flex items-center gap-2 border-b-2 border-black bg-white px-5 py-3">
        <button className="btn-brutal-sm gap-2 bg-white px-3 py-1.5 text-xs uppercase tracking-wider">
          <Hash size={14} />
          {scopeLabel(currentScope, channels)}
          <ChevronDown size={14} />
        </button>
        <button
          className="btn-brutal-sm ml-auto gap-1 bg-brutal-pink px-3 py-1.5 text-xs"
          onClick={() =>
            openModal({
              type: "taskCreate",
              onSubmit: createTasks,
            })
          }
        >
          <Plus size={13} /> New Task
        </button>
      </div>

      {mode === "board" ? (
        <Board tasks={tasks} hidden={hidden} setHidden={setHidden} />
      ) : (
        <TaskList tasks={tasks} hidden={hidden} setHidden={setHidden} />
      )}
    </div>
  );
}

function ModeButton({
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
      role="radio"
      aria-checked={active}
      className={clsx(
        "btn-brutal-sm gap-1 px-3 py-1.5 text-xs",
        active ? "bg-brutal-yellow" : "bg-white",
      )}
      onClick={onClick}
    >
      <Icon size={13} />
      {label}
    </button>
  );
}

function Board({
  tasks,
  hidden,
  setHidden,
}: {
  tasks: TaskItem[];
  hidden: Record<Status, boolean>;
  setHidden: Dispatch<SetStateAction<Record<Status, boolean>>>;
}) {
  return (
    <div className="stable-scrollbar flex min-h-0 flex-1 gap-4 overflow-auto bg-white p-4">
      {statuses.map((status) => {
        const rows = tasks.filter((t) => t.status === status.id);
        if (hidden[status.id]) {
          return (
            <button
              key={status.id}
              className="flex h-full min-w-12 items-start justify-center border-2 border-black bg-brutal-cream px-2 py-3 text-xs font-black uppercase [writing-mode:vertical-rl]"
              onClick={() =>
                setHidden((s) => ({ ...s, [status.id]: false }))
              }
            >
              Show {status.label}
            </button>
          );
        }
        return (
          <section
            key={status.id}
            className="min-w-[20rem] border-2 border-black bg-white p-3"
          >
            <StatusHeader
              status={status}
              count={rows.length}
              onToggle={() =>
                setHidden((s) => ({ ...s, [status.id]: true }))
              }
            />
            <div className="mt-3 space-y-2">
              {rows.length === 0 ? (
                <div className="border-2 border-dashed border-black/25 px-3 py-6 font-mono text-sm text-black/40">
                  No {status.label.toLowerCase()} tasks.
                </div>
              ) : (
                rows.map((task) => <TaskCard key={task.key} task={task} />)
              )}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function TaskList({
  tasks,
  hidden,
  setHidden,
}: {
  tasks: TaskItem[];
  hidden: Record<Status, boolean>;
  setHidden: Dispatch<SetStateAction<Record<Status, boolean>>>;
}) {
  return (
    <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto bg-white p-4">
      {statuses.map((status) => {
        const rows = tasks.filter((t) => t.status === status.id);
        if (hidden[status.id]) return null;
        return (
          <section key={status.id} className="mb-5">
            <StatusHeader
              status={status}
              count={rows.length}
              onToggle={() =>
                setHidden((s) => ({ ...s, [status.id]: true }))
              }
            />
            <div className="mt-2 space-y-2">
              {rows.length === 0 ? (
                <div className="border-2 border-dashed border-black/25 px-3 py-6 font-mono text-sm text-black/40">
                  No {status.label.toLowerCase()} tasks.
                </div>
              ) : (
                rows.map((task) => <TaskCard key={task.key} task={task} wide />)
              )}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function StatusHeader({
  status,
  count,
  onToggle,
}: {
  status: (typeof statuses)[number];
  count: number;
  onToggle: () => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className={clsx("border-2 border-black px-2 py-0.5 text-[10px] font-black uppercase", status.color)}>
        {status.label}
      </span>
      <span className="font-mono text-xs text-black/45">{count}</span>
      <button className="ml-auto text-black/35 hover:text-black" onClick={onToggle}>
        <ChevronDown size={14} />
      </button>
    </div>
  );
}

function TaskCard({ task, wide }: { task: TaskItem; wide?: boolean }) {
  return (
    <article
      className={clsx(
        "border-2 border-black bg-brutal-cream p-3 shadow-brutal-sm",
        wide && "max-w-3xl",
      )}
    >
      <div className="mb-2 flex items-center gap-2 font-mono text-[11px] text-black/45">
        <span>#{task.channel}</span>
        <span>{task.key}</span>
      </div>
      <div className="text-sm font-black leading-6">{task.title}</div>
      <div className="mt-3 flex flex-wrap gap-2 font-mono text-[11px] text-black/45">
        <span>creator {task.creator}</span>
        <span>assignee {task.assignee}</span>
      </div>
    </article>
  );
}

function deriveTasks(
  messageScopes: ReturnType<typeof useMessages.getState>["byScope"],
  inboxItems: ReturnType<typeof useInbox.getState>["items"],
  actors: ReturnType<typeof useActors.getState>["byId"],
): TaskItem[] {
  const tasks = new Map<string, TaskItem>();
  for (const item of inboxItems) {
    tasks.set(item.requestEventId, {
      key: item.requestEventId,
      channel: item.scope.id,
      title: item.title,
      creator: "request",
      assignee: "you",
      status: "todo",
    });
  }
  for (const [scopeKey, scopeState] of Object.entries(messageScopes)) {
    const [, scopeId] = scopeKey.split(":");
    for (const bubble of scopeState.bubbles) {
      if (bubble.kind !== "actionRequest") continue;
      const status =
        bubble.actionStatus === "accepted" || bubble.actionStatus === "declined"
          ? "done"
          : "todo";
      tasks.set(bubble.id, {
        key: bubble.id,
        channel: scopeId || "unknown",
        title: bubble.actionTitle || firstLine(bubble.text),
        creator: actors[bubble.actorId]?.displayName || bubble.actorId,
        assignee: "pending",
        status,
      });
    }
  }
  return [...tasks.values()].sort((a, b) => a.title.localeCompare(b.title));
}

function firstLine(value: string): string {
  return value.split("\n").find(Boolean) ?? "Untitled task";
}

function scopeLabel(scope: ScopeRef | null, channels: Array<{ id: string; title: string }>) {
  if (!scope) return "No channel selected";
  if (scope.kind === "thread") return `thread ${scope.id}`;
  return channels.find((c) => c.id === scope.id)?.title ?? scope.id;
}
