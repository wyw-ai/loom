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
import type { ScopeRef, Task, TaskStatus } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useSession } from "@/store/session";
import { useTasks } from "@/store/tasks";
import { useUI } from "@/store/ui";

type Status = TaskStatus;

interface TaskItem {
  key: string;
  channel: string;
  title: string;
  creator: string;
  assignee: string;
  status: Status;
  artifactCount: number;
  assignmentCount: number;
}

const statuses: Array<{
  id: Status;
  label: string;
  color: string;
}> = [
  { id: "todo", label: "Todo", color: "bg-brutal-orange" },
  { id: "claimed", label: "Claimed", color: "bg-brutal-yellow" },
  { id: "in_progress", label: "In Progress", color: "bg-brutal-cyan" },
  { id: "waiting_review", label: "In Review", color: "bg-brutal-lavender" },
  { id: "done", label: "Done", color: "bg-brutal-lime" },
  { id: "failed", label: "Failed", color: "bg-brutal-pink" },
  { id: "canceled", label: "Canceled", color: "bg-brutal-cream" },
];

export function TasksPage() {
  const channels = useChannels((s) => s.channels);
  const threadsByChannel = useChannels((s) => s.threadsByChannel);
  const currentScope = useChannels((s) => s.currentScope);
  const actors = useActors((s) => s.byId);
  const selfId = useSession((s) => s.workspace?.actorId);
  const taskRows = useTasks((s) => s.tasks);
  const upsertTask = useTasks((s) => s.upsertTask);
  const openModal = useUI((s) => s.openModal);
  const pushToast = useUI((s) => s.pushToast);
  const [mode, setMode] = useState<"board" | "list">("board");
  const [hidden, setHidden] = useState<Record<Status, boolean>>({
    todo: false,
    claimed: false,
    in_progress: false,
    waiting_review: false,
    done: true,
    failed: true,
    canceled: true,
  });

  const tasks = useMemo(
    () => deriveTasks(taskRows, channels, actors),
    [taskRows, channels, actors],
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
    const channelScope = taskCreateScope(currentScope, threadsByChannel);
    if (!channelScope) {
      pushToast("error", "cannot resolve a channel for the selected thread");
      return;
    }
    const created = await Promise.all(
      titles.map(async (title) => {
        const root = await ipc.eventAppend({
          type: "content.add",
          actorId: selfId,
          scope: channelScope,
          payload: {
            contentType: "text/markdown",
            text: title,
          },
          relations: [],
        });
        const task = await ipc.taskCreate({
          sourceEventId: root.event.id,
          title,
          description: title,
          requesterActorId: selfId,
          ownerActorId: selfId,
          status: "claimed",
        });
        upsertTask(task.task);
        return task.task;
      }),
    );
    pushToast("info", `${created.length} task(s) created`);
  };

  const claimTask = async (task: TaskItem) => {
    if (!selfId) return;
    const res = await ipc.taskUpdate({
      taskId: task.key,
      ownerActorId: selfId,
      status: "claimed",
    });
    upsertTask(res.task);
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
            {tasks.length} task{tasks.length === 1 ? "" : "s"} anchored to channel messages
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
        <Board tasks={tasks} hidden={hidden} setHidden={setHidden} onClaim={claimTask} />
      ) : (
        <TaskList tasks={tasks} hidden={hidden} setHidden={setHidden} onClaim={claimTask} />
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
  onClaim,
}: {
  tasks: TaskItem[];
  hidden: Record<Status, boolean>;
  setHidden: Dispatch<SetStateAction<Record<Status, boolean>>>;
  onClaim: (task: TaskItem) => void;
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
                rows.map((task) => (
                  <TaskCard key={task.key} task={task} onClaim={onClaim} />
                ))
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
  onClaim,
}: {
  tasks: TaskItem[];
  hidden: Record<Status, boolean>;
  setHidden: Dispatch<SetStateAction<Record<Status, boolean>>>;
  onClaim: (task: TaskItem) => void;
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
                rows.map((task) => (
                  <TaskCard key={task.key} task={task} wide onClaim={onClaim} />
                ))
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

function TaskCard({
  task,
  wide,
  onClaim,
}: {
  task: TaskItem;
  wide?: boolean;
  onClaim: (task: TaskItem) => void;
}) {
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
        <span>{task.assignmentCount} assignment(s)</span>
        <span>{task.artifactCount} artifact(s)</span>
      </div>
      {task.assignee === "unassigned" && (
        <button
          className="btn-brutal-sm mt-3 bg-white px-2 py-1 text-[10px]"
          onClick={() => onClaim(task)}
        >
          Claim
        </button>
      )}
    </article>
  );
}

function deriveTasks(
  rows: Task[],
  channels: Array<{ id: string; title: string }>,
  actors: ReturnType<typeof useActors.getState>["byId"],
): TaskItem[] {
  return rows
    .map((task) => ({
      key: task.id,
      channel:
        channels.find((channel) => channel.id === task.channelId)?.title ??
        task.channelId,
      title: `#${task.number} ${task.title}`,
      creator:
        actors[task.requesterActorId]?.displayName || task.requesterActorId,
      assignee: task.ownerActorId
        ? actors[task.ownerActorId]?.displayName || task.ownerActorId
        : "unassigned",
      status: task.status,
      artifactCount: task.artifactIds?.length ?? 0,
      assignmentCount: task.assignmentIds?.length ?? 0,
    }))
    .sort((a, b) => a.title.localeCompare(b.title));
}

function scopeLabel(scope: ScopeRef | null, channels: Array<{ id: string; title: string }>) {
  if (!scope) return "No channel selected";
  if (scope.kind === "thread") return `thread ${scope.id}`;
  return channels.find((c) => c.id === scope.id)?.title ?? scope.id;
}

function taskCreateScope(
  scope: ScopeRef,
  threadsByChannel: ReturnType<typeof useChannels.getState>["threadsByChannel"],
): ScopeRef | null {
  if (scope.kind === "channel") return scope;
  for (const [channelId, threads] of Object.entries(threadsByChannel)) {
    if (threads.some((thread) => thread.id === scope.id)) {
      return { kind: "channel", id: channelId };
    }
  }
  return null;
}
