import {
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";
import clsx from "clsx";
import {
  CheckCircle2,
  ChevronDown,
  ExternalLink,
  Hash,
  List,
  type LucideIcon,
  Plus,
  SquareCheckBig,
  Trello,
  X,
  XCircle,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import { openScope as openChatScope } from "@/features/chat/scopeActions";
import type {
  ScopeRef,
  Task,
  TaskArtifactLink,
  TaskAssignment,
  TaskChangeDelivery,
  TaskFact,
  TaskProjection,
  TaskProjectionHealth,
  TaskRef,
  TaskStatus,
  WorkspaceLease,
} from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useInbox, type InboxItem } from "@/store/inbox";
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

export interface TaskDetailSnapshot {
  task: Task;
  assignments: TaskAssignment[];
  refs: TaskRef[];
  artifactLinks: TaskArtifactLink[];
  facts: TaskFact[];
  projection?: TaskProjection | null;
  projectionHealth: TaskProjectionHealth;
  changes: TaskChangeDelivery[];
  leases: WorkspaceLease[];
}

export async function loadTaskDetailSnapshot(taskId: string): Promise<TaskDetailSnapshot> {
  const [taskRes, projectionRes, changeRes, leaseRes] = await Promise.all([
    ipc.taskGet(taskId),
    ipc.taskProjectionGet({ taskId, projectionType: "summary" }),
    ipc.taskChangeList({ taskId, includeHandled: false, limit: 50 }),
    ipc.taskWorkspaceLeaseList({ activeOnly: true }),
  ]);
  const assignmentIds = new Set(taskRes.assignments.map((item) => item.id));
  const taskDetail = taskRes as typeof taskRes & {
    artifact_links?: TaskArtifactLink[];
  };
  return {
    task: taskRes.task,
    assignments: taskRes.assignments,
    refs: taskRes.refs ?? [],
    artifactLinks: taskRes.artifactLinks ?? taskDetail.artifact_links ?? [],
    facts: taskRes.facts ?? [],
    projection:
      projectionRes.projection ??
      taskRes.projections?.find((p) => p.projectionType === "summary") ??
      null,
    projectionHealth: projectionRes.health,
    changes: changeRes.deliveries ?? [],
    leases: (leaseRes.leases ?? []).filter((lease) =>
      assignmentIds.has(lease.holderAssignmentId),
    ),
  };
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
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [detail, setDetail] = useState<TaskDetailSnapshot | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
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

  useEffect(() => {
    if (!selectedTaskId) return;
    if (tasks.length === 0 || !tasks.some((task) => task.key === selectedTaskId)) {
      setSelectedTaskId(null);
    }
  }, [selectedTaskId, tasks]);

  useEffect(() => {
    if (!selectedTaskId) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSelectedTaskId(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [selectedTaskId]);

  useEffect(() => {
    if (!selectedTaskId) {
      setDetail(null);
      setDetailError(null);
      return;
    }
    let alive = true;
    setDetailLoading(true);
    setDetailError(null);
    loadTaskDetailSnapshot(selectedTaskId)
      .then((snapshot) => {
        if (!alive) return;
        setDetail(snapshot);
      })
      .catch((err) => {
        if (!alive) return;
        setDetail(null);
        setDetailError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (alive) setDetailLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [selectedTaskId, taskRows]);

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

      <div className="relative flex min-h-0 flex-1 overflow-hidden">
        <div className="flex min-h-0 min-w-0 flex-1">
          {mode === "board" ? (
            <Board
              tasks={tasks}
              hidden={hidden}
              setHidden={setHidden}
              onClaim={claimTask}
              onSelect={setSelectedTaskId}
              selectedTaskId={selectedTaskId}
            />
          ) : (
            <TaskList
              tasks={tasks}
              hidden={hidden}
              setHidden={setHidden}
              onClaim={claimTask}
              onSelect={setSelectedTaskId}
              selectedTaskId={selectedTaskId}
            />
          )}
        </div>
        {selectedTaskId && (
          <div
            className="absolute inset-0 z-30 flex justify-end bg-black/25"
            onClick={() => setSelectedTaskId(null)}
          >
            <TaskDetailPanel
              detail={detail}
              loading={detailLoading}
              error={detailError}
              actors={actors}
              onClose={() => setSelectedTaskId(null)}
            />
          </div>
        )}
      </div>
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
  onSelect,
  selectedTaskId,
}: {
  tasks: TaskItem[];
  hidden: Record<Status, boolean>;
  setHidden: Dispatch<SetStateAction<Record<Status, boolean>>>;
  onClaim: (task: TaskItem) => void;
  onSelect: (taskId: string) => void;
  selectedTaskId: string | null;
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
                  <TaskCard
                    key={task.key}
                    task={task}
                    selected={task.key === selectedTaskId}
                    onClaim={onClaim}
                    onSelect={onSelect}
                  />
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
  onSelect,
  selectedTaskId,
}: {
  tasks: TaskItem[];
  hidden: Record<Status, boolean>;
  setHidden: Dispatch<SetStateAction<Record<Status, boolean>>>;
  onClaim: (task: TaskItem) => void;
  onSelect: (taskId: string) => void;
  selectedTaskId: string | null;
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
                  <TaskCard
                    key={task.key}
                    task={task}
                    wide
                    selected={task.key === selectedTaskId}
                    onClaim={onClaim}
                    onSelect={onSelect}
                  />
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
  selected,
  onClaim,
  onSelect,
}: {
  task: TaskItem;
  wide?: boolean;
  selected?: boolean;
  onClaim: (task: TaskItem) => void;
  onSelect: (taskId: string) => void;
}) {
  return (
    <article
      className={clsx(
        "cursor-pointer border-2 border-black bg-brutal-cream p-3 shadow-brutal-sm",
        selected && "bg-brutal-yellow",
        wide && "max-w-3xl",
      )}
      onClick={() => onSelect(task.key)}
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
          onClick={(event) => {
            event.stopPropagation();
            onClaim(task);
          }}
        >
          Claim
        </button>
      )}
    </article>
  );
}

export function TaskDetailPanel({
  detail,
  loading,
  error,
  actors,
  onClose,
}: {
  detail: TaskDetailSnapshot | null;
  loading: boolean;
  error: string | null;
  actors: ReturnType<typeof useActors.getState>["byId"];
  onClose: () => void;
}) {
  const payload = projectionPayload(detail?.projection);
  const targets = projectionTargets(payload);
  const ownerAttention = projectionOwnerAttention(payload);
  const pendingChanges = detail?.changes.filter((item) => item.status !== "handled") ?? [];
  const inboxItems = useInbox((s) => s.items);
  const pendingActions = detail
    ? inboxItems.filter((item) => isActionForTask(item, detail.task))
    : [];

  return (
    <aside
      className="flex h-full w-[min(42rem,calc(100vw-5rem))] shrink-0 flex-col border-l-2 border-black bg-white shadow-[-8px_0_0_rgba(0,0,0,0.18)]"
      onClick={(event) => event.stopPropagation()}
    >
      <div className="border-b-2 border-black px-4 py-3">
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-black uppercase tracking-wider text-black/45">Task Detail</div>
            <div className="truncate text-sm font-black">
              {detail ? `#${detail.task.number} ${detail.task.title}` : "No task selected"}
            </div>
          </div>
          <HealthBadge health={detail?.projectionHealth ?? "missing"} />
          <button
            type="button"
            aria-label="Close task detail"
            className="btn-brutal-sm bg-white p-1.5"
            onClick={onClose}
          >
            <X size={14} />
          </button>
        </div>
      </div>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto p-4">
        {loading && (
          <div className="border-2 border-dashed border-black/30 p-4 font-mono text-xs text-black/45">
            Loading task projection...
          </div>
        )}
        {error && (
          <div className="border-2 border-black bg-brutal-pink p-3 text-xs font-black">
            {error}
          </div>
        )}
        {!loading && !error && !detail && (
          <div className="border-2 border-dashed border-black/30 p-4 font-mono text-xs text-black/45">
            Select a task to inspect its Joi-native state.
          </div>
        )}
        {detail && (
          <div className="space-y-4">
            <DetailSection title="摘要">
              <div className="text-sm font-black leading-6">
                {asString(payload?.summary) || detail.task.resultSummary || "No projection summary."}
              </div>
              <div className="mt-2 grid grid-cols-2 gap-2 font-mono text-[11px] text-black/55">
                <InfoPill label="status" value={detail.task.status} />
                <InfoPill label="owner" value={actorName(detail.task.ownerActorId, actors)} />
                <InfoPill label="artifacts" value={String(detail.artifactLinks.length)} />
                <InfoPill label="facts" value={String(detail.facts.length)} />
              </div>
            </DetailSection>

            <DetailSection title="Target Matrix">
              {targets.length === 0 ? (
                <EmptyLine text="No projection targets." />
              ) : (
                <div className="space-y-2">
                  {targets.map((target) => (
                    <TargetRow key={target.key} target={target} />
                  ))}
                </div>
              )}
            </DetailSection>

            <DetailSection title="Projection">
              {detail.projection ? (
                <ProjectionDetail projection={detail.projection} />
              ) : (
                <EmptyLine text="No projection document." />
              )}
            </DetailSection>

            <DetailSection title="Owner Attention">
              {ownerAttention.length === 0 && pendingChanges.length === 0 ? (
                <EmptyLine text="No pending owner attention." />
              ) : (
                <div className="space-y-2">
                  {ownerAttention.map((item, index) => (
                    <AttentionRow key={`owner-${index}`} item={item} />
                  ))}
                  {pendingChanges.map((item) => (
                    <AttentionRow
                      key={item.change.id}
                      item={{
                        kind: item.change.changeType,
                        status: item.status,
                        reason: item.change.summary,
                      }}
                    />
                  ))}
                </div>
              )}
            </DetailSection>

            <DetailSection title="Human Action">
              {pendingActions.length === 0 ? (
                <EmptyLine text="No pending human action." />
              ) : (
                <div className="space-y-2">
                  {pendingActions.map((item) => (
                    <HumanActionRow key={item.requestEventId} item={item} />
                  ))}
                </div>
              )}
            </DetailSection>

            <DetailSection title="Refs">
              {detail.refs.length === 0 ? (
                <EmptyLine text="No task refs." />
              ) : (
                <div className="space-y-2">
                  {detail.refs.map((ref) => (
                    <RefRow key={ref.id} taskRef={ref} />
                  ))}
                </div>
              )}
            </DetailSection>

            <DetailSection title="Assignments">
              {detail.assignments.length === 0 ? (
                <EmptyLine text="No assignments." />
              ) : (
                <div className="space-y-2">
                  {detail.assignments.map((assignment) => (
                    <AssignmentRow
                      key={assignment.id}
                      assignment={assignment}
                      actors={actors}
                      lease={detail.leases.find((lease) => lease.holderAssignmentId === assignment.id)}
                    />
                  ))}
                </div>
              )}
            </DetailSection>

            <DetailSection title="Facts">
              {detail.facts.length === 0 ? (
                <EmptyLine text="No facts." />
              ) : (
                <div className="space-y-2">
                  {detail.facts.slice(0, 8).map((fact) => (
                    <FactRow key={fact.id} fact={fact} />
                  ))}
                  {detail.facts.length > 8 && (
                    <JsonDetails label={`Show ${detail.facts.length - 8} more fact id(s)`} value={detail.facts.slice(8).map((fact) => fact.id)} />
                  )}
                </div>
              )}
            </DetailSection>

            <DetailSection title="Artifacts">
              {detail.artifactLinks.length === 0 ? (
                <EmptyLine text="No artifact links." />
              ) : (
                <div className="space-y-2">
                  {detail.artifactLinks.slice(0, 8).map((link) => (
                    <ArtifactRow key={link.id} link={link} />
                  ))}
                  {detail.artifactLinks.length > 8 && (
                    <JsonDetails label={`Show ${detail.artifactLinks.length - 8} more artifact link id(s)`} value={detail.artifactLinks.slice(8).map((link) => link.id)} />
                  )}
                </div>
              )}
            </DetailSection>
          </div>
        )}
      </div>
    </aside>
  );
}

function DetailSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <div className="mb-2 text-[11px] font-black uppercase tracking-wider text-black/45">
        {title}
      </div>
      <div className="border-2 border-black bg-brutal-cream p-3 shadow-brutal-sm">
        {children}
      </div>
    </section>
  );
}

function HealthBadge({ health }: { health: TaskProjectionHealth }) {
  const color =
    health === "fresh"
      ? "bg-brutal-lime"
      : health === "missing" || health === "stale"
        ? "bg-brutal-yellow"
        : "bg-brutal-pink";
  return (
    <span className={clsx("border-2 border-black px-2 py-1 font-mono text-[10px] font-black uppercase", color)}>
      {health}
    </span>
  );
}

function InfoPill({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 border-2 border-black bg-white px-2 py-1">
      <span className="text-black/40">{label}</span>{" "}
      <span className="break-words font-black text-black">{value}</span>
    </div>
  );
}

function EmptyLine({ text }: { text: string }) {
  return <div className="font-mono text-xs text-black/45">{text}</div>;
}

function JsonDetails({ label, value }: { label: string; value: unknown }) {
  return (
    <details className="group border border-black/40 bg-white">
      <summary className="cursor-pointer px-2 py-1 font-mono text-[10px] font-black uppercase text-black/60 hover:bg-brutal-yellow">
        {label}
      </summary>
      <pre className="max-h-72 overflow-auto border-t border-black/30 bg-black p-2 font-mono text-[10px] leading-4 text-brutal-lime">
        {formatJson(value)}
      </pre>
    </details>
  );
}

interface ProjectionTarget {
  key: string;
  label?: string;
  kind?: string;
  required?: boolean;
  terminal_condition?: string;
  next_owner?: string;
  columns?: Record<string, unknown>;
}

function TargetRow({ target }: { target: ProjectionTarget }) {
  const columns = target.columns ?? {};
  const visibleColumns = projectionColumnEntries(columns);
  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs font-black">{target.label || target.key}</div>
          <div className="font-mono text-[10px] text-black/45">
            {target.kind || "target"} · {target.terminal_condition || "pass"}
          </div>
        </div>
        <span className="border-2 border-black bg-brutal-cyan px-2 py-0.5 font-mono text-[10px] font-black">
          {target.next_owner || "router"}
        </span>
      </div>
      {visibleColumns.length === 0 ? (
        <div className="mt-2 font-mono text-[10px] text-black/45">No target columns.</div>
      ) : (
        <div className="mt-2 grid grid-cols-2 gap-1 font-mono text-[10px]">
          {visibleColumns.map(([label, value]) => (
            <ColumnChip key={label} label={label} value={value} />
          ))}
        </div>
      )}
      <div className="mt-2 space-y-1">
        <JsonDetails label="Target detail" value={target} />
      </div>
    </div>
  );
}

function ColumnChip({ label, value }: { label: string; value: unknown }) {
  return (
    <div className="min-w-0 border border-black/40 bg-white px-1.5 py-1">
      <div className="truncate text-black/40">{label}</div>
      <div className="truncate font-black">{stringifySmall(value) || "-"}</div>
    </div>
  );
}

function AttentionRow({ item }: { item: Record<string, unknown> }) {
  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="font-mono text-[10px] font-black uppercase">
        {asString(item.kind) || "attention"} · {asString(item.status) || "pending"}
      </div>
      <div className="mt-1 text-xs leading-5 text-black/70">
        {asString(item.reason) || asString(item.summary) || "Needs owner review."}
      </div>
    </div>
  );
}

function ProjectionDetail({ projection }: { projection: TaskProjection }) {
  return (
    <div className="space-y-2">
      <div className="grid grid-cols-2 gap-2 font-mono text-[10px] text-black/55">
        <InfoPill label="type" value={projection.projectionType} />
        <InfoPill label="schema" value={projection.payloadSchema || "-"} />
        <InfoPill label="producer" value={projection.producerActorId} />
        <InfoPill label="updated" value={projection.updatedAt} />
      </div>
      <JsonDetails label="Projection payload" value={projection.payload ?? null} />
      <JsonDetails label="Projection watermark" value={projection.watermark ?? null} />
    </div>
  );
}

function RefRow({ taskRef }: { taskRef: TaskRef }) {
  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="flex items-center gap-2 font-mono text-[10px]">
        <span className="font-black">{taskRef.kind}</span>
        {taskRef.subtype && <span className="text-black/45">{taskRef.subtype}</span>}
        <span className="ml-auto border border-black/40 bg-brutal-yellow px-1.5 py-0.5 font-black">
          {taskRef.confidence}
        </span>
      </div>
      <div className="mt-1 break-all text-xs font-black leading-5">{taskRef.normalized || taskRef.value}</div>
      <div className="mt-1 font-mono text-[10px] text-black/45">{taskRef.status}</div>
      {(taskRef.fields !== undefined || taskRef.sourceEventId) && (
        <div className="mt-2">
          <JsonDetails
            label="Ref detail"
            value={{
              id: taskRef.id,
              value: taskRef.value,
              normalized: taskRef.normalized,
              fields: taskRef.fields,
              sourceEventId: taskRef.sourceEventId,
            }}
          />
        </div>
      )}
    </div>
  );
}

function HumanActionRow({ item }: { item: InboxItem }) {
  const selfId = useSession((s) => s.workspace?.actorId);
  const remove = useInbox((s) => s.remove);
  const pushToast = useUI((s) => s.pushToast);

  const respond = async (
    optionId: string,
    kind: "accepted" | "declined" | "answered",
  ) => {
    if (!selfId) return;
    try {
      await openChatScope(item.scope);
      await ipc.eventAppend({
        type: "action.response",
        actorId: selfId,
        scope: item.scope,
        payload: {
          optionId,
          kind,
          ...(item.actionRequestId ? { requestId: item.actionRequestId } : {}),
        },
        relations: [
          { kind: "responds_to", target: { kind: "event", id: item.requestEventId } },
        ],
      });
      remove(item.requestEventId);
    } catch (e) {
      pushToast(
        "error",
        `action.response failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  };

  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="font-mono text-[10px] font-black uppercase">
        {item.requestType || "action"} · {item.targetKey || item.scope.id}
      </div>
      <div className="mt-1 text-xs font-black leading-5">{item.title}</div>
      {(item.reason || item.description) && (
        <div className="mt-1 whitespace-pre-wrap text-xs leading-5 text-black/70">
          {item.reason || item.description}
        </div>
      )}
      {item.command && (
        <code className="mt-2 block overflow-x-auto border border-black/40 bg-brutal-cream px-2 py-1 font-mono text-[10px] leading-4 text-black/70">
          {item.command}
        </code>
      )}
      <div className="mt-2 flex flex-wrap items-center gap-2">
        {(item.choices.length > 0
          ? item.choices
          : [
              { id: "approve", label: "Approve" },
              { id: "reject", label: "Reject" },
            ]
        ).map((choice) => {
          const isDecline = /reject|decline|cancel|abort|no|wait/i.test(choice.label);
          const kind =
            item.requestType === "question" ||
            item.requestType === "human_decision"
              ? "answered"
              : isDecline
                ? "declined"
                : "accepted";
          const Icon = isDecline ? XCircle : CheckCircle2;
          return (
            <button
              key={choice.id}
              onClick={() => void respond(choice.id, kind)}
              className={
                isDecline
                  ? "btn-brutal-sm inline-flex gap-1 bg-white px-2 py-1 text-[10px]"
                  : "btn-brutal-sm inline-flex gap-1 bg-brutal-pink px-2 py-1 text-[10px]"
              }
            >
              <Icon size={11} />
              {choice.label}
            </button>
          );
        })}
        <button
          onClick={() => void openChatScope(item.scope)}
          className="btn-brutal-sm ml-auto inline-flex gap-1 bg-white px-2 py-1 text-[10px]"
        >
          <ExternalLink size={11} />
          Open
        </button>
      </div>
    </div>
  );
}

function AssignmentRow({
  assignment,
  actors,
  lease,
}: {
  assignment: TaskAssignment;
  actors: ReturnType<typeof useActors.getState>["byId"];
  lease?: WorkspaceLease;
}) {
  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="flex items-center gap-2">
        <span className="border-2 border-black bg-brutal-lavender px-2 py-0.5 font-mono text-[10px] font-black">
          {assignment.status}
        </span>
        <span className="min-w-0 truncate font-mono text-[11px] text-black/55">
          {actorName(assignment.toActorId, actors)}
        </span>
      </div>
      <div className="mt-1 line-clamp-2 text-xs leading-5">{assignment.instruction}</div>
      <div className="mt-2 flex flex-wrap gap-1 font-mono text-[10px] text-black/55">
        <span>{assignment.type}</span>
        {lease && <span>{lease.mode} lease</span>}
        {assignment.resultFactIds?.length ? <span>{assignment.resultFactIds.length} result facts</span> : null}
      </div>
      <div className="mt-2 space-y-1">
        {assignment.contract !== undefined && (
          <JsonDetails label="Contract" value={assignment.contract} />
        )}
        {assignment.resultEnvelope !== undefined && (
          <JsonDetails label="Result envelope" value={assignment.resultEnvelope} />
        )}
        {assignment.resultArtifactIds?.length ? (
          <JsonDetails label="Result artifacts" value={assignment.resultArtifactIds} />
        ) : null}
        {assignment.resultFactIds?.length ? (
          <JsonDetails label="Result facts" value={assignment.resultFactIds} />
        ) : null}
        {assignment.evidenceRefs?.length ? (
          <JsonDetails label="Evidence refs" value={assignment.evidenceRefs} />
        ) : null}
      </div>
    </div>
  );
}

function FactRow({ fact }: { fact: TaskFact }) {
  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="flex items-center gap-2 font-mono text-[10px]">
        <span className="font-black">{fact.kind}</span>
        <span className="text-black/45">{fact.snapshotCompleteness || "complete"}</span>
        <span className="ml-auto text-black/45">{fact.status}</span>
      </div>
      <div className="mt-1 text-xs leading-5 text-black/70">{fact.summary}</div>
      <div className="mt-2 space-y-1">
        <JsonDetails
          label="Fact detail"
          value={{
            id: fact.id,
            targetKey: fact.targetKey,
            factType: fact.factType,
            authority: fact.authority,
            authorityBinding: fact.authorityBinding,
            observedFields: fact.observedFields,
            unobservedFields: fact.unobservedFields,
            unavailableReason: fact.unavailableReason,
            producerId: fact.producerId,
            payloadSchema: fact.payloadSchema,
            payload: fact.payload,
          }}
        />
      </div>
    </div>
  );
}

function ArtifactRow({ link }: { link: TaskArtifactLink }) {
  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="font-mono text-[10px] font-black">{link.schema}</div>
      <div className="mt-1 flex flex-wrap gap-2 font-mono text-[10px] text-black/55">
        <span>{link.role}</span>
        <span>{link.status}</span>
        <span>#{link.sequence}</span>
      </div>
      <div className="mt-2 space-y-1">
        <JsonDetails
          label="Artifact detail"
          value={{
            id: link.id,
            artifactId: link.artifactId,
            lineage: link.lineage,
            binding: link.binding,
            createdByActorId: link.createdByActorId,
            createdAt: link.createdAt,
          }}
        />
      </div>
    </div>
  );
}

function projectionPayload(projection?: TaskProjection | null): Record<string, unknown> | null {
  const payload = projection?.payload;
  return isRecord(payload) ? payload : null;
}

function projectionTargets(payload: Record<string, unknown> | null): ProjectionTarget[] {
  const rows = Array.isArray(payload?.targets) ? payload.targets : [];
  return rows.filter(isRecord).flatMap((row) => {
    const key = asString(row.key);
    if (!key) return [];
    return [{
      key,
      label: asString(row.label),
      kind: asString(row.kind),
      required: typeof row.required === "boolean" ? row.required : undefined,
      terminal_condition: asString(row.terminal_condition),
      next_owner: asString(row.next_owner),
      columns: isRecord(row.columns) ? row.columns : {},
    }];
  });
}

function projectionOwnerAttention(payload: Record<string, unknown> | null): Array<Record<string, unknown>> {
  const rows = Array.isArray(payload?.owner_attention) ? payload.owner_attention : [];
  return rows.filter(isRecord);
}

function projectionColumnEntries(columns: Record<string, unknown>): Array<[string, unknown]> {
  const priority = [
    "ci",
    "test",
    "review",
    "approval",
    "permission",
    "release",
    "deploy",
    "merged",
    "comments",
  ];
  const seen = new Set<string>();
  const rows: Array<[string, unknown]> = [];
  for (const key of priority) {
    if (Object.prototype.hasOwnProperty.call(columns, key)) {
      seen.add(key);
      rows.push([key, columns[key]]);
    }
  }
  for (const [key, value] of Object.entries(columns)) {
    if (!seen.has(key)) rows.push([key, value]);
  }
  return rows;
}

function isActionForTask(item: InboxItem, task: Task): boolean {
  if (item.taskId) return item.taskId === task.id;
  return item.scope.kind === "thread" && item.scope.id === task.canonicalThreadId;
}

function actorName(
  actorId: string | null | undefined,
  actors: ReturnType<typeof useActors.getState>["byId"],
) {
  if (!actorId) return "unassigned";
  return actors[actorId]?.displayName || actorId;
}

function asString(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

function stringifySmall(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value);
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
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
