import { useEffect, useMemo, useState, type ReactNode } from "react";
import clsx from "clsx";
import {
  BellRing,
  CalendarClock,
  ChevronDown,
  ChevronRight,
  Clock3,
  Copy,
  Edit3,
  FileText,
  Folder,
  FolderOpen,
  Hash,
  type LucideIcon,
  MessageSquare,
  Monitor,
  Plus,
  RefreshCw,
  RotateCcw,
  Square,
  Trash2,
  Users,
  Workflow,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type {
  Actor,
  ActorKind,
  AgentInfo,
  AgentProviderSummary,
  JoiEvent,
  MachineAgentInfo,
  MachineInfo,
  Reminder,
  ReminderStatus,
  ScopeRef,
} from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import { summarizeActionRequest } from "@/features/chat/actionRequestSummary";
import { openScope } from "@/features/chat/scopeActions";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";
import { PixelAvatar } from "@/features/common/PixelAvatar";

type AgentTab = "profile" | "dms" | "reminders" | "workspace" | "activity";

const CODEX_MODELS = [
  { id: "gpt-5.5", label: "GPT-5.5" },
  { id: "gpt-5.4", label: "GPT-5.4" },
  { id: "gpt-5.4-mini", label: "GPT-5.4 Mini" },
  { id: "gpt-5.3-codex", label: "GPT-5.3 Codex" },
];

const REASONING_CHOICES = ["low", "medium", "high", "xhigh"];

interface ManagedAgent {
  actor: Actor;
  managed: boolean;
  canOpenLocalPath: boolean;
  status: string;
  machineId: string;
  machine: string;
  dataRoot: string;
  profilePath: string;
  identityPath: string;
  soulPath: string;
  providerId: string;
  provider: string;
  providers: AgentProviderSummary[];
  runtime: string;
  model: string;
  reasoningEffort: string;
  description: string;
  creator: string;
  created: string;
  env: Array<{ key: string; value: string }>;
  command: string;
  args: string[];
  autostart: boolean;
}

interface AgentUpdatePatch {
  displayName?: string;
  description?: string;
  providerId?: string;
  model?: string;
  reasoningEffort?: string;
  autostart?: boolean;
}

type AgentMachineContext = Pick<
  MachineInfo,
  "id" | "name" | "dataRoot" | "providers" | "readOnly" | "canOpenLocalPath"
> &
  Partial<Pick<MachineAgentInfo, "profilePath" | "identityPath" | "soulPath">>;

export function MembersPage() {
  const actorsById = useActors((s) => s.byId);
  const upsertMany = useActors((s) => s.upsertMany);
  const removeMany = useActors((s) => s.removeMany);
  const currentScope = useChannels((s) => s.currentScope);
  const channels = useChannels((s) => s.channels);
  const replaceMembers = useChannels((s) => s.replaceMembers);
  const activeWorkspaceId = useWorkspaces((s) => s.activeId);
  const sessionWorkspaceId = useSession((s) => s.workspace?.id ?? null);
  const setView = useUI((s) => s.setView);
  const setDraft = useUI((s) => s.setDraft);
  const pushToast = useUI((s) => s.pushToast);
  const openModal = useUI((s) => s.openModal);
  const [agents, setAgents] = useState<ManagedAgent[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<AgentTab>("profile");
  const [openKinds, setOpenKinds] = useState<Record<string, boolean>>({});

  const applyMachineAgents = (machines: MachineInfo[]) => {
    const rows = machines.flatMap((machine) =>
      machine.agents.map((agent) => normalizeAgent(agent, machine)),
    );
    setAgents(rows);
    upsertMany(rows.map((row) => row.actor));
    setSelectedId((current) =>
      current && rows.some((row) => row.actor.id === current)
        ? current
        : rows[0]?.actor.id ?? null,
    );
  };

  useEffect(() => {
    let alive = true;
    const requestWorkspaceId = activeWorkspaceId;
    setAgents([]);
    setSelectedId(null);
    (async () => {
      const [machineResult, actorResult] = await Promise.allSettled([
        ipc.machineList(),
        ipc.actorList(),
      ]);
      if (!alive || useWorkspaces.getState().activeId !== requestWorkspaceId) {
        return;
      }
      if (actorResult.status === "fulfilled") {
        upsertMany(actorResult.value.actors);
      }
      if (machineResult.status === "fulfilled") {
        applyMachineAgents(machineResult.value.machines);
      } else {
        pushToast(
          "error",
          `machine/list failed: ${
            machineResult.reason instanceof Error
              ? machineResult.reason.message
              : String(machineResult.reason)
          }`,
        );
        setAgents([]);
      }
    })();
    return () => {
      alive = false;
    };
  }, [activeWorkspaceId, sessionWorkspaceId, pushToast, upsertMany]);

  useEffect(() => {
    let alive = true;
    const requestWorkspaceId = activeWorkspaceId;
    const visibleChannels = channels;
    if (visibleChannels.length === 0) return;

    (async () => {
      const results = await Promise.allSettled(
        visibleChannels.map(async (channel) => ({
          channelId: channel.id,
          result: await ipc.channelMembers(channel.id),
        })),
      );
      if (!alive || useWorkspaces.getState().activeId !== requestWorkspaceId) {
        return;
      }
      for (const result of results) {
        if (result.status !== "fulfilled") continue;
        replaceMembers(result.value.channelId, result.value.result.members);
      }
    })();

    return () => {
      alive = false;
    };
  }, [
    activeWorkspaceId,
    channels.map((channel) => channel.id).join("\0"),
    replaceMembers,
  ]);

  const allMembers = useMemo(() => {
    const managedIds = new Set(agents.map((agent) => agent.actor.id));
    const registryActors = Object.values(actorsById)
      .filter((actor) => !managedIds.has(actor.id))
      .sort((a, b) =>
        (a.displayName || a.id).localeCompare(b.displayName || b.id),
      )
      .map(normalizeRegistryActor);
    return [...agents, ...registryActors];
  }, [agents, actorsById]);
  const memberSections = useMemo(
    () => groupMembersByKind(allMembers),
    [allMembers],
  );
  const selected = allMembers.find((a) => a.actor.id === selectedId) ?? null;

  useEffect(() => {
    setSelectedId((current) =>
      current && allMembers.some((agent) => agent.actor.id === current)
        ? current
        : allMembers[0]?.actor.id ?? null,
    );
  }, [allMembers]);

  const removeAgent = async (agent: ManagedAgent) => {
    if (!agent.managed) {
      pushToast("warn", "registry-only agents are managed by their remote serve");
      return;
    }
    const result = await ipc.machineAgentRemove(agent.machineId, agent.actor.id);
    removeMany([agent.actor.id]);
    applyMachineAgents(result.machines);
    pushToast("info", `${agent.actor.displayName || agent.actor.id} removed`);
  };

  const updateAgent = async (
    agent: ManagedAgent,
    patch: AgentUpdatePatch,
  ) => {
    if (!agent.managed) {
      pushToast("warn", "registry-only agents are read-only in this workspace");
      return;
    }
    const updated = normalizeAgent(
      await ipc.agentUpdate({
        machineId: agent.machineId,
        actorId: agent.actor.id,
        ...patch,
      }),
      {
        id: agent.machineId,
        name: agent.machine,
        dataRoot: agent.dataRoot,
        providers: agent.providers,
        readOnly: !agent.managed,
        canOpenLocalPath: agent.canOpenLocalPath,
        profilePath: agent.profilePath,
        identityPath: agent.identityPath,
        soulPath: agent.soulPath,
      },
    );
    setAgents((xs) =>
      xs.map((row) => (row.actor.id === updated.actor.id ? updated : row)),
    );
    upsertMany([updated.actor]);
    pushToast("info", `${updated.actor.displayName || updated.actor.id} updated`);
  };

  const messageAgent = () => {
    if (!selected) return;
    setView("chat");
    if (!currentScope) {
      pushToast("info", "Pick a channel before messaging this member");
      return;
    }
    setDraft(currentScope, `@${selected.actor.id} `);
    window.dispatchEvent(
      new CustomEvent("joi:focus-prompt", {
        detail: { scopeKey: scopeKey(currentScope) },
      }),
    );
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 bg-white text-black">
      <aside className="hidden h-full w-60 shrink-0 select-none flex-col border-r-2 border-black bg-brutal-cream md:flex">
        <header className="flex h-panel-header shrink-0 items-center border-b-2 border-black px-5">
          <div className="text-lg font-black">Members</div>
        </header>
        <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto px-2 py-3">
          {memberSections.map((section) => {
            const open = openKinds[section.kind] ?? true;
            return (
              <div key={section.kind}>
                <GroupHeader
                  label={actorKindPlural(section.kind)}
                  count={section.members.length}
                  open={open}
                  onToggle={() =>
                    setOpenKinds((state) => ({
                      ...state,
                      [section.kind]: !(state[section.kind] ?? true),
                    }))
                  }
                  onAdd={
                    section.kind === "agent"
                      ? () => setView("machines")
                      : section.kind === "human"
                        ? () =>
                            openModal({
                              type: "input",
                              title: "Invite human",
                              label: "Email or actor id",
                              placeholder: "name@example.com",
                              confirmLabel: "Invite",
                              onSubmit: (value) =>
                                pushToast("info", `invite staged for ${value}`),
                            })
                        : undefined
                  }
                />
                {open &&
                  section.members.map((member) => (
                    <AgentListRow
                      key={member.actor.id}
                      agent={member}
                      active={member.actor.id === selectedId}
                      onClick={() => {
                        setSelectedId(member.actor.id);
                        setTab("profile");
                      }}
                    />
                  ))}
              </div>
            );
          })}

          {memberSections.length === 0 && (
            <div className="px-3 py-6 text-center font-mono text-xs text-black/40">
              No actors found
            </div>
          )}
        </div>
      </aside>

      <main className="min-h-0 min-w-0 flex-1 bg-white">
        {selected ? (
          <AgentDetail
            agent={selected}
            tab={tab}
            setTab={setTab}
            onMessage={messageAgent}
            onRemove={() => void removeAgent(selected)}
            onUpdate={(patch) => updateAgent(selected, patch)}
          />
        ) : (
          <div className="flex h-full items-center justify-center text-lg font-black uppercase text-black/40">
            Select a member
          </div>
        )}
      </main>

    </div>
  );
}

function GroupHeader({
  label,
  count,
  open,
  onToggle,
  onAdd,
}: {
  label: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  onAdd?: () => void;
}) {
  return (
    <div className="mb-1 mt-2 flex items-center justify-between px-2">
      <button
        type="button"
        className="flex items-center gap-1 text-xs font-black uppercase tracking-widest text-black"
        onClick={onToggle}
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {label}
        <span className="font-mono text-black/40">{count}</span>
      </button>
      {onAdd && (
        <button className="btn-brutal-sm bg-white p-0.5" onClick={onAdd}>
          <Plus size={14} />
        </button>
      )}
    </div>
  );
}

function AgentListRow({
  agent,
  active,
  onClick,
}: {
  agent: ManagedAgent;
  active: boolean;
  onClick: () => void;
}) {
  const online = agent.status.toLowerCase() === "online";
  return (
    <>
      <div className="mt-1 flex items-center gap-1 px-3 text-[10px] font-mono lowercase text-black/40">
        <Monitor size={9} />
        <span className="truncate">{agent.machine}</span>
      </div>
      <button
        className={clsx(
          "mb-1 flex w-full items-center gap-2 border-2 px-2 py-1.5 text-left text-sm font-bold transition-colors",
          active
            ? "border-black bg-brutal-pink shadow-brutal-sm"
            : "border-transparent hover:border-black hover:bg-white hover:shadow-brutal-sm",
        )}
        onClick={onClick}
      >
        <PixelAvatar
          id={agent.actor.id}
          label={agent.actor.displayName}
          size={20}
        />
        <span className="min-w-0 flex-1 truncate">
          {agent.actor.displayName || agent.actor.id}
        </span>
        {agent.managed ? (
          <span
            className={clsx(
              "h-2.5 w-2.5 shrink-0 rounded-full border border-black",
              online ? "bg-brutal-lime" : "bg-black/20",
            )}
            title={online ? "online" : agent.status}
          />
        ) : (
          <span className="font-mono text-[10px] text-black/45">
            {agent.actor.kind}
          </span>
        )}
      </button>
    </>
  );
}

function AgentDetail({
  agent,
  tab,
  setTab,
  onMessage,
  onRemove,
  onUpdate,
}: {
  agent: ManagedAgent;
  tab: AgentTab;
  setTab: (tab: AgentTab) => void;
  onMessage: () => void;
  onRemove: () => void;
  onUpdate: (patch: AgentUpdatePatch) => Promise<void>;
}) {
  const ui = useUI();
  const online = agent.status.toLowerCase() === "online";

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex h-panel-header shrink-0 items-center gap-3 border-b-2 border-black bg-white px-5">
        <PixelAvatar id={agent.actor.id} label={agent.actor.displayName} size={36} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-base font-black">
            {agent.actor.displayName || agent.actor.id}
          </div>
          <div className="font-mono text-xs text-black/45">{agent.actor.id}</div>
        </div>
        <button className="btn-brutal-sm gap-1 bg-white px-3 py-1.5 text-xs" onClick={onMessage}>
          <MessageSquare size={14} /> Message
        </button>
        {agent.managed ? (
          <>
            <button
              className="btn-brutal-sm bg-white p-1.5"
              title="Stop Agent"
              onClick={() =>
                ui.openModal({
                  type: "confirm",
                  title: `Stop ${agent.actor.displayName || agent.actor.id}?`,
                  body: "This agent is owned by the machine daemon. Stop or restart the daemon on the host computer to control the process.",
                  confirmLabel: "Got It",
                  danger: true,
                  onConfirm: () =>
                    ui.pushToast("warn", "agent process control belongs to joi daemon"),
                })
              }
            >
              <Square size={14} />
            </button>
            <button
              className="btn-brutal-sm bg-white p-1.5"
              title="Restart / Reset"
              onClick={() =>
                ui.pushToast("warn", "restart the owning `joi daemon` process")
              }
            >
              <RotateCcw size={14} />
            </button>
            <button
              className="btn-brutal-sm bg-white p-1.5"
              title="Remove Agent"
              onClick={() =>
                ui.openModal({
                  type: "confirm",
                  title: `Remove ${agent.actor.displayName || agent.actor.id}?`,
                  body: "This removes the agent from its computer profile. It does not stop an already running daemon process.",
                  confirmLabel: "Remove",
                  danger: true,
                  onConfirm: onRemove,
                })
              }
            >
              <Trash2 size={14} />
            </button>
          </>
        ) : (
          <span className="chip-brutal bg-white">registry</span>
        )}
      </header>

      <div className="flex shrink-0 overflow-x-auto border-b-2 border-black bg-white scrollbar-none">
        <AgentTabButton active={tab === "profile"} icon={Users} label="Profile" onClick={() => setTab("profile")} />
        <AgentTabButton active={tab === "dms"} icon={MessageSquare} label="Agent DMs" onClick={() => setTab("dms")} />
        <AgentTabButton active={tab === "reminders"} icon={BellRing} label="Reminders" onClick={() => setTab("reminders")} />
        <AgentTabButton active={tab === "workspace"} icon={Folder} label="Workspace" onClick={() => setTab("workspace")} />
        <AgentTabButton active={tab === "activity"} icon={Workflow} label="Activity" onClick={() => setTab("activity")} />
      </div>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto bg-white">
        {tab === "profile" && (
          <ProfileTab agent={agent} online={online} onUpdate={onUpdate} />
        )}
        {tab === "dms" && <EmptyTab label="No agent-to-agent DMs yet" />}
        {tab === "reminders" && <RemindersTab agent={agent} />}
        {tab === "workspace" && <WorkspaceTab agent={agent} />}
        {tab === "activity" && <ActivityTab agent={agent} />}
      </div>
    </div>
  );
}

function AgentTabButton({
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
        "flex items-center gap-1.5 border-r-2 border-black px-4 py-1.5 text-xs font-black uppercase tracking-wide",
        active ? "bg-brutal-yellow" : "bg-white hover:bg-black/5",
      )}
      onClick={onClick}
    >
      <Icon size={12} />
      {label}
    </button>
  );
}

type ReminderFilter = ReminderStatus | "all";
type ThreadsByChannel = ReturnType<typeof useChannels.getState>["threadsByChannel"];
type ChannelList = ReturnType<typeof useChannels.getState>["channels"];

interface LinkedMessagePreview {
  actorId?: string;
  text: string;
  occurredAt?: string;
  missing?: boolean;
}

const REMINDER_FILTERS: Array<{ id: ReminderFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "scheduled", label: "Scheduled" },
  { id: "fired", label: "Fired" },
  { id: "cancelled", label: "Cancelled" },
];

function RemindersTab({ agent }: { agent: ManagedAgent }) {
  const channels = useChannels((s) => s.channels);
  const threadsByChannel = useChannels((s) => s.threadsByChannel);
  const pushToast = useUI((s) => s.pushToast);
  const setView = useUI((s) => s.setView);
  const [reminders, setReminders] = useState<Reminder[]>([]);
  const [messagePreviews, setMessagePreviews] = useState<Record<string, LinkedMessagePreview>>({});
  const [filter, setFilter] = useState<ReminderFilter>("all");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshNonce, setRefreshNonce] = useState(0);

  useEffect(() => {
    let alive = true;
    let busy = false;
    let previewCache: Record<string, LinkedMessagePreview> = {};

    const load = async (showLoading: boolean) => {
      if (busy) return;
      busy = true;
      if (showLoading) setLoading(true);
      try {
        const res = await ipc.reminderList({
          actorId: agent.actor.id,
          all: true,
        });
        const previews = await resolveLinkedMessagePreviews(
          res.reminders,
          previewCache,
        );
        if (!alive) return;
        previewCache = { ...previewCache, ...previews };
        setReminders(res.reminders);
        setMessagePreviews(previewCache);
        setError(null);
      } catch (e) {
        if (!alive) return;
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        busy = false;
        if (alive && showLoading) setLoading(false);
      }
    };

    setReminders([]);
    setMessagePreviews({});
    setError(null);
    void load(true);
    const timer = window.setInterval(() => void load(false), 8000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [agent.actor.id, refreshNonce]);

  const counts = useMemo(() => {
    const next: Record<ReminderFilter, number> = {
      all: reminders.length,
      scheduled: 0,
      fired: 0,
      cancelled: 0,
    };
    for (const reminder of reminders) next[reminder.status] += 1;
    return next;
  }, [reminders]);

  const visible = useMemo(
    () =>
      reminders.filter((reminder) =>
        filter === "all" ? true : reminder.status === filter,
      ),
    [filter, reminders],
  );

  const openLinkedScope = async (scope: ScopeRef | null | undefined) => {
    if (!scope) return;
    setView("chat");
    try {
      await openScope(scope);
    } catch (e) {
      pushToast("error", `open scope failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  return (
    <div className="min-h-full bg-white">
      <div className="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b-2 border-black bg-white px-5 py-3">
        <div className="mr-auto min-w-0">
          <div className="flex items-center gap-2 text-sm font-black">
            <BellRing size={16} />
            {counts.scheduled} scheduled
          </div>
          <div className="font-mono text-xs text-black/45">
            {reminders.length} reminder{reminders.length === 1 ? "" : "s"} for {agent.actor.displayName || agent.actor.id}
          </div>
        </div>
        <div role="radiogroup" aria-label="Filter reminders" className="flex flex-wrap gap-1">
          {REMINDER_FILTERS.map((item) => (
            <button
              key={item.id}
              role="radio"
              aria-checked={filter === item.id}
              className={clsx(
                "btn-brutal-sm gap-1 px-2 py-1 text-xs",
                filter === item.id ? "bg-brutal-yellow" : "bg-white",
              )}
              onClick={() => setFilter(item.id)}
            >
              {item.label}
              <span className="font-mono text-[10px] text-black/55">{counts[item.id]}</span>
            </button>
          ))}
        </div>
        <button
          className="btn-brutal-sm bg-white p-1.5"
          title="Refresh reminders"
          onClick={() => setRefreshNonce((value) => value + 1)}
        >
          <RefreshCw size={14} className={loading ? "animate-spin" : undefined} />
        </button>
      </div>

      {error && (
        <div className="mx-5 mt-4 border-2 border-black bg-danger px-3 py-2 text-sm font-bold">
          reminder/list failed: {error}
        </div>
      )}

      <div className="space-y-3 p-5">
        {loading && reminders.length === 0 ? (
          <div className="border-2 border-dashed border-black/30 px-5 py-8 text-center font-mono text-sm text-black/45">
            Loading reminders...
          </div>
        ) : visible.length === 0 ? (
          <div className="border-2 border-dashed border-black/30 px-5 py-8 text-center">
            <BellRing className="mx-auto mb-2 text-black/25" size={26} />
            <div className="font-black">
              {reminders.length === 0 ? "No reminders found." : `No ${filter} reminders.`}
            </div>
            <div className="mt-1 text-sm text-black/50">
              This list refreshes while the tab is open.
            </div>
          </div>
        ) : (
          visible.map((reminder) => {
            const key = reminderMessageKey(reminder);
            return (
              <ReminderCard
                key={reminder.id}
                reminder={reminder}
                channels={channels}
                threadsByChannel={threadsByChannel}
                messagePreview={key ? messagePreviews[key] : undefined}
                onOpenScope={() => openLinkedScope(reminder.scope)}
              />
            );
          })
        )}
      </div>
    </div>
  );
}

function ReminderCard({
  reminder,
  channels,
  threadsByChannel,
  messagePreview,
  onOpenScope,
}: {
  reminder: Reminder;
  channels: ChannelList;
  threadsByChannel: ThreadsByChannel;
  messagePreview?: LinkedMessagePreview;
  onOpenScope: () => void;
}) {
  const scopeInfo = describeReminderScope(reminder.scope, channels, threadsByChannel);
  const ScopeIcon = scopeInfo.kind === "thread" ? MessageSquare : Hash;

  return (
    <article className="border-2 border-black bg-brutal-cream p-4 shadow-brutal-sm">
      <div className="flex flex-wrap items-center gap-2">
        <span className={clsx("chip-brutal", reminderStatusClass(reminder.status))}>
          {reminder.status}
        </span>
        <span className="font-mono text-[11px] text-black/45">{reminder.id}</span>
        {reminder.repeat && (
          <span className="chip-brutal bg-white">
            <Clock3 size={11} />
            {reminder.repeat}
          </span>
        )}
      </div>

      <div className="mt-3 text-base font-black leading-6">{reminder.title}</div>

      <div className="mt-3 grid gap-2 lg:grid-cols-2">
        <ReminderFact icon={CalendarClock} label="Fire At">
          <div className="font-bold">{formatDateTime(reminder.fireAt)}</div>
          <div className="font-mono text-[11px] text-black/45">{formatRelativeTime(reminder.fireAt)}</div>
        </ReminderFact>
        <ReminderFact icon={ScopeIcon} label="Scope">
          <div className="font-bold">{scopeInfo.title}</div>
          <div className="font-mono text-[11px] text-black/45">{scopeInfo.subtitle}</div>
        </ReminderFact>
      </div>

      <div className="mt-3 border-2 border-black bg-white p-3">
        <div className="mb-1 flex items-center gap-2 text-xs font-black uppercase tracking-widest text-black/45">
          <MessageSquare size={13} />
          Linked Message
        </div>
        {reminder.msgId ? (
          <>
            <div className="break-all font-mono text-[11px] text-black/50">{reminder.msgId}</div>
            <div className={clsx("mt-2 text-sm", messagePreview?.missing ? "text-black/45" : "text-black")}>
              {messagePreview?.text ?? "Message preview unavailable until this scope has recent history."}
            </div>
            {messagePreview?.actorId && (
              <div className="mt-1 font-mono text-[11px] text-black/45">
                by {messagePreview.actorId}
                {messagePreview.occurredAt ? ` at ${formatDateTime(messagePreview.occurredAt)}` : ""}
              </div>
            )}
          </>
        ) : (
          <div className="text-sm text-black/45">No message id linked to this reminder.</div>
        )}
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <span className="font-mono text-[11px] text-black/45">
          created {formatDateTime(reminder.createdAt)}
        </span>
        {reminder.lastFiredAt && (
          <span className="font-mono text-[11px] text-black/45">
            last fired {formatDateTime(reminder.lastFiredAt)}
          </span>
        )}
        {reminder.scope && (
          <button
            className="btn-brutal-sm ml-auto gap-1 bg-white px-3 py-1.5 text-xs"
            onClick={() => void onOpenScope()}
          >
            Open Scope
          </button>
        )}
      </div>
    </article>
  );
}

function ReminderFact({
  icon: Icon,
  label,
  children,
}: {
  icon: LucideIcon;
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="flex min-w-0 gap-2 border-2 border-black bg-white p-3">
      <Icon size={15} className="mt-0.5 shrink-0" />
      <div className="min-w-0">
        <div className="text-xs font-black uppercase tracking-widest text-black/45">{label}</div>
        <div className="min-w-0 break-words text-sm">{children}</div>
      </div>
    </div>
  );
}

async function resolveLinkedMessagePreviews(
  reminders: Reminder[],
  existing: Record<string, LinkedMessagePreview> = {},
): Promise<Record<string, LinkedMessagePreview>> {
  const previews: Record<string, LinkedMessagePreview> = {};
  const pending = new Map<string, { scope: ScopeRef; ids: Set<string> }>();

  for (const reminder of reminders) {
    const key = reminderMessageKey(reminder);
    if (!key || !reminder.scope || !reminder.msgId) continue;
    if (existing[key] && !existing[key].missing) {
      previews[key] = existing[key];
      continue;
    }

    const cached = readCachedMessagePreview(reminder);
    if (cached) {
      previews[key] = cached;
      continue;
    }

    const scopeId = scopeKey(reminder.scope);
    const group = pending.get(scopeId) ?? {
      scope: reminder.scope,
      ids: new Set<string>(),
    };
    group.ids.add(reminder.msgId);
    pending.set(scopeId, group);
  }

  await Promise.all(
    [...pending.values()].map(async ({ scope, ids }) => {
      const found = new Set<string>();
      try {
        const res = await ipc.scopeRead(scope, 100);
        for (const event of res.events) {
          if (!ids.has(event.id)) continue;
          found.add(event.id);
          previews[`${scopeKey(scope)}:${event.id}`] = previewFromEvent(event);
        }
      } catch {
        // Scope visibility can differ from the reminder owner. Keep the
        // association visible even if the preview cannot be hydrated.
      }

      for (const id of ids) {
        if (found.has(id)) continue;
        previews[`${scopeKey(scope)}:${id}`] = {
          text: "Message preview unavailable; showing the linked event id above.",
          missing: true,
        };
      }
    }),
  );

  return previews;
}

function readCachedMessagePreview(reminder: Reminder): LinkedMessagePreview | null {
  if (!reminder.scope || !reminder.msgId) return null;
  const scopeState = useMessages.getState().byScope[scopeKey(reminder.scope)];
  const bubble = scopeState?.bubbles.find((item) => item.id === reminder.msgId);
  if (!bubble) return null;
  return {
    actorId: bubble.actorId,
    text: compactPreview(bubble.text),
    occurredAt: bubble.ts,
  };
}

function previewFromEvent(event: JoiEvent): LinkedMessagePreview {
  const payload = (event.payload ?? {}) as Record<string, unknown>;
  let text = asString(payload.text);

  if (event.type === "action.request") {
    const summary = summarizeActionRequest(payload);
    text = [summary.title, summary.description].filter(Boolean).join(" - ");
  } else if (!text) {
    text = asString(payload.title) || asString(payload.body) || event.type;
  }

  return {
    actorId: event.actorId,
    text: compactPreview(text),
    occurredAt: event.occurredAt,
  };
}

function reminderMessageKey(reminder: Reminder): string | null {
  if (!reminder.scope || !reminder.msgId) return null;
  return `${scopeKey(reminder.scope)}:${reminder.msgId}`;
}

function describeReminderScope(
  scope: ScopeRef | null | undefined,
  channels: ChannelList,
  threadsByChannel: ThreadsByChannel,
): { kind: ScopeRef["kind"] | "none"; title: string; subtitle: string } {
  if (!scope) {
    return {
      kind: "none",
      title: "No scope",
      subtitle: "fires without a channel/thread event",
    };
  }

  if (scope.kind === "channel") {
    const channel = channels.find((item) => item.id === scope.id);
    return {
      kind: "channel",
      title: channel ? `#${channel.title}` : `#${scope.id}`,
      subtitle: `channel ${scope.id}`,
    };
  }

  for (const [channelId, threads] of Object.entries(threadsByChannel)) {
    const thread = threads.find((item) => item.id === scope.id);
    if (!thread) continue;
    const channel = channels.find((item) => item.id === channelId);
    return {
      kind: "thread",
      title: thread.title,
      subtitle: channel
        ? `thread ${scope.id} in #${channel.title}`
        : `thread ${scope.id} in channel ${channelId}`,
    };
  }

  return {
    kind: "thread",
    title: scope.id,
    subtitle: `thread ${scope.id}`,
  };
}

function reminderStatusClass(status: ReminderStatus): string {
  switch (status) {
    case "scheduled":
      return "bg-brutal-yellow";
    case "fired":
      return "bg-brutal-lime";
    case "cancelled":
      return "bg-black text-white";
  }
}

function formatDateTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString([], {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatRelativeTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const diffMs = date.getTime() - Date.now();
  const absSeconds = Math.max(1, Math.round(Math.abs(diffMs) / 1000));
  const text = formatDuration(absSeconds);
  if (absSeconds < 30) return "now";
  return diffMs >= 0 ? `in ${text}` : `${text} ago`;
}

function formatDuration(seconds: number): string {
  const units = [
    { label: "d", value: 86400 },
    { label: "h", value: 3600 },
    { label: "m", value: 60 },
  ];
  for (const unit of units) {
    if (seconds >= unit.value) {
      return `${Math.round(seconds / unit.value)}${unit.label}`;
    }
  }
  return `${seconds}s`;
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function compactPreview(value: string): string {
  const normalized = value.replace(/\s+/g, " ").trim();
  if (normalized.length <= 180) return normalized || "Empty message";
  return `${normalized.slice(0, 177)}...`;
}

function ProfileTab({
  agent,
  online,
  onUpdate,
}: {
  agent: ManagedAgent;
  online: boolean;
  onUpdate: (patch: AgentUpdatePatch) => Promise<void>;
}) {
  const ui = useUI();
  const canEdit = agent.managed;
  return (
    <div>
      <section className="flex gap-4 border-b border-black/10 p-5">
        <PixelAvatar id={agent.actor.id} label={agent.actor.displayName} size={64} />
        <div>
          <div className="text-xl font-black">
            {agent.actor.displayName || agent.actor.id}{" "}
            <span className={clsx("inline-block h-2.5 w-2.5 rounded-full border border-black", online ? "bg-brutal-lime" : "bg-black/20")} />
            <span className="ml-1 font-mono text-sm font-normal text-black/50">
              {online ? "Online" : agent.status}
            </span>
          </div>
          <div className="font-mono text-sm text-black/45">@{agent.actor.id.replace(/^actor_/, "")}</div>
        </div>
      </section>

      <InfoSection
        title="Display Name"
        action={canEdit ? "Edit display name" : undefined}
      >
        {canEdit ? (
          <button
            className="inline-flex items-center gap-2 hover:underline"
            onClick={() =>
              ui.openModal({
                type: "input",
                title: "Edit display name",
                label: "Display name",
                initial: agent.actor.displayName || agent.actor.id,
                confirmLabel: "Save",
                onSubmit: (displayName) => onUpdate({ displayName }),
              })
            }
          >
            {agent.actor.displayName || agent.actor.id}
            <Edit3 size={13} className="text-black/40" />
          </button>
        ) : (
          <span>{agent.actor.displayName || agent.actor.id}</span>
        )}
      </InfoSection>

      <InfoSection
        title="Description"
        action={canEdit ? "Edit description" : undefined}
      >
        {canEdit ? (
          <button
            className="inline-flex items-center gap-2 text-black/55 hover:text-black"
            onClick={() =>
              ui.openModal({
                type: "input",
                title: "Edit description",
                label: "Description",
                initial: agent.description,
                confirmLabel: "Save",
                onSubmit: (description) => onUpdate({ description }),
              })
            }
          >
            {agent.description || "No description"}
            <Edit3 size={13} className="text-black/40" />
          </button>
        ) : (
          <span className="text-black/55">{agent.description || "No description"}</span>
        )}
      </InfoSection>

      <ActorProfileSection agent={agent} />

      <InfoSection title="Info">
        <div className="grid max-w-2xl grid-cols-2 gap-5 text-sm">
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Machine
            </div>
            <span>{agent.machine}</span>{" "}
            <span className="font-mono text-xs text-black/45">connected</span>
          </div>
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Creator
            </div>
            <span className="font-black">{agent.creator}</span>
          </div>
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Created
            </div>
            <span>{agent.created}</span>
          </div>
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Command
            </div>
            <span className="font-mono text-xs">{[agent.command, ...agent.args].join(" ")}</span>
          </div>
        </div>
      </InfoSection>

      <RuntimeConfigSection agent={agent} onUpdate={onUpdate} />

      <InfoSection title="Environment Variables" action="Edit environment variables">
        {agent.env.length === 0 ? (
          <span className="italic text-black/45">No environment variables configured</span>
        ) : (
          <div className="space-y-1">
            {agent.env.map((row) => (
              <div key={row.key} className="font-mono text-xs">
                {row.key}=<span className="text-black/45">{row.value}</span>
              </div>
            ))}
          </div>
        )}
      </InfoSection>

      <InfoSection title="Created Agents (0)">
        <span className="italic text-black/45">No created agents</span>
      </InfoSection>
    </div>
  );
}

type ProfileFileKind = "identity" | "soul";

function ActorProfileSection({ agent }: { agent: ManagedAgent }) {
  const pushToast = useUI((s) => s.pushToast);
  const [editing, setEditing] = useState<ProfileFileKind | null>(null);

  if (!agent.managed || !agent.profilePath) {
    return (
      <InfoSection title="Actor Profile">
        <span className="italic text-black/45">
          Local profile files are only available for machine-managed agents.
        </span>
      </InfoSection>
    );
  }

  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      pushToast("info", `${label} copied`);
    } catch {
      pushToast("info", value);
    }
  };

  const openProfile = async () => {
    if (!agent.canOpenLocalPath) {
      pushToast("info", "Remote profile paths can be copied but not opened locally");
      return;
    }
    try {
      await ipc.openLocalPath(agent.profilePath);
      pushToast("info", "profile opened");
    } catch (e) {
      pushToast("error", e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <InfoSection title="Actor Profile">
      <div className="max-w-4xl space-y-2">
        <ProfilePathRow
          label="Profile"
          value={agent.profilePath}
          onCopy={() => void copy(agent.profilePath, "profile path")}
          onOpen={agent.canOpenLocalPath ? () => void openProfile() : undefined}
        />
        <ProfilePathRow
          label="Identity"
          value={agent.identityPath}
          onCopy={() => void copy(agent.identityPath, "identity.md path")}
          onEdit={() => setEditing("identity")}
        />
        <ProfilePathRow
          label="Soul"
          value={agent.soulPath}
          onCopy={() => void copy(agent.soulPath, "soul.md path")}
          onEdit={() => setEditing("soul")}
        />
      </div>
      {editing && (
        <ProfileFileEditor
          agent={agent}
          file={editing}
          onClose={() => setEditing(null)}
        />
      )}
    </InfoSection>
  );
}

function ProfilePathRow({
  label,
  value,
  onCopy,
  onOpen,
  onEdit,
}: {
  label: string;
  value: string;
  onCopy: () => void;
  onOpen?: () => void;
  onEdit?: () => void;
}) {
  return (
    <div className="grid min-w-0 gap-2 md:grid-cols-[7rem_minmax(0,1fr)_auto]">
      <div className="flex items-center gap-2 text-xs font-black uppercase tracking-widest text-black/45">
        {label === "Profile" ? <Folder size={13} /> : <FileText size={13} />}
        {label}
      </div>
      <div className="min-w-0 border border-black/20 bg-brutal-cream px-2 py-1.5 font-mono text-xs text-black/70">
        <div className="truncate">{value}</div>
      </div>
      <div className="flex flex-wrap gap-1">
        {onOpen && (
          <button
            className="btn-brutal-sm bg-white p-1.5"
            title="Open profile folder"
            onClick={onOpen}
          >
            <FolderOpen size={12} />
          </button>
        )}
        <button
          className="btn-brutal-sm bg-white p-1.5"
          title={`Copy ${label.toLowerCase()} path`}
          onClick={onCopy}
        >
          <Copy size={12} />
        </button>
        {onEdit && (
          <button
            className="btn-brutal-sm gap-1 bg-white px-2 py-1 text-[11px]"
            onClick={onEdit}
          >
            <Edit3 size={12} /> Edit
          </button>
        )}
      </div>
    </div>
  );
}

function ProfileFileEditor({
  agent,
  file,
  onClose,
}: {
  agent: ManagedAgent;
  file: ProfileFileKind;
  onClose: () => void;
}) {
  const pushToast = useUI((s) => s.pushToast);
  const [text, setText] = useState("");
  const [sha256, setSha256] = useState<string | null>(null);
  const [path, setPath] = useState(file === "identity" ? agent.identityPath : agent.soulPath);
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    let alive = true;
    setBusy(true);
    ipc
      .agentProfileFileRead({
        machineId: agent.machineId,
        actorId: agent.actor.id,
        file,
      })
      .then((result) => {
        if (!alive) return;
        setText(result.text);
        setPath(result.path);
        setSha256(result.sha256 ?? null);
      })
      .catch((e) => {
        if (!alive) return;
        pushToast("error", e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (alive) setBusy(false);
      });
    return () => {
      alive = false;
    };
  }, [agent.actor.id, agent.machineId, file, pushToast]);

  const save = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await ipc.agentProfileFileWrite({
        machineId: agent.machineId,
        actorId: agent.actor.id,
        file,
        text,
        baseSha256: sha256,
      });
      pushToast("info", `${file}.md saved`);
      setPath(result.path);
      setSha256(result.sha256 ?? sha256);
      onClose();
    } catch (e) {
      pushToast("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/60 p-4">
      <section className="card-brutal w-[calc(100vw-2rem)] max-w-3xl p-5">
        <div className="mb-3 flex items-center justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-lg font-black uppercase">
              Edit {file === "identity" ? "Identity" : "Soul"}
            </h2>
            <div className="truncate font-mono text-xs text-black/45">{path}</div>
          </div>
          <button className="btn-brutal-sm bg-white p-1" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <textarea
          className="h-[min(58vh,32rem)] w-full resize-none border-2 border-black bg-brutal-cream p-3 font-mono text-xs leading-5 outline-none focus:bg-white"
          value={text}
          disabled={busy}
          onChange={(e) => setText(e.target.value)}
        />
        <div className="mt-3 flex justify-end gap-2">
          <button className="btn-brutal bg-white px-4 py-2 text-sm" onClick={onClose}>
            Cancel
          </button>
          <button
            className="btn-brutal bg-brutal-pink px-4 py-2 text-sm disabled:bg-black/10"
            disabled={busy}
            onClick={() => void save()}
          >
            Save
          </button>
        </div>
      </section>
    </div>
  );
}

function RuntimeConfigSection({
  agent,
  onUpdate,
}: {
  agent: ManagedAgent;
  onUpdate: (patch: AgentUpdatePatch) => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [providerId, setProviderId] = useState(agent.providerId);
  const [model, setModel] = useState(agent.model === "Default" ? "" : agent.model);
  const [reasoningEffort, setReasoningEffort] = useState(agent.reasoningEffort);
  const [autostart, setAutostart] = useState(agent.autostart);
  const selectedProvider =
    agent.providers.find((provider) => provider.id === providerId) ??
    agent.providers.find((provider) => provider.id === agent.providerId);
  const modelChoices = modelChoicesForProvider(selectedProvider);

  useEffect(() => {
    setProviderId(agent.providerId);
    setModel(agent.model === "Default" ? "" : agent.model);
    setReasoningEffort(agent.reasoningEffort);
    setAutostart(agent.autostart);
  }, [agent.actor.id, agent.providerId, agent.model, agent.reasoningEffort, agent.autostart]);

  const save = async () => {
    await onUpdate({
      providerId,
      model,
      reasoningEffort,
      autostart,
    });
    setEditing(false);
  };

  return (
    <section className="border-b border-black/10 px-5 py-4">
      <div className="mb-2 flex items-center gap-2 text-xs font-black uppercase tracking-widest text-black/45">
        Runtime Configuration
        {agent.managed && !editing && (
          <button title="Edit runtime configuration" onClick={() => setEditing(true)}>
            <Edit3 size={13} />
          </button>
        )}
      </div>
      {!editing ? (
        <div className="flex flex-wrap gap-2">
          <span className="chip-brutal bg-brutal-cyan">{agent.provider}</span>
          <span className="chip-brutal bg-brutal-yellow">{agent.model}</span>
          {agent.reasoningEffort && (
            <span className="chip-brutal bg-brutal-lavender">
              {agent.reasoningEffort}
            </span>
          )}
          <span className="chip-brutal bg-white">
            {agent.autostart ? "autostart" : "manual"}
          </span>
        </div>
      ) : (
        <div className="max-w-xl space-y-3">
          <Field label="Runtime">
            <SelectLike
              value={selectedProvider?.name || providerId}
              options={agent.providers.map((provider) => ({
                id: provider.id,
                label: provider.name || provider.id,
              }))}
              onPick={setProviderId}
            />
          </Field>
          {modelChoices.length > 0 ? (
            <Field label="Model">
              <SelectLike
                value={
                  modelChoices.find((choice) => choice.id === model)?.label ||
                  model ||
                  "Default"
                }
                options={modelChoices}
                onPick={setModel}
              />
            </Field>
          ) : (
            <Field label="Model" optional>
              <input
                className="input-brutal w-full"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              />
            </Field>
          )}
          {providerId === "codex" && (
            <Field label="Reasoning">
              <SelectLike
                value={reasoningEffort || "medium"}
                options={REASONING_CHOICES.map((id) => ({ id, label: id }))}
                onPick={setReasoningEffort}
              />
            </Field>
          )}
          <label className="flex items-center gap-2 text-sm font-bold">
            <input
              type="checkbox"
              checked={autostart}
              onChange={(e) => setAutostart(e.target.checked)}
              className="h-4 w-4 accent-black"
            />
            Autostart
          </label>
          <div className="flex gap-2">
            <button
              className="btn-brutal-sm bg-brutal-pink px-3 py-1.5 text-xs"
              onClick={() => void save()}
            >
              Save
            </button>
            <button
              className="btn-brutal-sm bg-white px-3 py-1.5 text-xs"
              onClick={() => setEditing(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

function InfoSection({
  title,
  action,
  children,
}: {
  title: string;
  action?: string;
  children: ReactNode;
}) {
  return (
    <section className="border-b border-black/10 px-5 py-4">
      <div className="mb-2 flex items-center gap-2 text-xs font-black uppercase tracking-widest text-black/45">
        {title}
        {action && (
          <button title={action} className="hover:text-black">
            <Edit3 size={13} />
          </button>
        )}
      </div>
      <div className="text-sm">{children}</div>
    </section>
  );
}

function Field({
  label,
  optional,
  children,
}: {
  label: string;
  optional?: boolean;
  children: ReactNode;
}) {
  return (
    <div>
      <label className="mb-1 block text-sm font-black uppercase tracking-wide">
        {label}
        {optional && <span className="text-black/40 normal-case">(optional)</span>}
      </label>
      {children}
    </div>
  );
}

function SelectLike({
  value,
  options,
  onPick,
}: {
  value: string;
  options: Array<{ id: string; label?: string | null }>;
  onPick: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="relative">
      <button
        className="input-brutal flex w-full items-center justify-between gap-2 text-sm"
        onClick={() => setOpen((value) => !value)}
      >
        <span className="truncate">{value}</span>
        <ChevronDown size={14} />
      </button>
      {open && (
        <div className="absolute left-0 right-0 top-full z-10 mt-1 border-2 border-black bg-white shadow-brutal">
          {options.map((option) => (
            <button
              key={option.id}
              className="block w-full px-3 py-2 text-left text-sm font-bold hover:bg-brutal-yellow"
              onClick={() => {
                onPick(option.id);
                setOpen(false);
              }}
            >
              {option.label || option.id}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function EmptyTab({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-[22rem] items-center justify-center font-mono text-sm text-black/40">
      {label}
    </div>
  );
}

function WorkspaceTab({ agent }: { agent: ManagedAgent }) {
  return (
    <div className="p-5">
      <InfoSection title="Machine">
        <div className="grid max-w-3xl grid-cols-2 gap-5 text-sm">
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Machine
            </div>
            <span>{agent.machine}</span>
          </div>
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Provider
            </div>
            <span>{agent.provider}</span>
          </div>
        </div>
      </InfoSection>
      <InfoSection title="Paths">
        <div className="space-y-2 font-mono text-xs">
          <div className="truncate">
            <span className="text-black/45">data </span>
            {agent.dataRoot}
          </div>
          <div className="truncate">
            <span className="text-black/45">profile </span>
            {agent.profilePath}
          </div>
          <div className="truncate">
            <span className="text-black/45">identity </span>
            {agent.identityPath}
          </div>
          <div className="truncate">
            <span className="text-black/45">soul </span>
            {agent.soulPath}
          </div>
        </div>
      </InfoSection>
      <InfoSection title="Transport">
        <div className="space-y-2 font-mono text-xs">
          <div>{agent.runtime}</div>
          <div className="break-all">{[agent.command, ...agent.args].join(" ")}</div>
        </div>
      </InfoSection>
    </div>
  );
}

function ActivityTab({ agent }: { agent: ManagedAgent }) {
  return (
    <div className="p-5">
      <InfoSection title="Connection">
        <div className="grid max-w-2xl grid-cols-2 gap-5 text-sm">
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Status
            </div>
            <span>{agent.status}</span>
          </div>
          <div>
            <div className="mb-1 text-xs font-black uppercase tracking-widest text-black/45">
              Autostart
            </div>
            <span>{agent.autostart ? "enabled" : "disabled"}</span>
          </div>
        </div>
      </InfoSection>
    </div>
  );
}

function groupMembersByKind(members: ManagedAgent[]) {
  const byKind = new Map<ActorKind, ManagedAgent[]>();
  for (const member of members) {
    const rows = byKind.get(member.actor.kind) ?? [];
    rows.push(member);
    byKind.set(member.actor.kind, rows);
  }
  return [...byKind.entries()]
    .sort(([left], [right]) => actorKindOrder(left) - actorKindOrder(right))
    .map(([kind, rows]) => ({
      kind,
      members: rows.sort((a, b) =>
        (a.actor.displayName || a.actor.id).localeCompare(
          b.actor.displayName || b.actor.id,
        ),
      ),
    }));
}

function actorKindOrder(kind: ActorKind): number {
  switch (kind) {
    case "agent":
      return 0;
    case "service":
      return 1;
    case "human":
      return 2;
  }
}

function actorKindPlural(kind: ActorKind): string {
  switch (kind) {
    case "agent":
      return "Agents";
    case "service":
      return "Services";
    case "human":
      return "Humans";
  }
}

function normalizeAgent(
  info: AgentInfo | MachineAgentInfo,
  machine: AgentMachineContext,
): ManagedAgent {
  const spec = info.spec;
  const actor = spec.actor;
  const meta = actor._meta ?? {};
  const env = spec.transport.env ?? {};
  const model = spec.models?.default ?? spec.transport.model ?? "";
  const reasoningEffort =
    typeof meta.reasoningEffort === "string" ? meta.reasoningEffort : "";
  const providerId =
    typeof meta.providerId === "string" ? meta.providerId : "unknown";
  const provider =
    typeof meta.providerName === "string" ? meta.providerName : providerId;
  const runtime =
    typeof meta.transportKind === "string"
      ? meta.transportKind
      : spec.transport.kind === "interactive_command"
        ? "Interactive command"
        : spec.transport.kind;
  return {
    actor,
    managed: !machine.readOnly,
    canOpenLocalPath: machine.canOpenLocalPath,
    status: info.status,
    machineId: machine.id,
    machine: machine.name,
    dataRoot: machine.dataRoot,
    profilePath:
      "profilePath" in info
        ? info.profilePath
        : machine.profilePath ?? displayJoin(machine.dataRoot, "agents", actor.id, "profile"),
    identityPath:
      "identityPath" in info
        ? info.identityPath
        : machine.identityPath ??
          displayJoin(machine.dataRoot, "agents", actor.id, "profile", "identity.md"),
    soulPath:
      "soulPath" in info
        ? info.soulPath
        : machine.soulPath ??
          displayJoin(machine.dataRoot, "agents", actor.id, "profile", "soul.md"),
    providerId,
    provider,
    providers: machine.providers,
    runtime,
    model: model || "Default",
    reasoningEffort,
    description: spec.identity?.description ?? "",
    creator: "local spec",
    created: "registered",
    env: Object.entries(env).map(([key, value]) => ({ key, value })),
    command: spec.transport.command,
    args: spec.transport.args ?? [],
    autostart: !!spec.autostart,
  };
}

function normalizeRegistryActor(actor: Actor): ManagedAgent {
  const meta = actor._meta ?? {};
  const description =
    typeof meta.description === "string"
      ? meta.description
      : `Registered server ${actor.kind}`;
  return {
    actor,
    managed: false,
    canOpenLocalPath: false,
    status: "registered",
    machineId: "",
    machine: "Server registry",
    dataRoot: "",
    profilePath: "",
    identityPath: "",
    soulPath: "",
    providerId: "registry",
    provider: "Registry",
    providers: [],
    runtime: "server actor",
    model: "n/a",
    reasoningEffort: "",
    description,
    creator: "server",
    created: "",
    env: [],
    command: "n/a",
    args: [],
    autostart: false,
  };
}

function displayJoin(root: string, ...parts: string[]) {
  return [root.replace(/\/+$/, ""), ...parts].join("/");
}

function modelChoicesForProvider(provider: AgentProviderSummary | null | undefined) {
  if (!provider) return [];
  if (provider.modelChoices?.length) return provider.modelChoices;
  if (provider.id === "codex") return CODEX_MODELS;
  return [];
}
