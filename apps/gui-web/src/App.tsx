import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type {
  ComponentType,
  CSSProperties,
  FormEvent,
  MouseEvent,
  PointerEvent,
  ReactNode,
} from "react";
import { createPortal } from "react-dom";
import ReactMarkdown from "react-markdown";
import {
  Bell,
  Bot,
  Check,
  ChevronDown,
  Clock,
  FileText,
  Folder,
  Github,
  GripVertical,
  Hash,
  HardDrive,
  Home,
  Lock,
  Loader2,
  LogOut,
  MessageSquare,
  Pencil,
  Plus,
  RefreshCw,
  Reply,
  Search,
  Send,
  Server,
  Smile,
  Split,
  Trash2,
  UserPlus,
  Users,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import {
  channelTarget,
  scopeKey,
  threadTarget,
  type Actor,
  type AudienceRef,
  type Channel,
  type DesktopConfig,
  type HumanAccount,
  type InboxListEntry,
  type MachineAgentProviderInfo,
  type MachineInfo,
  type Message,
  type Run,
  type ScopeRef,
  type StreamUpdate,
  type Task,
  type Thread,
  type Workspace,
} from "@/ipc/types";
import { Badge } from "@/components/ui/badge";
import {
  AgentIdentityBadge,
  type AgentIdentityBadgeProps,
} from "@/components/agent/AgentIdentityBadge";
import {
  AgentProviderIcon,
  agentProviderIconKey,
} from "@/components/agent/AgentProviderIcon";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { cn, formatTime, shortId } from "@/lib/utils";

type ConnectionState = "idle" | "connecting" | "open" | "closed" | "error";
type View = "chat" | "threads" | "channels" | "inbox" | "tasks" | "spaces" | "account" | "settings";
type ChannelPanelTab = "threads" | "members" | "tasks";
type ChannelGroup = {
  id: string;
  title: string;
  channelIds: string[];
  collapsed: boolean;
};
type ChannelGroupSection = {
  id: string;
  title: string;
  channels: Channel[];
  collapsed: boolean;
  local: boolean;
};
type ThreadWithChannel = Thread & {
  channel: Channel;
};
type ActionChoice = {
  id: string;
  label: string;
  accepted: boolean;
  votes?: number;
};
type BodyPoll = {
  question: string;
  choices: ActionChoice[];
};
type ChannelPointerDrag = {
  channelId: string;
  startX: number;
  startY: number;
  pointerId: number;
  dragging: boolean;
};
type ChannelContextMenu = {
  channelId: string;
  x: number;
  y: number;
};
type PanelResizeKind = "sidebar" | "detail";
type PanelSizes = {
  sidebar: number;
  detail: number;
};
type PanelResizeDrag = {
  kind: PanelResizeKind;
  startX: number;
  sidebar: number;
  detail: number;
};
type ThreadActivityStats = {
  replyCount: number;
  replyMessageIds: string[];
  participantActorIds: string[];
  hasMoreReplies: boolean;
  lastReplyAt: string | null;
};
type AgentFormState = {
  machineId: string;
  providerId: string;
  actorId: string;
  name: string;
  description: string;
  model: string;
  autostart: boolean;
};
type AgentUpdatePatch = {
  machineId: string;
  actorId: string;
  displayName: string;
  description: string;
  providerId?: string;
  model: string;
  reasoningEffort: string;
  autostart: boolean;
  avatarUrl: string;
};
type AgentSettingsDraft = {
  displayName: string;
  description: string;
  providerId: string;
  model: string;
  reasoningEffort: string;
  autostart: boolean;
  avatarUrl: string;
};
type AgentMemberEntry = {
  machine: MachineInfo;
  agent: MachineInfo["agents"][number];
};
type ChannelMemberPresence = {
  online: boolean;
  label: string;
  status: string;
};
type ChannelMemberPanelItem = {
  actor: Actor;
  presence: ChannelMemberPresence;
};

const supportedReactionEmojis = ["👍", "👀", "✅", "🥳", "💔"];
const avatarCount = 25;
const agentAvatarIndexes = [1, 5, 10, 15, 20, 25] as const;
const agentMessageBadgePopoverScale = 0.5;
const agentMessageBadgeCompactWidth = 256;
const agentMessageBadgeDetailWidth = 668;
const agentMessageBadgeCompactHeight = 490;
const agentMessageBadgeDetailHeight = 1352;
const avatarLibraryUrls = Array.from(
  { length: avatarCount },
  (_, index) => `/avatars/avatar-${String(index + 1).padStart(2, "0")}.png`,
);
const reasoningEffortChoices = ["", "minimal", "low", "medium", "high", "xhigh"] as const;
const ungroupedChannelGroupId = "__ungrouped";
const channelContextMenuWidthPx = 44 * 4;
const channelContextMenuItemHeightPx = 36;
const channelContextMenuItemCount = 2;
const channelContextMenuPaddingPx = 4;
const channelContextMenuBorderPx = 1;
const channelContextMenuViewportPaddingPx = 8;
const channelContextMenuHeightPx =
  channelContextMenuPaddingPx * 2 +
  channelContextMenuItemHeightPx * channelContextMenuItemCount +
  channelContextMenuBorderPx * 2;
const panelLayoutStorageKey = "loom:panel-layout:v1";
const detailPanelBreakpoint = 1280;
const railWidth = 72;
const resizeHandleWidth = 8;
const sidebarMinWidth = 216;
const sidebarMaxWidth = 420;
const detailMinWidth = 280;
const detailMaxWidth = 560;
const mainMinWidth = 360;
const defaultPanelSizes: PanelSizes = {
  sidebar: 286,
  detail: 340,
};

export function App() {
  const [config, setConfig] = useState<DesktopConfig>({
    workspaces: [],
    account: null,
  });
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("idle");
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("chat");
  const [settingsAgentId, setSettingsAgentId] = useState<string | null>(null);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [channelGroups, setChannelGroups] = useState<ChannelGroup[]>(() =>
    loadChannelGroups(channelGroupStorageKey(null)),
  );
  const [threadsByChannel, setThreadsByChannel] = useState<Record<string, Thread[]>>({});
  const [activeChannelId, setActiveChannelId] = useState<string | null>(null);
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [threadMessages, setThreadMessages] = useState<Message[]>([]);
  const [threadStatsById, setThreadStatsById] = useState<Record<string, ThreadActivityStats>>({});
  const [actors, setActors] = useState<Record<string, Actor>>({});
  const [, setRuns] = useState<Record<string, Run>>({});
  const [inbox, setInbox] = useState<InboxListEntry[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [machines, setMachines] = useState<MachineInfo[]>([]);
  const [draft, setDraft] = useState("");
  const [threadDraft, setThreadDraft] = useState("");
  const [workspaceForm, setWorkspaceForm] = useState({
    name: "Local",
    serverUrl: "ws://127.0.0.1:7878/rpc",
  });
  const [machineForm, setMachineForm] = useState({
    name: "Local Host",
    dataRoot: "",
  });
  const [agentForm, setAgentForm] = useState<AgentFormState>({
    machineId: "",
    providerId: "",
    actorId: "",
    name: "Echo",
    description: "Reply concisely and report completed work.",
    model: "",
    autostart: true,
  });
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [replyTo, setReplyTo] = useState<Message | null>(null);
  const [channelPanelTab, setChannelPanelTab] = useState<ChannelPanelTab | null>(null);
  const [panelSizes, setPanelSizes] = useState<PanelSizes>(() => loadPanelSizes());
  const [viewportWidth, setViewportWidth] = useState(() => initialViewportWidth());
  const [resizingPanel, setResizingPanel] = useState<PanelResizeKind | null>(null);

  const activeScopeRef = useRef<ScopeRef | null>(null);
  const activeThreadScopeRef = useRef<ScopeRef | null>(null);
  const actorIdRef = useRef<string | null>(null);
  const targetRef = useRef<string | null>(null);
  const workspaceRef = useRef<Workspace | null>(null);
  const autoReconnectRef = useRef(false);
  const reconnectTimerRef = useRef<number | null>(null);
  const reconnectAttemptRef = useRef(0);
  const panelResizeDragRef = useRef<PanelResizeDrag | null>(null);
  const panelResizeCleanupRef = useRef<(() => void) | null>(null);

  const account = config.account ?? null;
  const workspaces = config.workspaces ?? [];
  const activeChannel = channels.find((channel) => channel.id === activeChannelId) ?? null;
  const channelGroupsKey = channelGroupStorageKey(workspace);
  const channelThreads = activeChannel
    ? threadsByChannel[activeChannel.id] ?? []
    : [];
  const activeThread =
    channelThreads.find((thread) => thread.id === activeThreadId) ?? null;
  const target = activeChannel ? channelTarget(activeChannel.id) : null;
  const threadMessageTarget = activeThread ? threadTarget(activeThread) : null;
  const activeScope: ScopeRef | null = activeChannel
    ? { kind: "channel", id: activeChannel.id }
    : null;
  const activeThreadScope: ScopeRef | null = activeThread
    ? { kind: "thread", id: activeThread.id }
    : null;
  const allThreads = flattenThreads(threadsByChannel, channels);
  const channelIdsKey = channels.map((channel) => channel.id).join("|");
  const actorList = Object.values(actors).sort((a, b) =>
    displayName(a).localeCompare(displayName(b)),
  );
  const agentActors = actorList.filter((actor) => actor.kind === "agent");
  const memberCandidates = actorList.filter((actor) => actor.kind !== "service");
  const channelAgentActors = activeChannel
    ? agentActors.filter((actor) => isChannelMember(activeChannel, actor.id))
    : [];
  const channelTasks = activeChannel
    ? tasks.filter((task) => task.channelId === activeChannel.id)
    : tasks;
  const tasksBySourceMessageId: Record<string, Task> = Object.fromEntries(
    tasks.map((task) => [task.sourceMessageId, task]),
  );
  const activeThreadTask = activeThread
    ? tasksBySourceMessageId[activeThread.rootMessageId] ?? null
    : null;

  const openAgentSettings = useCallback((actorId: string) => {
    setSettingsAgentId(actorId);
    setView("settings");
  }, []);

  const applyConfig = useCallback((next: DesktopConfig) => {
    setConfig(next);
    const active = next.active
      ? next.workspaces.find((candidate) => candidate.id === next.active)
      : next.workspaces[0];
    if (active) {
      setWorkspace((current) => {
        const selected =
          current && next.workspaces.some((candidate) => candidate.id === current.id)
            ? current
            : active;
        workspaceRef.current = selected;
        return selected;
      });
    } else {
      workspaceRef.current = null;
      setWorkspace(null);
    }
  }, []);

  const pushNotice = useCallback((text: string) => {
    setNotice(text);
    window.setTimeout(() => setNotice(null), 3200);
  }, []);

  const clearReconnectTimer = useCallback(() => {
    if (reconnectTimerRef.current !== null) {
      window.clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
  }, []);

  const refreshInbox = useCallback(async (actorId: string) => {
    const result = await ipc.inboxList({
      actorId,
      state: "pending",
      limit: 100,
    });
    setInbox(result.deliveries);
  }, []);

  const applyMachines = useCallback((nextMachines: MachineInfo[]) => {
    setMachines(nextMachines);
    setAgentForm((current) => normalizeAgentForm(current, nextMachines));
  }, []);

  const loadMachines = useCallback(
    async (check = false) => {
      const result = check ? await ipc.machineCheck() : await ipc.machineList();
      applyMachines(result.machines);
      return result.machines;
    },
    [applyMachines],
  );

  const loadConfig = useCallback(async () => {
    try {
      const next = await ipc.workspacesList();
      applyConfig(next);
      void loadMachines().catch(() => {});
    } catch (err) {
      setError(errorText(err));
    }
  }, [applyConfig, loadMachines]);

  const loadWorkspaceData = useCallback(
    async (current: Workspace) => {
      actorIdRef.current = current.actorId;
      const [actorResult, channelResult, taskResult, inboxResult] =
        await Promise.allSettled([
          ipc.actorList(),
          ipc.channelList(),
          ipc.taskList(),
          ipc.inboxList({ actorId: current.actorId, state: "pending", limit: 100 }),
        ]);

      if (actorResult.status === "fulfilled") {
        const nextActors = Object.fromEntries(
          actorResult.value.actors.map((actor) => [actor.id, actor]),
        );
        if (account) nextActors[account.actorId] = accountToActor(account);
        setActors(nextActors);
      }
      if (channelResult.status === "fulfilled") {
        const nextChannels = sortChannels(channelResult.value.channels);
        setChannels(nextChannels);
        setActiveChannelId((currentChannel) =>
          currentChannel && nextChannels.some((channel) => channel.id === currentChannel)
            ? currentChannel
            : nextChannels[0]?.id ?? null,
        );
      }
      if (taskResult.status === "fulfilled") {
        setTasks(sortTasks(taskResult.value.tasks));
      }
      if (inboxResult.status === "fulfilled") {
        setInbox(inboxResult.value.deliveries);
      }
      void loadMachines().catch(() => {});
    },
    [account, loadMachines],
  );

  const connectWorkspace = useCallback(
    async (workspaceId: string, options: { automatic?: boolean } = {}) => {
      const automatic = options.automatic === true;
      if (!automatic) {
        autoReconnectRef.current = true;
        reconnectAttemptRef.current = 0;
      }
      clearReconnectTimer();
      if (!automatic) setBusy(`connect:${workspaceId}`);
      setConnection("connecting");
      setError(automatic ? "Connection lost. Reconnecting..." : null);
      try {
        const result = await ipc.connect(workspaceId);
        workspaceRef.current = result.workspace;
        autoReconnectRef.current = true;
        setWorkspace(result.workspace);
        setConnection("open");
        reconnectAttemptRef.current = 0;
        await loadWorkspaceData(result.workspace);
        pushNotice(
          automatic
            ? `Reconnected to ${result.workspace.name}`
            : `Connected to ${result.workspace.name}`,
        );
      } catch (err) {
        setConnection("error");
        setError(automatic ? "Connection lost. Reconnecting..." : errorText(err));
      } finally {
        if (!automatic) setBusy(null);
      }
    },
    [clearReconnectTimer, loadWorkspaceData, pushNotice],
  );

  const disconnect = useCallback(async () => {
    autoReconnectRef.current = false;
    reconnectAttemptRef.current = 0;
    clearReconnectTimer();
    try {
      await ipc.disconnect();
    } catch {
      /* local state still closes */
    }
    setConnection("closed");
  }, [clearReconnectTimer]);

  useEffect(() => {
    let unlistenStream: (() => void) | null = null;
    let unlistenConnection: (() => void) | null = null;

    void loadConfig();
    void ipc.onStream((update) => handleStream(update)).then((off) => {
      unlistenStream = off;
    });
    void ipc.onConnection((event) => {
      if (event.state === "closed") {
        autoReconnectRef.current = true;
        setConnection("closed");
        setError("Connection lost. Reconnecting...");
      } else {
        if (workspaceRef.current) autoReconnectRef.current = true;
        reconnectAttemptRef.current = 0;
        setConnection("open");
      }
    }).then((off) => {
      unlistenConnection = off;
    });

    return () => {
      unlistenStream?.();
      unlistenConnection?.();
    };
  }, [loadConfig]);

  useEffect(() => {
    workspaceRef.current = workspace;
  }, [workspace]);

  useEffect(() => {
    savePanelSizes(panelSizes);
  }, [panelSizes]);

  useEffect(() => {
    const updateViewport = () => setViewportWidth(initialViewportWidth());
    updateViewport();
    window.addEventListener("resize", updateViewport);
    return () => window.removeEventListener("resize", updateViewport);
  }, []);

  useEffect(() => {
    return () => {
      panelResizeCleanupRef.current?.();
      document.body.classList.remove("is-resizing-panels");
    };
  }, []);

  useEffect(() => {
    setChannelGroups(loadChannelGroups(channelGroupsKey));
  }, [channelGroupsKey]);

  useEffect(() => {
    if (
      !autoReconnectRef.current ||
      !workspace ||
      (connection !== "closed" && connection !== "error")
    ) {
      return;
    }

    const attempt = reconnectAttemptRef.current + 1;
    reconnectAttemptRef.current = attempt;
    const delay = reconnectDelayMs(attempt);
    const timer = window.setTimeout(() => {
      reconnectTimerRef.current = null;
      void connectWorkspace(workspace.id, { automatic: true });
    }, delay);
    reconnectTimerRef.current = timer;

    return () => {
      if (reconnectTimerRef.current === timer) {
        window.clearTimeout(timer);
        reconnectTimerRef.current = null;
      }
    };
  }, [connectWorkspace, connection, workspace]);

  useEffect(() => {
    if (!activeChannel || connection !== "open") return;
    let alive = true;
    void ipc
      .channelMembers(activeChannel.id)
      .then((result) => {
        if (!alive) return;
        setActors((current) => {
          const next = { ...current };
          for (const actor of result.members) next[actor.id] = actor;
          return next;
        });
      })
      .catch(() => {});
    void ipc
      .threadList(activeChannel.id)
      .then((result) => {
        if (!alive) return;
        setThreadsByChannel((current) => ({
          ...current,
          [activeChannel.id]: sortThreads(result.threads),
        }));
      })
      .catch((err) => setError(errorText(err)));
    return () => {
      alive = false;
    };
  }, [activeChannel?.id, activeChannel?.members.join("|"), connection]);

  useEffect(() => {
    if (connection !== "open" || channels.length === 0) return;
    let alive = true;
    void Promise.allSettled(
      channels.map((channel) =>
        ipc.threadList(channel.id).then((result) => ({
          channelId: channel.id,
          threads: result.threads,
        })),
      ),
    ).then((results) => {
      if (!alive) return;
      setThreadsByChannel((current) => {
        const next = { ...current };
        for (const result of results) {
          if (result.status === "fulfilled") {
            next[result.value.channelId] = sortThreads(result.value.threads);
          }
        }
        return next;
      });
    });
    return () => {
      alive = false;
    };
  }, [channelIdsKey, connection]);

  useEffect(() => {
    activeScopeRef.current = activeScope;
    targetRef.current = target;
    if (!activeScope || !target || connection !== "open") {
      setMessages([]);
      return;
    }

    let alive = true;
    setMessages([]);
    void ipc.scopeSubscribe(activeScope).catch(() => {});
    void ipc
      .messageList({ target, limit: 150 })
      .then((result) => {
        if (!alive) return;
        setError(null);
        setMessages(sortMessages(result.messages.map(normalizeMessage)));
      })
      .catch((err) => setError(errorText(err)));

    return () => {
      alive = false;
      void ipc.scopeUnsubscribe(activeScope).catch(() => {});
    };
  }, [activeScope ? scopeKey(activeScope) : null, connection, target]);

  useEffect(() => {
    activeThreadScopeRef.current = activeThreadScope;
    if (!activeThreadScope || !threadMessageTarget || connection !== "open") {
      setThreadMessages([]);
      return;
    }

    let alive = true;
    setThreadMessages([]);
    void ipc.scopeSubscribe(activeThreadScope).catch(() => {});
    void ipc
      .messageList({ target: threadMessageTarget, limit: 100 })
      .then((result) => {
        if (!alive) return;
        setError(null);
        const sorted = sortMessages(result.messages.map(normalizeMessage));
        setThreadMessages(sorted);
        setThreadStatsById((current) => ({
          ...current,
          [activeThreadScope.id]: threadStatsFromMessages(
            sorted,
            result.pageInfo?.hasMore ?? false,
          ),
        }));
      })
      .catch((err) => setError(errorText(err)));

    return () => {
      alive = false;
      void ipc.scopeUnsubscribe(activeThreadScope).catch(() => {});
    };
  }, [
    activeThreadScope ? scopeKey(activeThreadScope) : null,
    connection,
    threadMessageTarget,
  ]);

  function handleStream(update: StreamUpdate) {
    switch (update.kind) {
      case "channel.created":
      case "channel.updated":
      case "channel.invited": {
        const channel = update.data.channel as Channel | undefined;
        if (channel) setChannels((current) => sortChannels(upsert(current, channel)));
        return;
      }
      case "channel.deleted": {
        const channelId = update.data.channelId as string | undefined;
        if (!channelId) return;
        applyChannelDeleted(channelId);
        return;
      }
      case "channel.revoked": {
        const channelId = update.data.channelId as string | undefined;
        if (!channelId) return;
        applyChannelDeleted(channelId);
        return;
      }
      case "thread.created":
      case "thread.updated": {
        const thread = update.data.thread as Thread | undefined;
        if (!thread) return;
        setThreadsByChannel((current) => ({
          ...current,
          [thread.channelId]: sortThreads(
            upsert(current[thread.channelId] ?? [], thread).filter(
              (item) => !item.archivedAt,
            ),
          ),
        }));
        return;
      }
      case "message.created": {
        const message = update.data.message as Message | undefined;
        if (!message) return;
        if (messageIsActionRequestFor(message, actorIdRef.current)) {
          setInbox((current) =>
            current.some((item) => item.delivery.sourceId === message.id)
              ? current
              : [
                  {
                    delivery: {
                      sourceId: message.id,
                      actorId: actorIdRef.current ?? "",
                      state: "pending",
                      updatedAt: message.createdAt,
                    },
                    message: normalizeMessage(message),
                  },
                  ...current,
                ],
          );
        }
        if (update.scope && activeScopeRef.current && sameScope(update.scope, activeScopeRef.current)) {
          setMessages((current) => sortMessages(upsertMessage(current, message)));
        }
        if (message.scope.kind === "thread") {
          setThreadStatsById((current) => upsertThreadStatsMessage(current, message));
        }
        if (
          update.scope &&
          activeThreadScopeRef.current &&
          sameScope(update.scope, activeThreadScopeRef.current)
        ) {
          setThreadMessages((current) => sortMessages(upsertMessage(current, message)));
        }
        return;
      }
      case "message.updated": {
        const message = update.data.message as Message | undefined;
        if (!message) return;
        if (update.scope && activeScopeRef.current && sameScope(update.scope, activeScopeRef.current)) {
          setMessages((current) => sortMessages(upsertMessage(current, message)));
        }
        if (message.scope.kind === "thread") {
          setThreadStatsById((current) => upsertThreadStatsMessage(current, message));
        }
        if (
          update.scope &&
          activeThreadScopeRef.current &&
          sameScope(update.scope, activeThreadScopeRef.current)
        ) {
          setThreadMessages((current) => sortMessages(upsertMessage(current, message)));
        }
        setInbox((current) =>
          current.map((item) =>
            item.delivery.sourceId === message.id
              ? { ...item, message: normalizeMessage(message) }
              : item,
          ),
        );
        return;
      }
      case "run.updated": {
        const run = update.data.run as Run | undefined;
        if (run) setRuns((current) => ({ ...current, [run.id]: run }));
        return;
      }
      case "task.changed": {
        const task = update.data.task as Task | undefined;
        if (task) setTasks((current) => sortTasks(upsert(current, task)));
        return;
      }
      case "task_assignment.changed": {
        const task = update.data.task as Task | undefined;
        if (task) setTasks((current) => sortTasks(upsert(current, task)));
        return;
      }
      case "delivery.updated": {
        const actorId = actorIdRef.current;
        if (actorId) void refreshInbox(actorId).catch(() => {});
        return;
      }
    }
  }

  function applyChannelDeleted(channelId: string) {
    const deletedThreads = threadsByChannel[channelId] ?? [];
    const fallbackChannelId =
      channels.find((channel) => channel.id !== channelId)?.id ?? null;
    setChannels((current) => current.filter((channel) => channel.id !== channelId));
    setThreadsByChannel((current) => {
      const next = { ...current };
      delete next[channelId];
      return next;
    });
    setTasks((current) => current.filter((task) => task.channelId !== channelId));
    setThreadStatsById((current) => {
      const next = { ...current };
      for (const thread of deletedThreads) delete next[thread.id];
      return next;
    });
    updateChannelGroups((current) =>
      current.map((group) => ({
        ...group,
        channelIds: group.channelIds.filter((id) => id !== channelId),
      })),
    );
    setActiveChannelId((current) =>
      current === channelId ? fallbackChannelId : current,
    );
    setActiveThreadId((current) =>
      current && deletedThreads.some((thread) => thread.id === current)
        ? null
        : current,
    );
    if (activeChannelId === channelId) {
      setChannelPanelTab(null);
      setReplyTo(null);
      setMessages([]);
      setThreadMessages([]);
    }
  }

  async function login(provider: ipc.LoginProvider) {
    setBusy(`login:${provider}`);
    setError(null);
    try {
      const result = await ipc.accountLogin(provider);
      applyConfig(result.config);
      pushNotice(`Signed in as ${accountName(result.account)}`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function logout() {
    setBusy("logout");
    try {
      autoReconnectRef.current = false;
      reconnectAttemptRef.current = 0;
      clearReconnectTimer();
      applyConfig(await ipc.accountLogout());
      workspaceRef.current = null;
      setWorkspace(null);
      setConnection("idle");
      setChannels([]);
      setMessages([]);
      setThreadMessages([]);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function addWorkspace() {
    if (!workspaceForm.name.trim() || !workspaceForm.serverUrl.trim()) return;
    setBusy("workspace:add");
    setError(null);
    try {
      const next = await ipc.workspaceAdd({
        name: workspaceForm.name.trim(),
        serverUrl: workspaceForm.serverUrl.trim(),
        activate: true,
      });
      applyConfig(next);
      await loadMachines();
      setWorkspaceForm({ name: "Local", serverUrl: "ws://127.0.0.1:7878/rpc" });
      pushNotice(`Space ${workspaceForm.name.trim()} added`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function removeWorkspace(id: string) {
    setBusy(`workspace:remove:${id}`);
    setError(null);
    try {
      const next = await ipc.workspaceRemove(id);
      applyConfig(next);
      await loadMachines();
      if (workspace?.id === id) {
        autoReconnectRef.current = false;
        reconnectAttemptRef.current = 0;
        clearReconnectTimer();
        workspaceRef.current = null;
        setWorkspace(null);
        setConnection("idle");
      }
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function checkMachines() {
    setBusy("machine:check");
    setError(null);
    try {
      await loadMachines(true);
      pushNotice("Agent host status refreshed");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function createMachine() {
    const name = machineForm.name.trim();
    if (!name) return;
    setBusy("machine:create");
    setError(null);
    try {
      const result = await ipc.machineCreate({
        name,
        dataRoot: machineForm.dataRoot.trim() || undefined,
      });
      applyMachines(result.machines);
      setMachineForm({ name: "Local Host", dataRoot: "" });
      pushNotice(`Agent host ${name} added`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function removeMachine(machineId: string) {
    setBusy(`machine:remove:${machineId}`);
    setError(null);
    try {
      const result = await ipc.machineRemove(machineId);
      applyMachines(result.machines);
      pushNotice("Agent host removed");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function createAgent() {
    const name = agentForm.name.trim();
    const machine = resolveAgentMachine(agentForm, machines);
    const provider = resolveAgentProvider(agentForm, machine);
    if (!machine) {
      setError("Add an agent host before creating an agent.");
      return;
    }
    if (!machineCanCreateAgent(machine)) {
      setError(`Agent host ${machine.name} is read-only or does not support agent creation.`);
      return;
    }
    if (!provider) {
      setError(`No agent runtime is available for ${machine.name}.`);
      return;
    }
    if (!name) {
      setError("Agent name is required.");
      return;
    }
    setBusy("agent:create");
    setError(null);
    try {
      const result = await ipc.machineAgentCreate({
        machineId: machine.id,
        providerId: provider.id,
        actorId: agentForm.actorId.trim() || undefined,
        name,
        description: agentForm.description.trim(),
        model: agentForm.model.trim() || provider.defaultModel || "",
        autostart: agentForm.autostart,
      });
      applyMachines(result.machines);
      setAgentForm((current) =>
        normalizeAgentForm({ ...current, actorId: "", name: "Echo" }, result.machines),
      );
      if (workspace && connection === "open") {
        await loadWorkspaceData(workspace);
      }
      pushNotice(`Agent ${name} added`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function removeAgent(machineId: string, actorId: string) {
    setBusy(`agent:remove:${actorId}`);
    setError(null);
    try {
      const result = await ipc.machineAgentRemove({ machineId, actorId });
      applyMachines(result.machines);
      if (workspace && connection === "open") {
        await loadWorkspaceData(workspace);
      }
      pushNotice("Agent removed");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function updateAgent(patch: AgentUpdatePatch) {
    setBusy(`agent:update:${patch.actorId}`);
    setError(null);
    try {
      await ipc.agentUpdate({
        machineId: patch.machineId,
        actorId: patch.actorId,
        displayName: patch.displayName.trim(),
        description: patch.description.trim(),
        providerId: patch.providerId,
        model: patch.model.trim(),
        reasoningEffort: patch.reasoningEffort.trim(),
        autostart: patch.autostart,
        avatarUrl: patch.avatarUrl.trim(),
      });
      await loadMachines();
      if (workspace && connection === "open") {
        await loadWorkspaceData(workspace);
      }
      pushNotice("Agent settings saved");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function inviteMemberToChannel(channelId: string, actorId: string) {
    const actor = actors[actorId];
    setBusy(`channel:invite:${channelId}:${actorId}`);
    setError(null);
    try {
      const result = await ipc.channelInvite({ channelId, actorId });
      setChannels((current) => sortChannels(upsert(current, result.channel)));
      pushNotice(`${actor ? displayName(actor) : actorId} added to channel`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function removeMemberFromChannel(channelId: string, actorId: string) {
    const actor = actors[actorId];
    setBusy(`channel:revoke:${channelId}:${actorId}`);
    setError(null);
    try {
      const result = await ipc.channelRevoke({ channelId, actorId });
      setChannels((current) => sortChannels(upsert(current, result.channel)));
      pushNotice(`${actor ? displayName(actor) : actorId} removed from channel`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function openLocalPath(path: string) {
    setError(null);
    try {
      await ipc.openLocalPath(path);
    } catch (err) {
      setError(errorText(err));
    }
  }

  async function createChannelWithTitle(rawTitle: string) {
    const title = rawTitle.trim();
    if (!title || !workspace) return;
    setBusy("channel:create");
    try {
      const result = await ipc.channelCreate({
        title,
        actorId: workspace.actorId,
      });
      setChannels((current) => sortChannels(upsert(current, result.channel)));
      setActiveChannelId(result.channel.id);
      setActiveThreadId(null);
      setChannelPanelTab(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function renameChannel(channel: Channel, rawTitle: string) {
    const title = rawTitle.trim();
    if (!title || title === channel.title) return;
    setBusy(`channel:rename:${channel.id}`);
    setError(null);
    try {
      const result = await ipc.channelUpdate({
        channelId: channel.id,
        title,
        topic: channel.topic,
      });
      setChannels((current) => sortChannels(upsert(current, result.channel)));
      pushNotice(`Renamed #${channel.title} to #${result.channel.title}`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function deleteChannel(channel: Channel) {
    setBusy(`channel:delete:${channel.id}`);
    setError(null);
    try {
      const result = await ipc.channelDelete({
        channelId: channel.id,
        cascade: true,
      });
      if (result.deleted) {
        applyChannelDeleted(channel.id);
        const deletedThreads = result.deletedThreads ?? 0;
        pushNotice(
          deletedThreads > 0
            ? `Deleted #${channel.title} and ${deletedThreads} threads`
            : `Deleted #${channel.title}`,
        );
      }
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function sendMessage() {
    const body = draft.trim();
    if (!body || !target) return;
    setBusy("message:send");
    try {
      const parentMessageId = replyTo?.id;
      const repliedActor = replyTo ? actors[replyTo.authorActorId] : undefined;
      const mentionedAudience = mentionAudience(body, actors, workspace?.actorId);
      const replyAudience =
        repliedActor && repliedActor.id !== workspace?.actorId
          ? [{ kind: "actor" as const, id: repliedActor.id }]
          : [];
      const directedTo = uniqueAudience([...replyAudience, ...mentionedAudience]);
      const unavailableAgents = activeChannel
        ? directedTo.filter(
            (audience) =>
              audience.kind === "actor" && !isChannelMember(activeChannel, audience.id),
          )
        : [];
      if (unavailableAgents.length > 0) {
        setError(
          `Add ${unavailableAgents
            .map((audience) => actorName(actors, audience.id))
            .join(", ")} to this channel before mentioning them.`,
        );
        return;
      }
      const wakesAgent = directedTo.some((audience) =>
        audienceWakesAgent(audience, actors),
      );
      const result = await ipc.messageSend({
        target,
        body,
        parentMessageId,
        audience: directedTo,
        deliveryPolicy: wakesAgent ? "wake_agent" : "notify_only",
        intent: wakesAgent ? "request_action" : "chat",
      });
      setMessages((current) => sortMessages(upsertMessage(current, result.message)));
      setDraft("");
      setReplyTo(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function sendThreadMessage() {
    const body = threadDraft.trim();
    if (!body || !threadMessageTarget) return;
    setBusy("thread:message:send");
    try {
      const mentionedAudience = mentionAudience(body, actors, workspace?.actorId);
      const unavailableAgents = activeChannel
        ? mentionedAudience.filter(
            (audience) =>
              audience.kind === "actor" && !isChannelMember(activeChannel, audience.id),
          )
        : [];
      if (unavailableAgents.length > 0) {
        setError(
          `Add ${unavailableAgents
            .map((audience) => actorName(actors, audience.id))
            .join(", ")} to this channel before mentioning them.`,
        );
        return;
      }
      const wakesAgent = mentionedAudience.some((audience) =>
        audienceWakesAgent(audience, actors),
      );
      const result = await ipc.messageSend({
        target: threadMessageTarget,
        body,
        audience: mentionedAudience,
        deliveryPolicy: wakesAgent ? "wake_agent" : "notify_only",
        intent: wakesAgent ? "request_action" : "chat",
      });
      setThreadMessages((current) => sortMessages(upsertMessage(current, result.message)));
      setThreadStatsById((current) => upsertThreadStatsMessage(current, result.message));
      setThreadDraft("");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function startThread(message: Message) {
    if (!activeChannel) return;
    const existing = channelThreads.find(
      (thread) => thread.rootMessageId === message.id,
    );
    if (existing) {
      setActiveThreadId(existing.id);
      setChannelPanelTab(null);
      return;
    }
    setBusy(`thread:create:${message.id}`);
    try {
      const result = await ipc.threadCreate({
        channelId: activeChannel.id,
        rootMessageId: message.id,
        title: threadTitle(message),
      });
      setThreadsByChannel((current) => ({
        ...current,
        [activeChannel.id]: sortThreads(
          upsert(current[activeChannel.id] ?? [], result.thread),
        ),
      }));
      setActiveThreadId(result.thread.id);
      setChannelPanelTab(null);
      setThreadStatsById((current) => ({
        ...current,
        [result.thread.id]: emptyThreadStats(),
      }));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function toggleMessageReaction(message: Message, emoji: string) {
    setBusy(`message:reaction:${message.id}:${emoji}`);
    setError(null);
    try {
      const result = await ipc.messageReactionToggle({
        messageId: message.id,
        emoji,
      });
      setMessages((current) => sortMessages(upsertMessage(current, result.message)));
      setThreadMessages((current) =>
        current.some((item) => item.id === result.message.id)
          ? sortMessages(upsertMessage(current, result.message))
          : current,
      );
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function answerAction(message: Message, optionId: string, accepted: boolean) {
    const responseTarget = message.target || target;
    if (!responseTarget || !workspace) return;
    const responseKind = accepted ? "accepted" : "declined";
    setBusy(`action:${message.id}:${optionId}`);
    try {
      await ipc.messageSend({
        target: responseTarget,
        body: `${responseKind}: ${optionId}`,
        parentMessageId: message.id,
        audience: [{ kind: "actor", id: message.authorActorId }],
        intent: "notify",
        deliveryPolicy: "wake_agent",
        metadata: {
          kind: "action.response",
          optionId,
          responseKind,
          requestMessageId: message.id,
        },
      });
      await ipc.deliveryAck({
        actorId: workspace.actorId,
        sourceId: message.id,
      });
      setInbox((current) =>
        current.filter((item) => item.delivery.sourceId !== message.id),
      );
      pushNotice(`Action ${responseKind}`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  const updateChannelGroups = useCallback(
    (updater: (current: ChannelGroup[]) => ChannelGroup[]) => {
      setChannelGroups((current) => {
        const next = normalizeChannelGroups(updater(current));
        saveChannelGroups(channelGroupsKey, next);
        return next;
      });
    },
    [channelGroupsKey],
  );

  const addChannelGroup = useCallback((title: string) => {
    const trimmed = title.trim();
    if (!trimmed) return;
    updateChannelGroups((current) => [
      ...current,
      {
        id: `local-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
        title: trimmed,
        channelIds: [],
        collapsed: false,
      },
    ]);
  }, [updateChannelGroups]);

  const renameChannelGroup = useCallback(
    (groupId: string, title: string) => {
      const trimmed = title.trim();
      if (!trimmed) return;
      updateChannelGroups((current) =>
        current.map((item) =>
          item.id === groupId ? { ...item, title: trimmed } : item,
        ),
      );
    },
    [updateChannelGroups],
  );

  const removeChannelGroup = useCallback(
    (groupId: string) => {
      updateChannelGroups((current) => current.filter((item) => item.id !== groupId));
    },
    [updateChannelGroups],
  );

  const toggleChannelGroup = useCallback(
    (groupId: string) => {
      updateChannelGroups((current) =>
        current.map((item) =>
          item.id === groupId ? { ...item, collapsed: !item.collapsed } : item,
        ),
      );
    },
    [updateChannelGroups],
  );

  const moveChannelToGroup = useCallback(
    (channelId: string, groupId: string) => {
      updateChannelGroups((current) =>
        current.map((group) => {
          const channelIds = group.channelIds.filter((id) => id !== channelId);
          if (group.id === groupId && groupId !== ungroupedChannelGroupId) {
            channelIds.push(channelId);
            return { ...group, channelIds, collapsed: false };
          }
          return { ...group, channelIds };
        }),
      );
    },
    [updateChannelGroups],
  );

  const chatEmpty =
    connection === "open"
      ? activeChannel
        ? "No messages in this channel."
        : "No channels."
      : "No space connection.";

  const showWorkspaceChrome =
    view === "chat" ||
    view === "threads" ||
    view === "channels" ||
    view === "inbox" ||
    view === "tasks" ||
    view === "settings";
  const showChatDetail =
    view === "chat" &&
    (Boolean(activeThread) || (Boolean(channelPanelTab) && Boolean(activeChannel)));
  const detailVisibleInGrid =
    showChatDetail && viewportWidth >= detailPanelBreakpoint;
  const fittedPanelSizes = fitPanelSizes(
    panelSizes,
    viewportWidth,
    detailVisibleInGrid,
  );
  const shellStyle = {
    "--sidebar-width": `${fittedPanelSizes.sidebar}px`,
    "--detail-width": `${fittedPanelSizes.detail}px`,
    "--main-min-width": `${mainMinWidth}px`,
  } as CSSProperties;
  const cleanupPanelResize = () => {
    panelResizeCleanupRef.current?.();
    panelResizeCleanupRef.current = null;
    panelResizeDragRef.current = null;
    setResizingPanel(null);
    document.body.classList.remove("is-resizing-panels");
  };
  const applyPanelResize = (
    drag: PanelResizeDrag,
    clientX: number,
    width = initialViewportWidth(),
  ) => {
    const delta = clientX - drag.startX;
    const next =
      drag.kind === "sidebar"
        ? { sidebar: drag.sidebar + delta, detail: drag.detail }
        : { sidebar: drag.sidebar, detail: drag.detail - delta };
    setPanelSizes(fitPanelSizes(next, width, width >= detailPanelBreakpoint && showChatDetail));
  };
  const startPanelResize = (
    event: PointerEvent<HTMLButtonElement>,
    kind: PanelResizeKind,
  ) => {
    if (event.button !== 0) return;
    event.preventDefault();
    cleanupPanelResize();
    const drag: PanelResizeDrag = {
      kind,
      startX: event.clientX,
      sidebar: panelSizes.sidebar,
      detail: panelSizes.detail,
    };
    panelResizeDragRef.current = drag;
    setResizingPanel(kind);
    document.body.classList.add("is-resizing-panels");
    const handlePointerMove = (moveEvent: globalThis.PointerEvent) => {
      const current = panelResizeDragRef.current;
      if (!current) return;
      moveEvent.preventDefault();
      applyPanelResize(current, moveEvent.clientX);
    };
    const handlePointerUp = (upEvent: globalThis.PointerEvent) => {
      const current = panelResizeDragRef.current;
      if (current) applyPanelResize(current, upEvent.clientX);
      cleanupPanelResize();
    };
    window.addEventListener("pointermove", handlePointerMove, { passive: false });
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", cleanupPanelResize);
    panelResizeCleanupRef.current = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", cleanupPanelResize);
    };
  };
  const resizePanelByKeyboard = (kind: PanelResizeKind, delta: number) => {
    setPanelSizes((current) => {
      const next =
        kind === "sidebar"
          ? { ...current, sidebar: current.sidebar + delta }
          : { ...current, detail: current.detail - delta };
      return fitPanelSizes(next, viewportWidth, detailVisibleInGrid);
    });
  };
  const selectWorkspace = (workspaceId: string) => {
    setView("chat");
    setChannelPanelTab(null);
    if (workspace?.id !== workspaceId || connection !== "open") {
      void connectWorkspace(workspaceId);
    }
  };

  return (
    <div
      className={cn(
        "app-shell h-screen w-screen overflow-hidden bg-[#f5f6fa] text-foreground",
        showWorkspaceChrome && "app-shell-workspace",
        showChatDetail && "app-shell-detail",
      )}
      style={shellStyle}
    >
      <Rail
        account={account}
        busy={busy}
        connection={connection}
        workspace={workspace}
        workspaces={workspaces}
        onSelectWorkspace={selectWorkspace}
        onDisconnect={disconnect}
        onOpenHome={() => setView("chat")}
        onOpenSpaces={() => setView("spaces")}
        onOpenAccount={() => setView("account")}
      />
      {showWorkspaceChrome && (
        <Sidebar
          view={view}
          setView={setView}
          busy={busy}
          channels={channels}
          channelGroups={channelGroups}
          connection={connection}
          activeChannelId={activeChannelId}
          activeThreadId={activeThreadId}
          threadsByChannel={threadsByChannel}
          onAddChannel={(title) => {
            void createChannelWithTitle(title);
          }}
          onAddChannelGroup={addChannelGroup}
          onMoveChannelToGroup={moveChannelToGroup}
          onDeleteChannel={deleteChannel}
          onRenameChannel={renameChannel}
          onRemoveChannelGroup={removeChannelGroup}
          onRenameChannelGroup={renameChannelGroup}
          onSelectChannel={(id) => {
            setView("chat");
            setActiveChannelId(id);
            setActiveThreadId(null);
            setChannelPanelTab(null);
          }}
          onSelectThread={(thread) => {
            setView("chat");
            setActiveChannelId(thread.channelId);
            setActiveThreadId(thread.id);
            setChannelPanelTab(null);
          }}
          onToggleChannelGroup={toggleChannelGroup}
        />
      )}
      {showWorkspaceChrome && (
        <ResizeHandle
          active={resizingPanel === "sidebar"}
          label="Resize sidebar"
          onKeyboardResize={(delta) => resizePanelByKeyboard("sidebar", delta)}
          onPointerDown={(event) => startPanelResize(event, "sidebar")}
        />
      )}
      <main
        className="flex min-h-0 min-w-0 flex-col bg-white"
      >
        {view === "chat" ? (
          <>
            <ChatHeader
              channel={activeChannel}
              target={target}
              connection={connection}
              activePanel={activeThread ? null : channelPanelTab}
              onOpenPanel={(panel) => {
                setActiveThreadId(null);
                setChannelPanelTab((current) => (current === panel ? null : panel));
              }}
            />
            {error && (
              <div className="border-b border-red-200 bg-red-50 px-4 py-2 text-sm font-medium text-red-700">
                {error}
              </div>
            )}
            <MessageFeed
              actors={actors}
              feedKey={target ?? "channel:none"}
              machines={machines}
              messages={messages}
              tasksBySourceMessageId={tasksBySourceMessageId}
              channelThreads={channelThreads}
              threadStatsById={threadStatsById}
              emptyText={chatEmpty}
              onReply={setReplyTo}
              onStartThread={startThread}
              onToggleReaction={toggleMessageReaction}
              onAnswerAction={answerAction}
              onOpenAgentSettings={openAgentSettings}
              currentActorId={workspace?.actorId ?? null}
              busy={busy}
            />
            <Composer
              draft={draft}
              setDraft={setDraft}
              disabled={connection !== "open" || !target}
              replyTo={replyTo}
              actorName={replyTo ? actorName(actors, replyTo.authorActorId) : ""}
              onClearReply={() => setReplyTo(null)}
              onSend={sendMessage}
              mentionAgents={channelAgentActors}
              busy={busy === "message:send"}
            />
          </>
        ) : view === "threads" ? (
          <>
            <ErrorBanner error={error} />
            <ThreadsView
              actors={actors}
              channels={channels}
              messages={messages}
              machines={machines}
              threadMessages={threadMessages}
              threadStatsById={threadStatsById}
              threads={allThreads}
              activeChannelId={activeChannelId}
              activeThread={activeThread}
              activeThreadTask={activeThreadTask}
              currentActorId={workspace?.actorId ?? null}
              threadDraft={threadDraft}
              setThreadDraft={setThreadDraft}
              onSelectThread={(thread) => {
                setActiveChannelId(thread.channelId);
                setActiveThreadId(thread.id);
                setChannelPanelTab(null);
              }}
              onCloseThread={() => setActiveThreadId(null)}
              onSendThreadMessage={sendThreadMessage}
              onToggleReaction={toggleMessageReaction}
              onOpenAgentSettings={openAgentSettings}
              busy={busy}
              disabled={connection !== "open" || !threadMessageTarget}
            />
          </>
        ) : view === "channels" ? (
          <>
            <ErrorBanner error={error} />
            <ChannelsView
              actors={actors}
              busy={busy}
              channels={channels}
              channelGroups={channelGroups}
              threadsByChannel={threadsByChannel}
              activeChannel={activeChannel}
              onSelectChannel={(channelId) => {
                setActiveChannelId(channelId);
                setActiveThreadId(null);
                setChannelPanelTab(null);
              }}
              onDeleteChannel={deleteChannel}
            />
          </>
        ) : view === "inbox" ? (
          <>
            <ErrorBanner error={error} />
            <InboxView
              actors={actors}
              inbox={inbox}
              onOpen={(message) => {
                if (!message) return;
                setView("chat");
                setActiveChannelId(channelFromMessage(message));
                setActiveThreadId(threadIdForMessage(threadsByChannel, message));
                setChannelPanelTab(null);
              }}
              onAnswer={answerAction}
              busy={busy}
            />
          </>
        ) : view === "tasks" ? (
          <>
            <ErrorBanner error={error} />
            <TasksView tasks={tasks} channels={channels} />
          </>
        ) : view === "spaces" ? (
          <>
            <ErrorBanner error={error} />
            <SpacesView
              busy={busy}
              connection={connection}
              workspace={workspace}
              workspaceForm={workspaceForm}
              setWorkspaceForm={setWorkspaceForm}
              workspaces={workspaces}
              onAddWorkspace={addWorkspace}
              onRemoveWorkspace={removeWorkspace}
              onSelectWorkspace={selectWorkspace}
            />
          </>
        ) : view === "account" ? (
          <>
            <ErrorBanner error={error} />
            <AccountView
              account={account}
              busy={busy}
              onLogin={login}
              onLogout={logout}
            />
          </>
        ) : (
          <>
            <ErrorBanner error={error} />
            <SettingsView
              busy={busy}
              machineForm={machineForm}
              setMachineForm={setMachineForm}
              agentForm={agentForm}
              setAgentForm={setAgentForm}
              machines={machines}
              targetAgentId={settingsAgentId}
              onCheckMachines={checkMachines}
              onAddMachine={createMachine}
              onRemoveMachine={removeMachine}
              onAddAgent={createAgent}
              onUpdateAgent={updateAgent}
              onRemoveAgent={removeAgent}
              onOpenLocalPath={openLocalPath}
            />
          </>
        )}
      </main>
      {showChatDetail && (
        <ResizeHandle
          active={resizingPanel === "detail"}
          className="hidden xl:block"
          label="Resize details panel"
          onKeyboardResize={(delta) => resizePanelByKeyboard("detail", delta)}
          onPointerDown={(event) => startPanelResize(event, "detail")}
        />
      )}
      {showChatDetail && (
        activeThread ? (
          <ThreadPanel
            actors={actors}
            channel={activeChannel}
            channelMessages={messages}
            currentActorId={workspace?.actorId ?? null}
            disabled={connection !== "open" || !threadMessageTarget}
            draft={threadDraft}
            machines={machines}
            messages={threadMessages}
            setDraft={setThreadDraft}
            task={activeThreadTask}
            thread={activeThread}
            busy={busy}
            className="hidden min-h-0 min-w-0 flex-col bg-white xl:flex"
            onClose={() => setActiveThreadId(null)}
            onSend={sendThreadMessage}
            onToggleReaction={toggleMessageReaction}
            onOpenAgentSettings={openAgentSettings}
          />
        ) : channelPanelTab && activeChannel ? (
          <ChannelPanel
            actors={actors}
            memberCandidates={memberCandidates}
            channel={activeChannel}
            channelMessages={messages}
            channelTasks={channelTasks}
            channelThreads={channelThreads}
            currentActorId={workspace?.actorId ?? null}
            machines={machines}
            threadStatsById={threadStatsById}
            tab={channelPanelTab}
            busy={busy}
            onClose={() => setChannelPanelTab(null)}
            onSelectTab={setChannelPanelTab}
            onSelectThread={(thread) => {
              setActiveThreadId(thread.id);
              setChannelPanelTab(null);
            }}
            onInviteMember={inviteMemberToChannel}
            onRemoveMember={removeMemberFromChannel}
          />
        ) : null
      )}
      {notice && (
        <div className="fixed bottom-4 left-1/2 z-50 -translate-x-1/2 rounded-md border border-border bg-popover px-4 py-2 text-sm shadow-soft">
          {notice}
        </div>
      )}
    </div>
  );
}

function Rail({
  account,
  busy,
  connection,
  workspace,
  workspaces,
  onSelectWorkspace,
  onDisconnect,
  onOpenHome,
  onOpenSpaces,
  onOpenAccount,
}: {
  account: HumanAccount | null;
  busy: string | null;
  connection: ConnectionState;
  workspace: Workspace | null;
  workspaces: Workspace[];
  onSelectWorkspace: (workspaceId: string) => void;
  onDisconnect: () => void;
  onOpenHome: () => void;
  onOpenSpaces: () => void;
  onOpenAccount: () => void;
}) {
  return (
    <nav className="flex min-h-0 flex-col items-center border-r border-[#e2e6ef] bg-[#f7f8fb] px-2.5 py-4">
      <button
        type="button"
        title="Home"
        className="mb-4 flex h-11 w-11 items-center justify-center rounded-xl bg-gradient-to-br from-[#6f58f6] to-[#4b36d8] text-base font-bold text-white shadow-sm ring-1 ring-white/60"
        onClick={onOpenHome}
      >
        L
      </button>
      <div className="flex flex-1 flex-col items-center gap-2">
        {workspaces.map((item) => {
          const selected = item.id === workspace?.id;
          return (
            <button
              key={item.id}
              title={item.name}
              className={cn(
                "relative flex h-10 w-10 items-center justify-center rounded-xl border text-sm font-bold transition-colors",
                selected
                  ? "border-[#6e5bf2] bg-white text-[#5843d7] shadow-sm ring-2 ring-[#d9d4ff]"
                  : "border-[#dfe3ec] bg-white/70 text-[#303849] hover:border-[#c8cee0] hover:bg-white",
              )}
              onClick={() => onSelectWorkspace(item.id)}
              disabled={busy === `connect:${item.id}`}
            >
              {busy === `connect:${item.id}` ? (
                <Loader2 className="animate-spin" size={15} />
              ) : (
                workspaceInitials(item)
              )}
              {selected && connection === "open" && (
                <span className="absolute -bottom-0.5 -right-0.5 h-3.5 w-3.5 rounded-full border-2 border-[#f7f8fb] bg-emerald-400" />
              )}
            </button>
          );
        })}
        {workspaces.length === 0 && (
          <button
            type="button"
            title="Add space"
            className="flex h-10 w-10 items-center justify-center rounded-xl border border-[#dfe3ec] bg-white/70 text-[#667085] transition-colors hover:bg-white hover:text-[#5843d7]"
            onClick={onOpenSpaces}
          >
            <Plus size={18} />
          </button>
        )}
        {workspaces.length > 0 && (
          <button
            type="button"
            title="Manage spaces"
            className="mt-1 flex h-9 w-9 items-center justify-center rounded-xl border border-[#dfe3ec] bg-white/50 text-[#667085] transition-colors hover:bg-white hover:text-[#5843d7]"
            onClick={onOpenSpaces}
          >
            <Plus size={17} />
          </button>
        )}
      </div>
      <div className="flex flex-col items-center gap-3">
        {connection === "open" ? (
          <button
            type="button"
            title="Disconnect"
            className="flex h-8 w-8 items-center justify-center rounded-lg text-[#667085] transition-colors hover:bg-white hover:text-[#5843d7]"
            onClick={onDisconnect}
          >
            <LogOut size={16} />
          </button>
        ) : null}
        <button
          type="button"
          title={account ? `${accountName(account)} account` : "Account"}
          className="relative"
          onClick={onOpenAccount}
        >
          {account ? (
            <Avatar account={account} />
          ) : (
            <span className="flex h-10 w-10 items-center justify-center rounded-xl border border-[#dfe3ec] bg-white text-sm font-bold text-[#667085]">
              <Users size={17} />
            </span>
          )}
        </button>
      </div>
    </nav>
  );
}

function ResizeHandle({
  active,
  className,
  label,
  onKeyboardResize,
  onPointerDown,
}: {
  active: boolean;
  className?: string;
  label: string;
  onKeyboardResize: (delta: number) => void;
  onPointerDown: (event: PointerEvent<HTMLButtonElement>) => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      className={cn("resize-handle", active && "resize-handle-active", className)}
      onKeyDown={(event) => {
        const step = event.shiftKey ? 32 : 16;
        if (event.key === "ArrowLeft") {
          event.preventDefault();
          onKeyboardResize(-step);
        } else if (event.key === "ArrowRight") {
          event.preventDefault();
          onKeyboardResize(step);
        }
      }}
      onPointerDown={onPointerDown}
    />
  );
}

function Sidebar({
  view,
  setView,
  busy,
  channels,
  channelGroups,
  connection,
  activeChannelId,
  activeThreadId,
  threadsByChannel,
  onAddChannel,
  onAddChannelGroup,
  onMoveChannelToGroup,
  onDeleteChannel,
  onRenameChannel,
  onRemoveChannelGroup,
  onRenameChannelGroup,
  onSelectChannel,
  onSelectThread,
  onToggleChannelGroup,
}: {
  view: View;
  setView: (view: View) => void;
  busy: string | null;
  channels: Channel[];
  channelGroups: ChannelGroup[];
  connection: ConnectionState;
  activeChannelId: string | null;
  activeThreadId: string | null;
  threadsByChannel: Record<string, Thread[]>;
  onAddChannel: (title: string) => void;
  onAddChannelGroup: (title: string) => void;
  onMoveChannelToGroup: (channelId: string, groupId: string) => void;
  onDeleteChannel: (channel: Channel) => void;
  onRenameChannel: (channel: Channel, title: string) => void;
  onRemoveChannelGroup: (groupId: string) => void;
  onRenameChannelGroup: (groupId: string, title: string) => void;
  onSelectChannel: (channelId: string) => void;
  onSelectThread: (thread: Thread) => void;
  onToggleChannelGroup: (groupId: string) => void;
}) {
  const [createMenuOpen, setCreateMenuOpen] = useState(false);
  const [createKind, setCreateKind] = useState<"channel" | "section" | null>(null);
  const [createTitle, setCreateTitle] = useState("");
  const [editingSectionId, setEditingSectionId] = useState<string | null>(null);
  const [sectionTitleDraft, setSectionTitleDraft] = useState("");
  const [deleteSectionId, setDeleteSectionId] = useState<string | null>(null);
  const [editingChannelId, setEditingChannelId] = useState<string | null>(null);
  const [channelTitleDraft, setChannelTitleDraft] = useState("");
  const [deleteChannelId, setDeleteChannelId] = useState<string | null>(null);
  const [deleteTitleConfirm, setDeleteTitleConfirm] = useState("");
  const [channelContextMenu, setChannelContextMenu] =
    useState<ChannelContextMenu | null>(null);
  const [draggingChannelId, setDraggingChannelId] = useState<string | null>(null);
  const [dragOverSectionId, setDragOverSectionId] = useState<string | null>(null);
  const dragSessionRef = useRef<ChannelPointerDrag | null>(null);
  const dragListenerCleanupRef = useRef<(() => void) | null>(null);
  const suppressChannelClickRef = useRef<string | null>(null);
  const sections = channelGroupSections(channelGroups, channels);
  const contextMenuChannel = channelContextMenu
    ? channels.find((channel) => channel.id === channelContextMenu.channelId) ?? null
    : null;
  const navItems = [
    { id: "chat" as const, label: "Home", icon: Home },
    { id: "channels" as const, label: "All Channels", icon: Hash },
    { id: "threads" as const, label: "Threads", icon: MessageSquare },
    { id: "inbox" as const, label: "Inbox", icon: Bell },
    { id: "tasks" as const, label: "Tasks", icon: Check },
    { id: "settings" as const, label: "Hosts", icon: Server },
  ];
  const closeCreateMenu = () => {
    setCreateMenuOpen(false);
    setCreateKind(null);
    setCreateTitle("");
  };

  const closeDeleteChannelConfirm = () => {
    setDeleteChannelId(null);
    setDeleteTitleConfirm("");
  };

  const closeRenameChannel = () => {
    setEditingChannelId(null);
    setChannelTitleDraft("");
  };

  const handleOpenCreate = (kind: "channel" | "section") => {
    setCreateKind(kind);
    setCreateTitle("");
  };

  const handleCreateSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const title = createTitle.trim();
    if (!title || !createKind) return;
    if (createKind === "channel") {
      if (connection !== "open") return;
      onAddChannel(title);
    } else {
      onAddChannelGroup(title);
    }
    closeCreateMenu();
  };

  const startRenameSection = (section: ChannelGroupSection) => {
    setEditingSectionId(section.id);
    setSectionTitleDraft(section.title);
    setDeleteSectionId(null);
    closeDeleteChannelConfirm();
    closeRenameChannel();
    setChannelContextMenu(null);
  };

  const submitRenameSection = (
    event: FormEvent<HTMLFormElement>,
    sectionId: string,
  ) => {
    event.preventDefault();
    const title = sectionTitleDraft.trim();
    if (!title) return;
    onRenameChannelGroup(sectionId, title);
    setEditingSectionId(null);
    setSectionTitleDraft("");
  };

  const confirmDeleteSection = (sectionId: string) => {
    onRemoveChannelGroup(sectionId);
    if (editingSectionId === sectionId) {
      setEditingSectionId(null);
      setSectionTitleDraft("");
    }
    setDeleteSectionId(null);
  };

  const requestDeleteChannel = (channelId: string) => {
    closeCreateMenu();
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    closeRenameChannel();
    setChannelContextMenu(null);
    setDeleteChannelId(channelId);
    setDeleteTitleConfirm("");
  };

  const startRenameChannel = (channel: Channel) => {
    closeCreateMenu();
    closeDeleteChannelConfirm();
    setChannelContextMenu(null);
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    setEditingChannelId(channel.id);
    setChannelTitleDraft(channel.title);
  };

  const submitRenameChannel = (
    event: FormEvent<HTMLFormElement>,
    channel: Channel,
  ) => {
    event.preventDefault();
    const title = channelTitleDraft.trim();
    if (!title) return;
    onRenameChannel(channel, title);
    closeRenameChannel();
  };

  const openChannelContextMenu = (
    event: MouseEvent<HTMLElement>,
    channel: Channel,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    cancelChannelDrag();
    closeCreateMenu();
    closeDeleteChannelConfirm();
    closeRenameChannel();
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    setChannelContextMenu({
      channelId: channel.id,
      x: Math.max(
        channelContextMenuViewportPaddingPx,
        Math.min(
          event.clientX,
          window.innerWidth - channelContextMenuWidthPx - channelContextMenuViewportPaddingPx,
        ),
      ),
      y: Math.max(
        channelContextMenuViewportPaddingPx,
        Math.min(
          event.clientY,
          window.innerHeight - channelContextMenuHeightPx - channelContextMenuViewportPaddingPx,
        ),
      ),
    });
  };

  const sectionIdAtPoint = (x: number, y: number) => {
    const element = document.elementFromPoint(x, y);
    const section = element?.closest("[data-channel-section-id]") as HTMLElement | null;
    return section?.dataset.channelSectionId ?? null;
  };

  const cleanupChannelDragListeners = () => {
    dragListenerCleanupRef.current?.();
    dragListenerCleanupRef.current = null;
  };

  const updateChannelDragAtPoint = (
    clientX: number,
    clientY: number,
    pointerId: number,
    preventDefault?: () => void,
  ) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== pointerId) return;
    const distance =
      Math.abs(clientX - session.startX) + Math.abs(clientY - session.startY);
    if (!session.dragging && distance < 6) return;
    if (!session.dragging) {
      session.dragging = true;
      closeCreateMenu();
      setDraggingChannelId(session.channelId);
    }
    preventDefault?.();
    setDragOverSectionId(sectionIdAtPoint(clientX, clientY));
  };

  const finishChannelDragAtPoint = (
    clientX: number,
    clientY: number,
    pointerId: number,
    preventDefault?: () => void,
    stopPropagation?: () => void,
  ) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== pointerId) return;
    const didDrag = session.dragging;
    const sectionId = didDrag
      ? sectionIdAtPoint(clientX, clientY) ?? dragOverSectionId
      : null;
    dragSessionRef.current = null;
    cleanupChannelDragListeners();
    if (didDrag) {
      preventDefault?.();
      stopPropagation?.();
      suppressChannelClickRef.current = session.channelId;
      window.setTimeout(() => {
        if (suppressChannelClickRef.current === session.channelId) {
          suppressChannelClickRef.current = null;
        }
      }, 120);
      if (sectionId) onMoveChannelToGroup(session.channelId, sectionId);
    }
    setDraggingChannelId(null);
    setDragOverSectionId(null);
  };

  const cancelChannelDrag = () => {
    cleanupChannelDragListeners();
    dragSessionRef.current = null;
    setDraggingChannelId(null);
    setDragOverSectionId(null);
  };

  const beginChannelDrag = (
    event: PointerEvent<HTMLElement>,
    channelId: string,
  ) => {
    if (event.button !== 0) return;
    cleanupChannelDragListeners();
    dragSessionRef.current = {
      channelId,
      startX: event.clientX,
      startY: event.clientY,
      pointerId: event.pointerId,
      dragging: false,
    };
    const handlePointerMove = (moveEvent: globalThis.PointerEvent) => {
      updateChannelDragAtPoint(
        moveEvent.clientX,
        moveEvent.clientY,
        moveEvent.pointerId,
        () => moveEvent.preventDefault(),
      );
    };
    const handlePointerUp = (upEvent: globalThis.PointerEvent) => {
      finishChannelDragAtPoint(
        upEvent.clientX,
        upEvent.clientY,
        upEvent.pointerId,
        () => upEvent.preventDefault(),
        () => upEvent.stopPropagation(),
      );
    };
    const handlePointerCancel = (cancelEvent: globalThis.PointerEvent) => {
      if (dragSessionRef.current?.pointerId === cancelEvent.pointerId) {
        cancelChannelDrag();
      }
    };
    window.addEventListener("pointermove", handlePointerMove, { passive: false });
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);
    dragListenerCleanupRef.current = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerCancel);
    };
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      /* Some webviews do not support pointer capture; window listeners still handle drag. */
    }
  };

  const updateChannelDrag = (event: PointerEvent<HTMLElement>) => {
    updateChannelDragAtPoint(
      event.clientX,
      event.clientY,
      event.pointerId,
      () => event.preventDefault(),
    );
  };

  const finishChannelDrag = (event: PointerEvent<HTMLElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    try {
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    } catch {
      /* Ignore pointer-capture differences across desktop webviews. */
    }
    finishChannelDragAtPoint(
      event.clientX,
      event.clientY,
      event.pointerId,
      () => event.preventDefault(),
      () => event.stopPropagation(),
    );
  };

  useEffect(() => {
    if (!channelContextMenu) return;
    const close = () => setChannelContextMenu(null);
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [channelContextMenu]);

  useEffect(() => () => cleanupChannelDragListeners(), []);
  return (
    <aside className="flex min-h-0 min-w-0 flex-col bg-[#fbfbfd]">
      <div className="border-b border-[#edf0f5] p-3">
        <div className="space-y-1">
          {navItems.map((item) => {
            const Icon = item.icon;
            const selected = view === item.id;
            return (
              <button
                key={item.id}
                className={cn("nav-row h-9 text-sm", selected && "nav-row-active")}
                onClick={() => {
                  closeCreateMenu();
                  setView(item.id);
                }}
              >
                <Icon size={16} />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="border-b border-[#edf0f5] p-3">
          <div className="flex items-center justify-between px-1">
            <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
              Channels
            </span>
            <div className="relative">
              <button
                type="button"
                className="composer-icon h-6 min-w-6"
                title="Create channel or section"
                aria-haspopup="menu"
                aria-expanded={createMenuOpen}
                onClick={() => {
                  if (createMenuOpen) {
                    closeCreateMenu();
                  } else {
                    setCreateMenuOpen(true);
                  }
                }}
              >
                <Plus size={15} />
              </button>
              {createMenuOpen && (
                <div className="absolute right-0 top-7 z-30 w-64 rounded-lg border border-[#dfe3ec] bg-white p-1 text-sm shadow-soft">
                  {!createKind ? (
                    <>
                      <button
                        type="button"
                        className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                        onClick={() => handleOpenCreate("channel")}
                      >
                        <Hash size={15} />
                        New channel
                      </button>
                      <button
                        type="button"
                        className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                        onClick={() => handleOpenCreate("section")}
                      >
                        <Folder size={15} />
                        New section
                      </button>
                    </>
                  ) : (
                    <form className="grid gap-2 p-2" onSubmit={handleCreateSubmit}>
                      <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-[#667085]">
                        {createKind === "channel" ? <Hash size={13} /> : <Folder size={13} />}
                        {createKind === "channel" ? "New channel" : "New section"}
                      </div>
                      <Input
                        autoFocus
                        value={createTitle}
                        onChange={(event) => setCreateTitle(event.target.value)}
                        placeholder={createKind === "channel" ? "Channel name" : "Section name"}
                        className="h-9 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                      />
                      {createKind === "channel" && connection !== "open" && (
                        <div className="text-xs font-medium text-amber-700">
                          Connect a space before creating a channel.
                        </div>
                      )}
                      <div className="flex justify-end gap-2 pt-1">
                        <Button type="button" variant="outline" size="sm" onClick={() => setCreateKind(null)}>
                          Back
                        </Button>
                        <Button
                          type="submit"
                          size="sm"
                          disabled={
                            !createTitle.trim() ||
                            (createKind === "channel" && connection !== "open")
                          }
                        >
                          Create
                        </Button>
                      </div>
                    </form>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-3 soft-scrollbar">
          {sections.map((section) => (
            <div
              key={section.id}
              data-channel-section-id={section.id}
              className={cn(
                "mb-3 rounded-lg transition-colors",
                draggingChannelId &&
                  dragOverSectionId === section.id &&
                  "channel-drop-target",
              )}
            >
              {(section.local || channelGroups.length > 0) && (
                <div className="channel-group-header group/channelgroup">
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                    onClick={() => section.local && onToggleChannelGroup(section.id)}
                    disabled={!section.local}
                  >
                    {section.local ? (
                      section.collapsed ? (
                        <ChevronDown size={13} className="-rotate-90 text-[#667085]" />
                      ) : (
                        <ChevronDown size={13} className="text-[#667085]" />
                      )
                    ) : (
                      <span className="w-[13px]" />
                    )}
                    <span className="min-w-0 truncate">{section.title}</span>
                    <span className="count-badge ml-1 h-5 min-w-5 text-[10px]">
                      {section.channels.length}
                    </span>
                  </button>
                  {section.local && (
                    <div className="flex items-center gap-1">
                      <button
                        type="button"
                        className="composer-icon h-6 min-w-6"
                        title="Rename section"
                        onClick={() => startRenameSection(section)}
                      >
                        <Pencil size={12} />
                      </button>
                      <button
                        type="button"
                        className="composer-icon h-6 min-w-6 text-red-500 hover:text-red-600"
                        title="Delete section"
                        onClick={() => {
                          setDeleteSectionId(section.id);
                          setEditingSectionId(null);
                          setSectionTitleDraft("");
                          closeDeleteChannelConfirm();
                        }}
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  )}
                </div>
              )}
              {editingSectionId === section.id && (
                <form
                  className="channel-section-editor"
                  onSubmit={(event) => submitRenameSection(event, section.id)}
                >
                  <Input
                    autoFocus
                    value={sectionTitleDraft}
                    onChange={(event) => setSectionTitleDraft(event.target.value)}
                    placeholder="Section name"
                    className="h-8 rounded-lg border-[#dfe3ec] bg-white text-xs shadow-none"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      setEditingSectionId(null);
                      setSectionTitleDraft("");
                    }}
                  >
                    Cancel
                  </Button>
                  <Button type="submit" size="sm" disabled={!sectionTitleDraft.trim()}>
                    Save
                  </Button>
                </form>
              )}
              {deleteSectionId === section.id && (
                <div className="channel-section-editor">
                  <div className="min-w-0 flex-1 text-xs font-medium text-[#667085]">
                    Delete "{section.title}"? Channels stay available.
                  </div>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => setDeleteSectionId(null)}
                  >
                    Cancel
                  </Button>
                  <Button type="button" size="sm" onClick={() => confirmDeleteSection(section.id)}>
                    Delete
                  </Button>
                </div>
              )}
              {!section.collapsed && (
                <div className="mt-1 space-y-1">
                  {section.channels.length === 0 ? (
                    <div className="px-3 py-2 text-xs text-[#8a93a5]">
                      {section.local ? "Drop channels here." : "No channels yet."}
                    </div>
                  ) : (
                    section.channels.map((channel) => {
                      const selected = channel.id === activeChannelId && !activeThreadId;
                      const threads = threadsByChannel[channel.id] ?? [];
                      const deleteBusy = busy === `channel:delete:${channel.id}`;
                      const renameBusy = busy === `channel:rename:${channel.id}`;
                      return (
                        <div
                          key={channel.id}
                          className={cn(
                            "group/channel",
                            draggingChannelId === channel.id && "opacity-45",
                          )}
                        >
                          <div className="flex items-center gap-1">
                            <button
                              className={cn(
                                "channel-row min-w-0 flex-1 touch-none select-none",
                                draggingChannelId === channel.id && "cursor-grabbing",
                                selected && "channel-row-active",
                              )}
                              onPointerDown={(event) => beginChannelDrag(event, channel.id)}
                              onPointerMove={updateChannelDrag}
                              onPointerUp={finishChannelDrag}
                              onPointerCancel={cancelChannelDrag}
                              onContextMenu={(event) =>
                                openChannelContextMenu(event, channel)
                              }
                              onClick={() => {
                                if (suppressChannelClickRef.current === channel.id) {
                                  suppressChannelClickRef.current = null;
                                  return;
                                }
                                closeCreateMenu();
                                closeDeleteChannelConfirm();
                                closeRenameChannel();
                                onSelectChannel(channel.id);
                              }}
                            >
                              <GripVertical
                                size={13}
                                className={cn(
                                  "shrink-0 text-[#98a2b3] opacity-0 transition-opacity group-hover/channel:opacity-100",
                                  selected && "text-white/70",
                                )}
                              />
                              <Hash size={15} />
                              <span className="min-w-0 flex-1 truncate">{channel.title}</span>
                              <Badge
                                variant="outline"
                                className={cn(
                                  "ml-auto h-5 border-transparent bg-[#f1efff] px-1.5 text-[10px] text-[#5843d7]",
                                  selected && "bg-white/20 text-white",
                                )}
                              >
                                {threads.length}
                              </Badge>
                            </button>
                          </div>
                          {editingChannelId === channel.id && (
                            <form
                              className="channel-section-editor"
                              onSubmit={(event) => submitRenameChannel(event, channel)}
                            >
                              <Input
                                autoFocus
                                value={channelTitleDraft}
                                onChange={(event) =>
                                  setChannelTitleDraft(event.target.value)
                                }
                                placeholder="Channel name"
                                className="h-8 rounded-lg border-[#dfe3ec] bg-white text-xs shadow-none"
                              />
                              <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                disabled={renameBusy}
                                onClick={closeRenameChannel}
                              >
                                Cancel
                              </Button>
                              <Button
                                type="submit"
                                size="sm"
                                disabled={!channelTitleDraft.trim() || renameBusy}
                              >
                                {renameBusy ? (
                                  <Loader2 className="animate-spin" size={13} />
                                ) : (
                                  "Save"
                                )}
                              </Button>
                            </form>
                          )}
                          {deleteChannelId === channel.id && (
                            <ChannelDeleteConfirm
                              channel={channel}
                              confirmTitle={deleteTitleConfirm}
                              deleteBusy={deleteBusy}
                              onCancel={closeDeleteChannelConfirm}
                              onConfirm={() => {
                                onDeleteChannel(channel);
                                closeDeleteChannelConfirm();
                              }}
                              setConfirmTitle={setDeleteTitleConfirm}
                            />
                          )}
                          {channel.id === activeChannelId && threads.length > 0 && (
                            <div className="ml-4 mt-1 space-y-1 border-l border-[#e1e5ef] pl-2">
                              {threads.map((thread) => (
                                <button
                                  key={thread.id}
                                  className={cn(
                                    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs font-medium text-[#667085] hover:bg-[#f0f1f8] hover:text-[#303849]",
                                    activeThreadId === thread.id && "bg-[#eeeaff] text-[#5843d7]",
                                  )}
                                  onClick={() => {
                                    closeCreateMenu();
                                    closeDeleteChannelConfirm();
                                    closeRenameChannel();
                                    onSelectThread(thread);
                                  }}
                                >
                                  <Split size={13} />
                                  <span className="min-w-0 flex-1 truncate">{thread.title}</span>
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                      );
                    })
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
      {channelContextMenu &&
        contextMenuChannel &&
        createPortal(
          <div
            className="fixed z-50 rounded-lg border border-[#dfe3ec] bg-white p-1 text-sm shadow-soft"
            style={{
              left: channelContextMenu.x,
              top: channelContextMenu.y,
              width: channelContextMenuWidthPx,
            }}
            role="menu"
            aria-label={`Channel actions for ${contextMenuChannel.title}`}
            onClick={(event) => event.stopPropagation()}
            onContextMenu={(event) => event.preventDefault()}
          >
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
              role="menuitem"
              onClick={() => startRenameChannel(contextMenuChannel)}
            >
              <Pencil size={14} />
              Rename
            </button>
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-red-600 hover:bg-red-50"
              role="menuitem"
              onClick={() => requestDeleteChannel(contextMenuChannel.id)}
            >
              <Trash2 size={14} />
              Delete
            </button>
          </div>,
          document.body,
        )}
    </aside>
  );
}

function ChatHeader({
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

function MessageFeed({
  actors,
  feedKey,
  machines,
  messages,
  tasksBySourceMessageId,
  channelThreads,
  threadStatsById,
  emptyText,
  onReply,
  onStartThread,
  onToggleReaction,
  onAnswerAction,
  onOpenAgentSettings,
  currentActorId,
  busy,
}: {
  actors: Record<string, Actor>;
  feedKey: string;
  machines: MachineInfo[];
  messages: Message[];
  tasksBySourceMessageId: Record<string, Task>;
  channelThreads: Thread[];
  threadStatsById: Record<string, ThreadActivityStats>;
  emptyText: string;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  onOpenAgentSettings: (actorId: string) => void;
  currentActorId: string | null;
  busy: string | null;
}) {
  const workflowSourceIds = new Set(
    messages.filter(isWorkflowMessage).map((message) => message.id),
  );
  const visibleMessages = messages.filter((message) => !isHiddenProtocolMessage(message));
  const messageGroups = groupMessagesByDate(visibleMessages);
  const messageListKey = visibleMessages
    .map((message) => `${message.id}:${message.createdAt}:${message.body.length}`)
    .join("|");
  const feedScroll = useStickToBottomScroll({
    contentKey: messageListKey,
    itemCount: visibleMessages.length,
    scrollKey: feedKey,
  });

  if (visibleMessages.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center bg-white px-8 text-sm text-muted-foreground">
        <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] px-8 py-10 text-center">
          {emptyText}
        </div>
      </div>
    );
  }
  return (
    <div
      ref={feedScroll.ref}
      className="min-h-0 flex-1 overflow-y-auto bg-white px-5 py-2 soft-scrollbar"
      onScroll={feedScroll.onScroll}
    >
      <div className="mx-auto flex max-w-4xl flex-col gap-2">
        {messageGroups.map((group) => (
          <Fragment key={group.key}>
            <div className="date-divider">
              <span />
              <div>{group.label}</div>
              <span />
            </div>
            {group.messages.map((message) => {
              const threadSummary =
                channelThreads.find((thread) => thread.rootMessageId === message.id) ?? null;
              const sourceTask =
                message.scope.kind === "channel"
                  ? tasksBySourceMessageId[message.id] ?? null
                  : null;
              return (
                <MessageRow
                  key={message.id}
                  actor={actors[message.authorActorId]}
                  actors={actors}
                  machines={machines}
                  message={message}
                  workflowSourceIds={workflowSourceIds}
                  onReply={onReply}
                  onStartThread={onStartThread}
                  onToggleReaction={onToggleReaction}
                  onAnswerAction={onAnswerAction}
                  onOpenAgentSettings={onOpenAgentSettings}
                  canStartThread={canUseAsThreadRoot(message)}
                  threadSummary={threadSummary}
                  threadStats={
                    threadSummary ? threadStatsById[threadSummary.id] : undefined
                  }
                  sourceTask={sourceTask}
                  currentActorId={currentActorId}
                  busy={busy}
                />
              );
            })}
          </Fragment>
        ))}
      </div>
    </div>
  );
}

function MessageRow({
  actor,
  actors,
  machines,
  message,
  workflowSourceIds,
  onReply,
  onStartThread,
  onToggleReaction,
  onAnswerAction,
  onOpenAgentSettings,
  canStartThread,
  threadSummary,
  threadStats,
  sourceTask,
  currentActorId,
  busy,
}: {
  actor?: Actor;
  actors: Record<string, Actor>;
  machines: MachineInfo[];
  message: Message;
  workflowSourceIds: Set<string>;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  onOpenAgentSettings: (actorId: string) => void;
  canStartThread: boolean;
  threadSummary: Thread | null;
  threadStats?: ThreadActivityStats;
  sourceTask: Task | null;
  currentActorId: string | null;
  busy: string | null;
}) {
  const actionRequest = messageKind(message) === "action.request";
  const bodyPoll = actionRequest ? null : bodyPollFromMessage(message);
  const choices = actionChoices(message);
  const pollChoices = choices.length > 0 ? choices : bodyPoll?.choices ?? [];
  const displayBody = bodyPoll?.question || message.body || metadataText(message);
  const reactions = message.reactions ?? [];
  const attachments = message.attachments ?? [];
  if (isWorkflowMessage(message)) {
    return <WorkflowEventRow actor={actor} actors={actors} message={message} />;
  }
  if (isWorkflowResultMessage(message, workflowSourceIds)) {
    return (
      <WorkflowResultRow
        actor={actor}
        machines={machines}
        message={message}
        onOpenAgentSettings={onOpenAgentSettings}
      />
    );
  }
  return (
    <article
      className={cn(
        "group rounded-xl px-4 py-3 transition-colors hover:bg-[#f7f8fb]",
        actionRequest && "border border-amber-300 bg-amber-50",
      )}
    >
      <div className="flex items-start gap-4">
        <AgentMessageAvatar
          actor={actor}
          fallback={message.authorActorId}
          machines={machines}
          onOpenAgentSettings={onOpenAgentSettings}
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold text-[#111827]">{actor ? displayName(actor) : message.authorActorId}</span>
            <span className="text-xs font-medium text-[#667085]">{formatTime(message.createdAt)}</span>
            {sourceTask && <TaskStateBadge task={sourceTask} />}
            {message.parentMessageId && (
              <span className="font-mono text-xs text-muted-foreground">
                reply {shortId(message.parentMessageId)}
              </span>
            )}
          </div>
          <div className="message-markdown mt-1 max-w-none break-words text-[15px] leading-6 text-[#111827]">
            <ReactMarkdown>{displayBody}</ReactMarkdown>
          </div>
          {attachments.length > 0 && (
            <AttachmentStack attachments={attachments} />
          )}
          {pollChoices.length > 0 && (
            <PollCard
              choices={pollChoices}
              disabled={!actionRequest || Boolean(busy?.startsWith(`action:${message.id}:`))}
              onChoose={
                actionRequest
                  ? (choice) => onAnswerAction(message, choice.id, choice.accepted)
                  : undefined
              }
            />
          )}
          {reactions.length > 0 && (
            <div className="mt-3 flex min-h-7 flex-wrap items-center gap-1.5">
              {reactions.map((reaction) => {
                const selected = Boolean(
                  currentActorId && reaction.actorIds.includes(currentActorId),
                );
                return (
                  <button
                    key={reaction.emoji}
                    type="button"
                    className={cn(
                      "reaction-chip",
                      selected
                        ? "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]"
                        : "border-[#e2e5ed] bg-white text-[#31394a]",
                    )}
                    title={reaction.actorIds
                      .map((actorId) => actorName(actors, actorId))
                      .join(", ")}
                    disabled={busy === `message:reaction:${message.id}:${reaction.emoji}`}
                    onClick={() => onToggleReaction(message, reaction.emoji)}
                  >
                    <span className="text-sm leading-none">{reaction.emoji}</span>
                    <span>{reaction.actorIds.length}</span>
                  </button>
                );
              })}
              <ReactionPicker
                busy={busy}
                compact
                message={message}
                onToggleReaction={onToggleReaction}
              />
            </div>
          )}
          {threadSummary && (
            <ThreadSummaryRow
              actors={actors}
              rootAuthor={actor}
              thread={threadSummary}
              threadStats={threadStats}
              onOpen={() => onStartThread(message)}
            />
          )}
          <div className="mt-2 flex flex-wrap gap-2 opacity-0 transition-opacity group-hover:opacity-100">
            <Button variant="ghost" size="sm" onClick={() => onReply(message)}>
              <Reply size={14} />
              Reply
            </Button>
            {reactions.length === 0 && (
              <ReactionPicker
                busy={busy}
                message={message}
                onToggleReaction={onToggleReaction}
              />
            )}
            {canStartThread && !threadSummary && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onStartThread(message)}
                disabled={busy === `thread:create:${message.id}`}
              >
                <Split size={14} />
                Thread
              </Button>
            )}
            <span className="self-center font-mono text-[11px] text-muted-foreground">
              {shortId(message.id, 10)}
            </span>
          </div>
        </div>
      </div>
    </article>
  );
}

function AttachmentStack({ attachments }: { attachments: string[] }) {
  return (
    <div className="mt-3 grid max-w-[560px] gap-2">
      {attachments.slice(0, 3).map((attachment) => (
        <AttachmentCard key={attachment} attachment={attachment} />
      ))}
    </div>
  );
}

function AttachmentCard({ attachment }: { attachment: string }) {
  const title = attachmentTitle(attachment);
  const kind = attachmentKind(attachment);
  return (
    <div className="attachment-card">
      <div className="attachment-icon">
        <FileText size={18} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-bold text-[#303849]">{title}</div>
        <div className="mt-0.5 truncate text-xs font-medium text-[#667085]">{kind}</div>
      </div>
      <div className="attachment-preview" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

function PollCard({
  choices,
  disabled,
  onChoose,
}: {
  choices: ActionChoice[];
  disabled: boolean;
  onChoose?: (choice: ActionChoice) => void;
}) {
  const totalVotes = choices.reduce((sum, choice) => sum + (choice.votes ?? 0), 0);
  const fallbackMax = choices.length;
  return (
    <div className="poll-card">
      {choices.map((choice, index) => {
        const votes = choice.votes ?? (totalVotes === 0 ? fallbackMax - index : 0);
        const denominator = totalVotes || fallbackMax || 1;
        const percent = Math.max(6, Math.round((votes / denominator) * 100));
        return (
          <button
            key={choice.id}
            type="button"
            className="poll-choice"
            disabled={disabled || !onChoose}
            onClick={() => onChoose?.(choice)}
          >
            <span className="poll-letter">{choice.id.slice(0, 1).toUpperCase()}</span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-semibold text-[#303849]">
                {choice.label}
              </span>
              <span className="mt-1 block h-0.5 overflow-hidden rounded-full bg-[#e7e9f3]">
                <span
                  className="block h-full rounded-full bg-[#5a47e9]"
                  style={{ width: `${percent}%` }}
                />
              </span>
            </span>
            <span className="w-8 text-right text-sm font-bold text-[#303849]">
              {votes}
            </span>
          </button>
        );
      })}
      <div className="mt-2 flex items-center gap-2 px-1 text-xs font-medium text-[#667085]">
        <span>{totalVotes || choices.length} votes</span>
        <span>•</span>
        <span>Poll closes soon</span>
      </div>
    </div>
  );
}

function ThreadSummaryRow({
  actors,
  rootAuthor,
  thread,
  threadStats,
  onOpen,
}: {
  actors: Record<string, Actor>;
  rootAuthor?: Actor;
  thread: Thread;
  threadStats?: ThreadActivityStats;
  onOpen: () => void;
}) {
  const participants = threadParticipants(thread, actors, rootAuthor, threadStats);
  const replyCount = threadReplyCount(thread, threadStats);
  const lastReply = threadLastReplyLabel(thread, threadStats);
  return (
    <button type="button" className="thread-summary-row" onClick={onOpen}>
      <AvatarStack actors={participants} max={4} small />
      <span className="min-w-0 truncate text-xs font-bold text-[#503ed4]">
        {typeof replyCount === "number"
          ? `${replyCount}${threadStats?.hasMoreReplies ? "+" : ""} ${
              replyCount === 1 ? "reply" : "replies"
            }`
          : "Thread"}
      </span>
      {lastReply && (
        <span className="shrink-0 text-xs font-medium text-[#667085]">
          Last reply {lastReply}
        </span>
      )}
    </button>
  );
}

function TaskStateBadge({ task }: { task: Task }) {
  return (
    <Badge
      variant="outline"
      title={task.id}
      className={cn("whitespace-nowrap font-semibold", taskStatusBadgeClass(task.status))}
    >
      Task #{task.number} · {task.status}
    </Badge>
  );
}

function WorkflowEventRow({
  actor,
  actors,
  message,
}: {
  actor?: Actor;
  actors: Record<string, Actor>;
  message: Message;
}) {
  const summary = workflowSummary(message, actors);
  return (
    <div className="mx-auto flex max-w-[80%] items-center gap-2 rounded-xl border border-[#dfe3ec] bg-[#f7f8fb] px-3 py-2 text-xs text-[#667085]">
      <Check size={14} />
      <span className="min-w-0 flex-1 truncate">{summary}</span>
      <span>{formatTime(message.createdAt)}</span>
      {actor && <Badge variant="outline">{displayName(actor)}</Badge>}
    </div>
  );
}

function WorkflowResultRow({
  actor,
  machines,
  message,
  onOpenAgentSettings,
}: {
  actor?: Actor;
  machines: MachineInfo[];
  message: Message;
  onOpenAgentSettings: (actorId: string) => void;
}) {
  return (
    <article className="group rounded-xl px-4 py-3 transition-colors hover:bg-[#f7f8fb]">
      <div className="flex items-start gap-4">
        <AgentMessageAvatar
          actor={actor}
          fallback={message.authorActorId}
          machines={machines}
          onOpenAgentSettings={onOpenAgentSettings}
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold text-[#111827]">{actor ? displayName(actor) : message.authorActorId}</span>
            <span className="text-xs text-muted-foreground">{formatTime(message.createdAt)}</span>
            <Badge variant="success">task result</Badge>
          </div>
          <div className="mt-1 text-sm leading-6 text-[#303849]">{workflowResultSummary(message)}</div>
        </div>
      </div>
    </article>
  );
}

function Composer({
  draft,
  setDraft,
  disabled,
  replyTo,
  actorName,
  onClearReply,
  onSend,
  mentionAgents,
  busy,
}: {
  draft: string;
  setDraft: (value: string) => void;
  disabled: boolean;
  replyTo: Message | null;
  actorName: string;
  onClearReply: () => void;
  onSend: () => void;
  mentionAgents: Actor[];
  busy: boolean;
}) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [caretIndex, setCaretIndex] = useState(draft.length);
  const [selectedMentionIndex, setSelectedMentionIndex] = useState(0);
  const [dismissedMentionKey, setDismissedMentionKey] = useState<string | null>(null);
  const activeMention = activeMentionQuery(draft, caretIndex);
  const mentionKey = activeMention
    ? `${activeMention.start}:${activeMention.end}:${activeMention.query}`
    : null;
  const mentionOptions = activeMention
    ? mentionCandidates(mentionAgents, activeMention)
    : [];
  const showMentions =
    !disabled &&
    !busy &&
    activeMention !== null &&
    dismissedMentionKey !== mentionKey &&
    mentionOptions.length > 0;
  const effectiveMentionIndex = mentionOptions.length
    ? Math.min(selectedMentionIndex, mentionOptions.length - 1)
    : 0;
  const selectedMention = showMentions ? mentionOptions[effectiveMentionIndex] : null;

  useEffect(() => {
    setSelectedMentionIndex(0);
  }, [mentionKey]);

  function syncCaret(element: HTMLTextAreaElement) {
    setCaretIndex(element.selectionStart ?? element.value.length);
  }

  function chooseMention(option: MentionOption) {
    const before = draft.slice(0, option.start);
    const after = draft.slice(option.end).replace(/^\s*/, "");
    const next = `${before}${option.token} ${after}`;
    const nextCaret = before.length + option.token.length + 1;
    setDraft(next);
    setCaretIndex(nextCaret);
    setDismissedMentionKey(null);
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCaret, nextCaret);
    });
  }

  return (
    <footer className="border-t border-[#e2e6ef] bg-white px-5 py-4">
      <div className="mx-auto max-w-4xl">
        {replyTo && (
          <div className="mb-2 flex items-center gap-2 rounded-lg border border-[#dfe3ec] bg-[#f7f8fb] px-3 py-2 text-xs text-[#667085]">
            <span className="min-w-0 flex-1 truncate">Replying to {actorName}</span>
            <button onClick={onClearReply}>
              <X size={14} />
            </button>
          </div>
        )}
        <div className="composer-box relative">
          {showMentions && (
            <div className="absolute bottom-[calc(100%+8px)] left-0 z-20 w-full max-w-xl overflow-hidden rounded-xl border border-[#dfe3ec] bg-white shadow-soft">
              <div className="border-b border-[#edf0f5] px-3 py-2 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
                Mentions
              </div>
              <div className="max-h-64 overflow-y-auto py-1 scrollbar-thin">
                {mentionOptions.map((option, index) => (
                  <button
                    key={`${option.kind}:${option.id}`}
                    type="button"
                    className={cn(
                      "flex w-full items-center gap-3 px-3 py-2 text-left text-sm transition-colors",
                      index === effectiveMentionIndex
                        ? "bg-[#f1efff] text-[#5843d7]"
                        : "hover:bg-[#f7f8fb]",
                    )}
                    onMouseDown={(event) => {
                      event.preventDefault();
                      chooseMention(option);
                    }}
                  >
                    {option.actor ? (
                      <ActorAvatar actor={option.actor} fallback={option.actor.id} small />
                    ) : (
                      <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-primary/15 text-primary">
                        <Users size={14} />
                      </span>
                    )}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{option.title}</span>
                      <span className="block truncate text-xs text-muted-foreground">
                        {option.detail}
                      </span>
                    </span>
                    <span className="font-mono text-xs text-muted-foreground">
                      {option.token}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}
          <Textarea
            ref={textareaRef}
            value={draft}
            onChange={(event) => {
              setDraft(event.target.value);
              syncCaret(event.currentTarget);
              setDismissedMentionKey(null);
            }}
            onClick={(event) => syncCaret(event.currentTarget)}
            onKeyUp={(event) => syncCaret(event.currentTarget)}
            onKeyDown={(event) => {
              if (showMentions) {
                if (event.key === "ArrowDown") {
                  event.preventDefault();
                  setSelectedMentionIndex((index) =>
                    (index + 1) % mentionOptions.length,
                  );
                  return;
                }
                if (event.key === "ArrowUp") {
                  event.preventDefault();
                  setSelectedMentionIndex((index) =>
                    (index - 1 + mentionOptions.length) % mentionOptions.length,
                  );
                  return;
                }
                if ((event.key === "Enter" || event.key === "Tab") && selectedMention) {
                  event.preventDefault();
                  chooseMention(selectedMention);
                  return;
                }
                if (event.key === "Escape") {
                  event.preventDefault();
                  setDismissedMentionKey(mentionKey);
                  return;
                }
              }
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                onSend();
              }
            }}
            disabled={disabled}
            placeholder={disabled ? "Connect and select a channel" : "Message"}
            className="max-h-48 min-h-[44px] flex-1 border-0 bg-transparent px-0 py-1 shadow-none focus-visible:ring-0"
          />
          <Button
            size="icon"
            onClick={onSend}
            disabled={disabled || !draft.trim() || busy}
            className="h-9 w-9 rounded-lg"
          >
            {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
          </Button>
        </div>
      </div>
    </footer>
  );
}

function ThreadPanel({
  actors,
  channel,
  channelMessages,
  currentActorId,
  disabled,
  draft,
  machines,
  messages,
  setDraft,
  task,
  thread,
  busy,
  className,
  onClose,
  onSend,
  onToggleReaction,
  onOpenAgentSettings,
}: {
  actors: Record<string, Actor>;
  channel: Channel | null;
  channelMessages: Message[];
  currentActorId: string | null;
  disabled: boolean;
  draft: string;
  machines: MachineInfo[];
  messages: Message[];
  setDraft: (value: string) => void;
  task: Task | null;
  thread: Thread | null;
  busy: string | null;
  className?: string;
  onClose: () => void;
  onSend: () => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onOpenAgentSettings: (actorId: string) => void;
}) {
  const rootMessage = thread
    ? channelMessages.find((message) => message.id === thread.rootMessageId) ?? null
    : null;
  const replyMessages = messages.filter(
    (message) =>
      !isHiddenProtocolMessage(message) &&
      (!rootMessage || message.id !== rootMessage.id),
  );
  const replyGroups = groupMessagesByDate(replyMessages);
  const starter = rootMessage ? actors[rootMessage.authorActorId] : undefined;
  const threadScrollKey = thread?.id ?? "thread:none";
  const threadContentKey = [
    rootMessage
      ? `${rootMessage.id}:${rootMessage.createdAt}:${rootMessage.body.length}`
      : "root:none",
    ...replyMessages.map(
      (message) => `${message.id}:${message.createdAt}:${message.body.length}`,
    ),
  ].join("|");
  const threadScroll = useStickToBottomScroll({
    contentKey: threadContentKey,
    itemCount: replyMessages.length + (rootMessage ? 1 : 0),
    scrollKey: threadScrollKey,
  });
  return (
    <aside
      className={cn(
        "min-h-0 min-w-0 flex-col bg-white",
        className ?? "hidden border-l border-[#e2e6ef] xl:flex",
      )}
    >
      <div className="flex min-h-[86px] shrink-0 items-center border-b border-[#e2e6ef] bg-white px-5 py-3">
        <div className="flex min-w-0 flex-1 items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="min-w-0 truncate text-lg font-bold text-[#111827]">
              {thread?.title ?? "Thread"}
            </div>
            <div className="mt-0.5 truncate text-sm text-[#485063]">
              {thread
                ? starter
                  ? `Started by ${displayName(starter)} in #${channel?.title ?? "channel"}`
                  : `#${channel?.title ?? "channel"}`
                : "Select a thread"}
            </div>
            {task && (
              <div className="mt-2 flex">
                <TaskStateBadge task={task} />
              </div>
            )}
          </div>
          <div className="flex items-center gap-1">
            <button className="composer-icon" type="button" title="Close" onClick={onClose}>
              <X size={16} />
            </button>
          </div>
        </div>
      </div>

      <div
        ref={threadScroll.ref}
        className="min-h-0 flex-1 overflow-y-auto bg-white soft-scrollbar"
        onScroll={threadScroll.onScroll}
      >
        {!thread ? (
          <div className="p-4">
            <EmptyState icon={Split} text="Select a thread." />
          </div>
        ) : (
          <div>
            <section className="border-b border-[#edf0f5] bg-white px-5 py-4">
              <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-[#667085]">
                <Split size={13} />
                Original message
              </div>
              {rootMessage ? (
                <ThreadConversationMessage
                  actor={starter}
                  actors={actors}
                  busy={busy}
                  currentActorId={currentActorId}
                  machines={machines}
                  message={rootMessage}
                  onOpenAgentSettings={onOpenAgentSettings}
                  onToggleReaction={onToggleReaction}
                  root
                />
              ) : (
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] px-4 py-6">
                  <MutedLine>Original message unavailable.</MutedLine>
                </div>
              )}
            </section>

            <section className="bg-white px-5 py-2">
              {replyMessages.length === 0 ? (
                <div className="py-8">
                  <EmptyState icon={MessageSquare} text="No replies in this thread." />
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  {replyGroups.map((group) => (
                    <Fragment key={group.key}>
                      <div className="date-divider px-0">
                        <span />
                        <div>{group.label}</div>
                        <span />
                      </div>
                      {group.messages.map((message) => (
                        <ThreadConversationMessage
                          key={message.id}
                          actor={actors[message.authorActorId]}
                          actors={actors}
                          busy={busy}
                          currentActorId={currentActorId}
                          machines={machines}
                          message={message}
                          onOpenAgentSettings={onOpenAgentSettings}
                          onToggleReaction={onToggleReaction}
                        />
                      ))}
                    </Fragment>
                  ))}
                </div>
              )}
            </section>
          </div>
        )}
      </div>

      <ThreadComposer
        draft={draft}
        setDraft={setDraft}
        disabled={disabled || !thread}
        busy={busy === "thread:message:send"}
        onSend={onSend}
      />
    </aside>
  );
}

function ThreadConversationMessage({
  actor,
  actors,
  currentActorId,
  machines,
  message,
  busy,
  root = false,
  onOpenAgentSettings,
  onToggleReaction,
}: {
  actor?: Actor;
  actors: Record<string, Actor>;
  currentActorId: string | null;
  machines: MachineInfo[];
  message: Message;
  busy: string | null;
  root?: boolean;
  onOpenAgentSettings: (actorId: string) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
}) {
  const reactions = message.reactions ?? [];
  const bodyPoll = bodyPollFromMessage(message);
  const choices = actionChoices(message);
  const pollChoices = choices.length > 0 ? choices : bodyPoll?.choices ?? [];
  const displayBody = bodyPoll?.question || message.body || metadataText(message);
  const attachments = message.attachments ?? [];
  return (
    <article
      className={cn(
        "group rounded-xl px-4 py-3 transition-colors",
        root ? "bg-[#fbfbfd]" : "hover:bg-[#f7f8fb]",
      )}
    >
      <div className="flex items-start gap-4">
        <AgentMessageAvatar
          actor={actor}
          fallback={message.authorActorId}
          machines={machines}
          onOpenAgentSettings={onOpenAgentSettings}
          preferredPlacement="left"
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold text-[#111827]">
              {actor ? displayName(actor) : message.authorActorId}
            </span>
            <span className="text-xs font-medium text-[#667085]">
              {formatTime(message.createdAt)}
            </span>
          </div>
          <div className="message-markdown mt-1 max-w-none break-words text-[15px] leading-6 text-[#111827]">
            <ReactMarkdown>{displayBody}</ReactMarkdown>
          </div>
          {pollChoices.length > 0 && (
            <PollCard choices={pollChoices} disabled />
          )}
          {attachments.length > 0 && (
            <AttachmentStack attachments={attachments} />
          )}
          {reactions.length > 0 ? (
            <div className="mt-3 flex min-h-7 flex-wrap items-center gap-1.5">
              {reactions.map((reaction) => {
                const selected = Boolean(
                  currentActorId && reaction.actorIds.includes(currentActorId),
                );
                return (
                  <button
                    key={reaction.emoji}
                    type="button"
                    className={cn(
                      "reaction-chip",
                      selected
                        ? "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]"
                        : "border-[#e2e5ed] bg-white text-[#31394a]",
                    )}
                    title={reaction.actorIds
                      .map((actorId) => actorName(actors, actorId))
                      .join(", ")}
                    disabled={busy === `message:reaction:${message.id}:${reaction.emoji}`}
                    onClick={() => onToggleReaction(message, reaction.emoji)}
                  >
                    <span className="text-sm leading-none">{reaction.emoji}</span>
                    <span>{reaction.actorIds.length}</span>
                  </button>
                );
              })}
              <ReactionPicker
                busy={busy}
                compact
                message={message}
                onToggleReaction={onToggleReaction}
              />
            </div>
          ) : (
            <div className="mt-2 flex flex-wrap gap-2 opacity-0 transition-opacity group-hover:opacity-100">
              <ReactionPicker
                busy={busy}
                message={message}
                onToggleReaction={onToggleReaction}
              />
            </div>
          )}
        </div>
      </div>
    </article>
  );
}

function ReactionPicker({
  busy,
  compact = false,
  message,
  onToggleReaction,
}: {
  busy: string | null;
  compact?: boolean;
  message: Message;
  onToggleReaction: (message: Message, emoji: string) => void;
}) {
  return (
    <div className={cn("reaction-picker", compact && "h-7")}>
      <button
        type="button"
        className={cn(
          "composer-icon reaction-picker-trigger rounded-full",
          compact ? "h-7 min-w-7" : "h-8 min-w-8",
        )}
        title="Add reaction"
        aria-label="Add reaction"
        aria-haspopup="true"
      >
        <Smile size={compact ? 14 : 15} />
      </button>
      <div className="reaction-picker-menu" role="menu" aria-label="Choose reaction">
        {supportedReactionEmojis.map((emoji) => (
          <button
            key={emoji}
            type="button"
            className={cn("reaction-picker-option", compact && "h-7 w-7 text-sm")}
            title={`React ${emoji}`}
            aria-label={`React ${emoji}`}
            disabled={busy === `message:reaction:${message.id}:${emoji}`}
            onClick={() => onToggleReaction(message, emoji)}
            role="menuitem"
          >
            {emoji}
          </button>
        ))}
      </div>
    </div>
  );
}

function ThreadComposer({
  draft,
  setDraft,
  disabled,
  busy,
  onSend,
}: {
  draft: string;
  setDraft: (value: string) => void;
  disabled: boolean;
  busy: boolean;
  onSend: () => void;
}) {
  return (
    <footer className="shrink-0 border-t border-[#edf0f5] bg-white p-4">
      <div className="composer-box composer-box-compact relative">
        <Textarea
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              onSend();
            }
          }}
          disabled={disabled}
          placeholder={disabled ? "Select a thread" : "Reply in thread..."}
          className="max-h-36 min-h-[42px] flex-1 border-0 bg-transparent px-0 py-1 text-sm shadow-none focus-visible:ring-0"
        />
        <Button
          size="icon"
          onClick={onSend}
          disabled={disabled || !draft.trim() || busy}
          className="h-9 w-9 rounded-lg bg-[#503ed4] text-white hover:bg-[#4635c5]"
        >
          {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
        </Button>
      </div>
    </footer>
  );
}

function ThreadsView({
  actors,
  channels,
  messages,
  machines,
  threadMessages,
  threadStatsById,
  threads,
  activeChannelId,
  activeThread,
  activeThreadTask,
  currentActorId,
  threadDraft,
  setThreadDraft,
  onSelectThread,
  onCloseThread,
  onSendThreadMessage,
  onToggleReaction,
  onOpenAgentSettings,
  busy,
  disabled,
}: {
  actors: Record<string, Actor>;
  channels: Channel[];
  messages: Message[];
  machines: MachineInfo[];
  threadMessages: Message[];
  threadStatsById: Record<string, ThreadActivityStats>;
  threads: ThreadWithChannel[];
  activeChannelId: string | null;
  activeThread: Thread | null;
  activeThreadTask: Task | null;
  currentActorId: string | null;
  threadDraft: string;
  setThreadDraft: (value: string) => void;
  onSelectThread: (thread: Thread) => void;
  onCloseThread: () => void;
  onSendThreadMessage: () => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onOpenAgentSettings: (actorId: string) => void;
  busy: string | null;
  disabled: boolean;
}) {
  const [query, setQuery] = useState("");
  const activeChannel = activeChannelId
    ? channels.find((channel) => channel.id === activeChannelId) ?? null
    : null;
  const filteredThreads = threads.filter((thread) => {
    const text = `${thread.title} ${thread.channel.title}`.toLowerCase();
    return text.includes(query.trim().toLowerCase());
  });
  const rootMessagesById = new Map(messages.map((message) => [message.id, message]));
  return (
    <section className="flex min-h-0 flex-1 flex-col bg-white">
      <div className="flex h-[96px] shrink-0 items-center justify-between border-b border-[#e2e6ef] bg-white px-6">
        <div>
          <h1 className="text-[22px] font-bold text-[#111827]">All Threads</h1>
          <p className="mt-1 text-sm text-[#485063]">
            Track and resolve conversations across all channels.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <label className="search-pill h-10 w-[250px]">
            <Search size={16} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search threads"
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[#8a93a5]"
            />
          </label>
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(420px,1fr)_460px] bg-[#fbfbfd]">
        <div className="min-h-0 overflow-y-auto border-r border-[#e2e6ef] p-4 soft-scrollbar">
          <div className="mb-4 flex items-center justify-between gap-2 text-xs font-semibold text-[#667085]">
            <span>{filteredThreads.length} threads</span>
            <span>Newest activity first</span>
          </div>
          <div className="space-y-2">
            {filteredThreads.map((thread) => (
              <ThreadListCard
                key={thread.id}
                actors={actors}
                rootMessage={rootMessagesById.get(thread.rootMessageId) ?? null}
                replyCount={
                  activeThread?.id === thread.id
                    ? threadMessages.length
                    : threadStatsById[thread.id]?.replyCount
                }
                selected={activeThread?.id === thread.id}
                thread={thread}
                threadStats={threadStatsById[thread.id]}
                onSelect={() => onSelectThread(thread)}
              />
            ))}
            {filteredThreads.length === 0 && <EmptyState icon={Split} text="No threads." />}
          </div>
        </div>
        <ThreadPanel
          actors={actors}
          channel={activeChannel}
          channelMessages={messages}
          currentActorId={currentActorId}
          disabled={disabled}
          draft={threadDraft}
          machines={machines}
          messages={threadMessages}
          setDraft={setThreadDraft}
          task={activeThreadTask}
          thread={activeThread}
          busy={busy}
          className="flex border-l border-[#e2e6ef] xl:flex"
          onClose={onCloseThread}
          onSend={onSendThreadMessage}
          onToggleReaction={onToggleReaction}
          onOpenAgentSettings={onOpenAgentSettings}
        />
      </div>
    </section>
  );
}

function ThreadListCard({
  actors,
  rootMessage,
  replyCount,
  selected,
  thread,
  threadStats,
  onSelect,
}: {
  actors: Record<string, Actor>;
  rootMessage: Message | null;
  replyCount?: number;
  selected: boolean;
  thread: ThreadWithChannel;
  threadStats?: ThreadActivityStats;
  onSelect: () => void;
}) {
  const starter = rootMessage ? actors[rootMessage.authorActorId] : null;
  const participants = threadParticipants(thread, actors, starter ?? undefined, threadStats);
  const effectiveReplyCount = replyCount ?? threadReplyCount(thread, threadStats);
  const preview = rootMessage
    ? rootMessage.body || metadataText(rootMessage)
    : "Open the conversation to review the latest replies.";
  return (
    <button
      type="button"
      className={cn("thread-list-card", selected && "thread-list-card-active")}
      onClick={onSelect}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold text-[#596174]">
            <Hash size={13} />
            <span className="truncate">{thread.channel.title}</span>
          </div>
          <div className="truncate text-base font-bold text-[#111827]">{thread.title}</div>
        </div>
        <span className="shrink-0 text-xs font-medium text-[#667085]">
          {rootMessage ? formatTime(rootMessage.createdAt) : "Thread"}
        </span>
      </div>
      <p className="mt-2 line-clamp-2 text-sm leading-5 text-[#485063]">
        {starter ? `${displayName(starter)}: ` : ""}
        {preview}
      </p>
      <div className="mt-3 flex items-center gap-2">
        <AvatarStack actors={participants} max={5} small />
        <span className="rounded-full bg-[#f1efff] px-2 py-1 text-xs font-bold text-[#5843d7]">
          {typeof effectiveReplyCount === "number"
            ? `${Math.max(0, effectiveReplyCount)}${threadStats?.hasMoreReplies ? "+" : ""} ${
                effectiveReplyCount === 1 ? "reply" : "replies"
              }`
            : participants.length > 0
              ? `${participants.length} ${
                  participants.length === 1 ? "participant" : "participants"
                }`
              : "Thread"}
        </span>
        <span className="rounded-full border border-[#dfe3ec] bg-white px-2 py-1 text-xs font-semibold text-[#667085]">
          {shortId(thread.rootMessageId, 5)}
        </span>
      </div>
    </button>
  );
}

function ChannelsView({
  actors,
  busy,
  channels,
  channelGroups,
  threadsByChannel,
  activeChannel,
  onSelectChannel,
  onDeleteChannel,
}: {
  actors: Record<string, Actor>;
  busy: string | null;
  channels: Channel[];
  channelGroups: ChannelGroup[];
  threadsByChannel: Record<string, Thread[]>;
  activeChannel: Channel | null;
  onSelectChannel: (channelId: string) => void;
  onDeleteChannel: (channel: Channel) => void;
}) {
  const [query, setQuery] = useState("");
  const [deleteChannelId, setDeleteChannelId] = useState<string | null>(null);
  const [deleteTitleConfirm, setDeleteTitleConfirm] = useState("");
  const filteredChannels = channels.filter((channel) => {
    const text = `${channel.title} ${channel.topic ?? ""} ${channel.visibility}`.toLowerCase();
    return text.includes(query.trim().toLowerCase());
  });
  const sections = channelGroupSections(channelGroups, filteredChannels);
  const closeDeleteConfirm = () => {
    setDeleteChannelId(null);
    setDeleteTitleConfirm("");
  };
  return (
    <section className="flex min-h-0 flex-1 flex-col bg-white">
      <div className="flex h-[96px] shrink-0 items-center justify-between border-b border-[#e2e6ef] bg-white px-6">
        <div>
          <h1 className="text-[22px] font-bold text-[#111827]">Channels</h1>
          <p className="mt-1 text-sm text-[#485063]">
            Durable spaces for teams and topics. Create channels or sections from the sidebar plus.
          </p>
        </div>
        <div className="text-sm font-semibold text-[#667085]">
          {filteredChannels.length} visible
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto bg-[#fbfbfd] p-6 soft-scrollbar">
        <label className="mb-5 flex h-10 max-w-[340px] items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-[#667085]">
          <Search size={16} />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search channels"
            className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[#8a93a5]"
          />
        </label>
        <div className="channel-table">
          <div className="channel-table-head">
            <span>Channel</span>
            <span>Type</span>
            <span>Members</span>
            <span>Activity</span>
            <span className="text-right">Actions</span>
          </div>
          {sections.map((section) => (
            <div key={section.id}>
              {(section.local || channelGroups.length > 0) && (
                <div className="channel-table-group">
                  {section.title}
                  <span>({section.channels.length} channels)</span>
                </div>
              )}
              {section.channels.map((channel) => {
                const threads = threadsByChannel[channel.id] ?? [];
                const deleteBusy = busy === `channel:delete:${channel.id}`;
                return (
                  <Fragment key={channel.id}>
                    <ChannelTableRow
                      actors={actors}
                      channel={channel}
                      deleteBusy={deleteBusy}
                      selected={activeChannel?.id === channel.id}
                      threads={threads}
                      onRequestDelete={() => {
                        setDeleteChannelId(channel.id);
                        setDeleteTitleConfirm("");
                      }}
                      onSelect={() => onSelectChannel(channel.id)}
                    />
                    {deleteChannelId === channel.id && (
                      <ChannelDeleteConfirm
                        channel={channel}
                        confirmTitle={deleteTitleConfirm}
                        deleteBusy={deleteBusy}
                        onCancel={closeDeleteConfirm}
                        onConfirm={() => {
                          onDeleteChannel(channel);
                          closeDeleteConfirm();
                        }}
                        setConfirmTitle={setDeleteTitleConfirm}
                      />
                    )}
                  </Fragment>
                );
              })}
            </div>
          ))}
          {filteredChannels.length === 0 && <EmptyState icon={Hash} text="No channels." />}
        </div>
      </div>
    </section>
  );
}

function ChannelTableRow({
  actors,
  channel,
  deleteBusy,
  selected,
  threads,
  onRequestDelete,
  onSelect,
}: {
  actors: Record<string, Actor>;
  channel: Channel;
  deleteBusy: boolean;
  selected: boolean;
  threads: Thread[];
  onRequestDelete: () => void;
  onSelect: () => void;
}) {
  const members = channel.members.map((actorId) => actors[actorId] ?? fallbackActor(actorId));
  return (
    <div
      role="button"
      tabIndex={0}
      className={cn("channel-table-row", selected && "channel-table-row-active")}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      <span className="flex min-w-0 items-center gap-3">
        <span className="channel-icon">
          <Hash size={17} />
        </span>
        <span className="min-w-0 text-left">
          <span className="block truncate text-sm font-bold text-[#111827]">
            # {channel.title}
          </span>
          <span className="block truncate text-xs text-[#596174]">
            {channelTopic(channel) || `${threads.length} active threads`}
          </span>
        </span>
      </span>
      <span className="flex items-center gap-1 text-xs font-medium text-[#667085]">
        {channel.visibility === "private" && <Lock size={12} />}
        {capitalize(channel.visibility)}
      </span>
      <span className="flex items-center">
        <AvatarStack actors={members} max={4} small />
        <span className="ml-2 text-xs font-semibold text-[#596174]">{members.length}</span>
      </span>
      <span className="flex items-center gap-2 text-xs font-medium text-[#667085]">
        <Clock size={13} />
        {threads.length > 0 ? `${threads.length} threads` : "No threads"}
      </span>
      <span className="flex justify-end">
        <button
          type="button"
          title={`Delete #${channel.title}`}
          disabled={deleteBusy}
          className="composer-icon h-8 min-w-8 text-red-500 hover:text-red-600"
          onKeyDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            onRequestDelete();
          }}
        >
          {deleteBusy ? <Loader2 className="animate-spin" size={14} /> : <Trash2 size={14} />}
        </button>
      </span>
    </div>
  );
}

function ChannelDeleteConfirm({
  channel,
  confirmTitle,
  deleteBusy,
  onCancel,
  onConfirm,
  setConfirmTitle,
}: {
  channel: Channel;
  confirmTitle: string;
  deleteBusy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  setConfirmTitle: (value: string) => void;
}) {
  const titleMatches = confirmTitle.trim() === channel.title;
  return (
    <div className="my-2 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-900">
      <div className="flex items-start gap-3">
        <Trash2 className="mt-0.5 shrink-0 text-red-500" size={16} />
        <div className="min-w-0 flex-1">
          <div className="font-bold">Delete #{channel.title}?</div>
          <div className="mt-1 text-xs font-medium text-red-700">
            This will delete the channel and all threads in this channel.
          </div>
          <Input
            autoFocus
            value={confirmTitle}
            onChange={(event) => setConfirmTitle(event.target.value)}
            placeholder={`Type ${channel.title} to confirm`}
            className="mt-3 h-9 rounded-lg border-red-200 bg-white text-sm text-red-900 shadow-none placeholder:text-red-300"
          />
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button type="button" variant="outline" size="sm" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={deleteBusy || !titleMatches}
            className="bg-red-600 text-white hover:bg-red-700"
            onClick={onConfirm}
          >
            {deleteBusy ? <Loader2 className="animate-spin" size={13} /> : <Trash2 size={13} />}
            Delete
          </Button>
        </div>
      </div>
    </div>
  );
}

function InboxView({
  actors,
  inbox,
  onOpen,
  onAnswer,
  busy,
}: {
  actors: Record<string, Actor>;
  inbox: InboxListEntry[];
  onOpen: (message: Message | null | undefined) => void;
  onAnswer: (message: Message, optionId: string, accepted: boolean) => void;
  busy: string | null;
}) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Inbox" detail={`${inbox.length} pending deliveries`} />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto max-w-3xl space-y-3">
          {inbox.length === 0 ? (
            <EmptyState icon={Bell} text="No pending deliveries." />
          ) : (
            inbox.map((item) => {
              const message = item.message;
              return (
                <div
                  key={item.delivery.sourceId}
                  className="rounded-md border border-border bg-card p-4"
                >
                  <div className="mb-2 flex items-center gap-2">
                    <Badge variant="warning">{item.delivery.state}</Badge>
                    <span className="font-mono text-xs text-muted-foreground">
                      {shortId(item.delivery.sourceId)}
                    </span>
                  </div>
                  {message ? (
                    <>
                      <div className="text-sm font-medium">
                        {messageTitle(message)}
                      </div>
                      <div className="mt-1 text-sm text-muted-foreground">
                        From {actorName(actors, message.authorActorId)}
                      </div>
                      <div className="mt-3 flex flex-wrap gap-2">
                        <Button variant="outline" size="sm" onClick={() => onOpen(message)}>
                          Open
                        </Button>
                        {actionChoices(message).map((choice) => (
                          <Button
                            key={choice.id}
                            size="sm"
                            variant={choice.accepted ? "default" : "outline"}
                            disabled={busy?.startsWith(`action:${message.id}:`)}
                            onClick={() => onAnswer(message, choice.id, choice.accepted)}
                          >
                            {choice.label}
                          </Button>
                        ))}
                      </div>
                    </>
                  ) : (
                    <div className="text-sm text-muted-foreground">Message unavailable.</div>
                  )}
                </div>
              );
            })
          )}
        </div>
      </div>
    </section>
  );
}

function TasksView({
  tasks,
  channels,
}: {
  tasks: Task[];
  channels: Channel[];
}) {
  const channelsById = Object.fromEntries(channels.map((channel) => [channel.id, channel]));
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Tasks" detail={`${tasks.length} tasks`} />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto max-w-4xl space-y-2">
          {tasks.length === 0 ? (
            <EmptyState icon={Check} text="No tasks." />
          ) : (
            tasks.map((task) => (
              <div
                key={task.id}
                className="grid gap-3 rounded-md border border-border bg-card p-4 sm:grid-cols-[1fr_auto]"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="secondary">#{task.number}</Badge>
                    <span className="truncate font-medium">{task.title}</span>
                  </div>
                  <div className="mt-1 line-clamp-2 text-sm text-muted-foreground">
                    {task.description || task.resultSummary || "No description."}
                  </div>
                </div>
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <Badge
                    variant="outline"
                    className={cn("font-semibold", taskStatusBadgeClass(task.status))}
                  >
                    {task.status}
                  </Badge>
                  <span>{channelsById[task.channelId]?.title ?? task.channelId}</span>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </section>
  );
}

function SpacesView({
  busy,
  connection,
  workspace,
  workspaceForm,
  setWorkspaceForm,
  workspaces,
  onAddWorkspace,
  onRemoveWorkspace,
  onSelectWorkspace,
}: {
  busy: string | null;
  connection: ConnectionState;
  workspace: Workspace | null;
  workspaceForm: { name: string; serverUrl: string };
  setWorkspaceForm: (form: { name: string; serverUrl: string }) => void;
  workspaces: Workspace[];
  onAddWorkspace: () => void;
  onRemoveWorkspace: (id: string) => void;
  onSelectWorkspace: (workspaceId: string) => void;
}) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Spaces" detail="Choose or add a server connection" />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        <div className="mx-auto max-w-4xl space-y-4">
          <SettingsSection title="Add Space" detail="Save a connection target in the side rail.">
            <form
              className="grid gap-3 sm:grid-cols-[180px_1fr_auto]"
              onSubmit={(event) => {
                event.preventDefault();
                onAddWorkspace();
              }}
            >
              <Input
                value={workspaceForm.name}
                onChange={(event) =>
                  setWorkspaceForm({ ...workspaceForm, name: event.target.value })
                }
                placeholder="Name"
              />
              <Input
                value={workspaceForm.serverUrl}
                onChange={(event) =>
                  setWorkspaceForm({ ...workspaceForm, serverUrl: event.target.value })
                }
                placeholder="ws://127.0.0.1:7878/rpc"
              />
              <Button
                type="submit"
                disabled={
                  busy === "workspace:add" ||
                  !workspaceForm.name.trim() ||
                  !workspaceForm.serverUrl.trim()
                }
              >
                {busy === "workspace:add" ? (
                  <Loader2 className="animate-spin" size={15} />
                ) : (
                  <Plus size={15} />
                )}
                Add Space
              </Button>
            </form>
          </SettingsSection>

          <SettingsSection title="Saved Spaces" detail="The side rail uses this list for switching.">
            <div className="space-y-2">
              {workspaces.length === 0 ? (
                <MutedLine>No spaces configured.</MutedLine>
              ) : (
                workspaces.map((item) => {
                  const selected = item.id === workspace?.id;
                  const connecting = busy === `connect:${item.id}`;
                  return (
                    <div
                      key={item.id}
                      className="flex items-center gap-3 rounded-lg border border-[#dfe3ec] bg-[#fbfbfd] px-3 py-3"
                    >
                      <span
                        className={cn(
                          "flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border text-sm font-bold",
                          selected
                            ? "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]"
                            : "border-[#dfe3ec] bg-white text-[#303849]",
                        )}
                      >
                        {workspaceInitials(item)}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-center gap-2">
                          <div className="truncate text-sm font-bold text-[#111827]">
                            {item.name}
                          </div>
                          <Badge
                            variant={
                              selected && connection === "open" ? "success" : "outline"
                            }
                          >
                            {selected ? connectionLabel(connection) : "Saved"}
                          </Badge>
                        </div>
                        <div className="mt-1 truncate font-mono text-xs text-[#667085]">
                          {item.serverUrl}
                        </div>
                      </div>
                      <Button
                        variant={selected && connection === "open" ? "outline" : "default"}
                        size="sm"
                        onClick={() => onSelectWorkspace(item.id)}
                        disabled={connecting}
                      >
                        {connecting ? (
                          <Loader2 className="animate-spin" size={14} />
                        ) : selected && connection === "open" ? (
                          <Check size={14} />
                        ) : null}
                        {selected && connection === "open" ? "Open" : "Connect"}
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        title="Remove space"
                        onClick={() => onRemoveWorkspace(item.id)}
                        disabled={busy === `workspace:remove:${item.id}`}
                      >
                        <Trash2 size={16} />
                      </Button>
                    </div>
                  );
                })
              )}
            </div>
          </SettingsSection>
        </div>
      </div>
    </section>
  );
}

function AccountView({
  account,
  busy,
  onLogin,
  onLogout,
}: {
  account: HumanAccount | null;
  busy: string | null;
  onLogin: (provider: ipc.LoginProvider) => void;
  onLogout: () => void;
}) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Account" detail="Identity and sign-in" />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        <div className="mx-auto max-w-2xl">
          <SettingsSection title="Account" detail="Used for presence and local ownership.">
            {account ? (
              <div className="space-y-5">
                <div className="flex items-center gap-3">
                  <Avatar account={account} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-base font-bold text-[#111827]">
                      {accountName(account)}
                    </div>
                    <div className="truncate text-sm text-[#667085]">
                      {account.email || account.staffId || capitalize(account.provider)}
                    </div>
                  </div>
                  <Button
                    variant="outline"
                    onClick={onLogout}
                    disabled={busy === "logout"}
                  >
                    <LogOut size={15} />
                    Sign Out
                  </Button>
                </div>
                <div className="grid gap-3 sm:grid-cols-2">
                  <AccountField label="Provider" value={capitalize(account.provider)} />
                  <AccountField label="Actor ID" value={account.actorId} mono />
                  <AccountField label="Staff ID" value={account.staffId || "-"} />
                  <AccountField label="Email" value={account.email || "-"} />
                </div>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={() => onLogin("github")} disabled={busy === "login:github"}>
                  <Github size={15} />
                  GitHub
                </Button>
                <Button
                  variant="outline"
                  onClick={() => onLogin("google")}
                  disabled={busy === "login:google"}
                >
                  Google
                </Button>
              </div>
            )}
          </SettingsSection>
        </div>
      </div>
    </section>
  );
}

function AccountField({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0 rounded-lg border border-[#edf0f5] bg-[#fbfbfd] px-3 py-2">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        {label}
      </div>
      <div
        className={cn(
          "mt-1 truncate text-sm font-semibold text-[#303849]",
          mono && "font-mono",
        )}
      >
        {value}
      </div>
    </div>
  );
}

function SettingsView({
  busy,
  machineForm,
  setMachineForm,
  agentForm,
  setAgentForm,
  machines,
  targetAgentId,
  onCheckMachines,
  onAddMachine,
  onRemoveMachine,
  onAddAgent,
  onUpdateAgent,
  onRemoveAgent,
  onOpenLocalPath,
}: {
  busy: string | null;
  machineForm: { name: string; dataRoot: string };
  setMachineForm: (form: { name: string; dataRoot: string }) => void;
  agentForm: AgentFormState;
  setAgentForm: (form: AgentFormState) => void;
  machines: MachineInfo[];
  targetAgentId: string | null;
  onCheckMachines: () => void;
  onAddMachine: () => void;
  onRemoveMachine: (machineId: string) => void;
  onAddAgent: () => void;
  onUpdateAgent: (patch: AgentUpdatePatch) => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
  onOpenLocalPath: (path: string) => void;
}) {
  const [selectedMachineId, setSelectedMachineId] = useState<string | null>(null);
  const [hostComposerOpen, setHostComposerOpen] = useState(false);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(targetAgentId);
  const memberEntries = agentMemberEntries(machines);
  const selectedMemberEntry =
    selectedAgentId === null
      ? null
      : memberEntries.find((entry) => entry.agent.spec.actor.id === selectedAgentId) ?? null;
  const selectedMachine =
    selectedMemberEntry?.machine ??
    machines.find((machine) => machine.id === selectedMachineId) ??
    machines.find((machine) => machine.id === agentForm.machineId) ??
    machines[0];

  useEffect(() => {
    if (machines.length === 0) {
      if (selectedMachineId) setSelectedMachineId(null);
      return;
    }
    if (selectedMachineId && machines.some((machine) => machine.id === selectedMachineId)) {
      return;
    }
    const nextMachine =
      machines.find((machine) => machine.id === agentForm.machineId) ?? machines[0];
    setSelectedMachineId(nextMachine.id);
    setAgentForm(agentFormForMachine(agentForm, nextMachine));
  }, [agentForm, machines, selectedMachineId, setAgentForm]);

  useEffect(() => {
    if (
      selectedAgentId &&
      !memberEntries.some((entry) => entry.agent.spec.actor.id === selectedAgentId)
    ) {
      setSelectedAgentId(null);
    }
  }, [memberEntries, selectedAgentId]);

  useEffect(() => {
    if (!targetAgentId) return;
    const entry = findAgentMemberEntry(machines, targetAgentId);
    setSelectedAgentId(targetAgentId);
    setHostComposerOpen(false);
    if (entry) setSelectedMachineId(entry.machine.id);
  }, [machines, targetAgentId]);

  function selectMachine(machine: MachineInfo) {
    setSelectedMachineId(machine.id);
    setSelectedAgentId(null);
    setAgentForm(agentFormForMachine(agentForm, machine));
  }

  function selectAgent(entry: AgentMemberEntry) {
    setSelectedMachineId(entry.machine.id);
    setSelectedAgentId(entry.agent.spec.actor.id);
    setAgentForm(agentFormForMachine(agentForm, entry.machine));
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Hosts" detail="Workspace-bound runtimes and agents" />
      <div className="min-h-0 flex-1 overflow-hidden bg-white">
        <div className="grid h-full min-h-0 grid-cols-1 lg:grid-cols-[minmax(208px,224px)_minmax(0,1fr)]">
          <aside className="flex min-h-0 flex-col border-r border-[#e2e6ef] bg-[#fbfbfd]">
            <div className="border-b border-[#edf0f5] p-4">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                    Hosts
                  </div>
                  <div className="mt-1 text-sm font-bold text-[#111827]">
                    {machines.length} configured
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <Button
                    variant="outline"
                    size="icon"
                    title="Refresh hosts"
                    onClick={onCheckMachines}
                    disabled={busy === "machine:check"}
                    className="h-9 w-9 rounded-lg border-[#dfe3ec] bg-white"
                  >
                    {busy === "machine:check" ? (
                      <Loader2 className="animate-spin" size={15} />
                    ) : (
                      <RefreshCw size={15} />
                    )}
                  </Button>
                  <Button
                    variant={hostComposerOpen ? "secondary" : "outline"}
                    size="icon"
                    title={hostComposerOpen ? "Close add host" : "Add host"}
                    onClick={() => setHostComposerOpen((open) => !open)}
                    className="h-9 w-9 rounded-lg border-[#dfe3ec] bg-white"
                  >
                    {hostComposerOpen ? <X size={15} /> : <Plus size={15} />}
                  </Button>
                </div>
              </div>
            </div>
            {hostComposerOpen && (
              <form
                className="border-b border-[#edf0f5] bg-white p-4"
                onSubmit={(event) => {
                  event.preventDefault();
                  onAddMachine();
                }}
              >
                <div className="mb-3 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-[#596174]">
                  <Plus size={13} />
                  Add Host
                </div>
                <div className="space-y-2">
                  <Input
                    value={machineForm.name}
                    onChange={(event) =>
                      setMachineForm({ ...machineForm, name: event.target.value })
                    }
                    placeholder="Host name"
                    className="h-9 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                  />
                  <Input
                    value={machineForm.dataRoot}
                    onChange={(event) =>
                      setMachineForm({ ...machineForm, dataRoot: event.target.value })
                    }
                    placeholder="Data root"
                    className="h-9 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                  />
                  <Button
                    type="submit"
                    className="w-full rounded-lg"
                    disabled={busy === "machine:create" || !machineForm.name.trim()}
                  >
                    <Plus size={15} />
                    Add Host
                  </Button>
                </div>
              </form>
            )}
            <div className="min-h-0 flex-1 overflow-y-auto p-3 soft-scrollbar">
              <div className="space-y-2">
                {machines.length === 0 ? (
                  <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-white p-4 text-sm text-[#667085]">
                    No hosts configured.
                  </div>
                ) : (
                  machines.map((machine) => (
                    <HostListItem
                      key={machine.id}
                      machine={machine}
                      selected={selectedMachine?.id === machine.id}
                      onSelect={() => selectMachine(machine)}
                    />
                  ))
                )}
              </div>
              <div className="mt-5 border-t border-[#edf0f5] pt-4">
                <div className="mb-2 flex items-center justify-between px-1">
                  <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                    Members
                  </div>
                  <span className="count-badge">{memberEntries.length}</span>
                </div>
                <div className="space-y-1.5">
                  {memberEntries.length === 0 ? (
                    <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-white p-3 text-xs text-[#667085]">
                      No agents configured.
                    </div>
                  ) : (
                    memberEntries.map((entry) => (
                      <MemberListItem
                        key={`${entry.machine.id}:${entry.agent.spec.actor.id}`}
                        entry={entry}
                        selected={selectedMemberEntry?.agent.spec.actor.id === entry.agent.spec.actor.id}
                        onSelect={() => selectAgent(entry)}
                      />
                    ))
                  )}
                </div>
              </div>
            </div>
          </aside>

          <div className="min-h-0 overflow-y-auto bg-white soft-scrollbar">
            {selectedMemberEntry ? (
              <AgentMemberDetail
                entry={selectedMemberEntry}
                busy={busy}
                onUpdateAgent={onUpdateAgent}
                onRemoveAgent={onRemoveAgent}
              />
            ) : selectedMachine ? (
              <MachineCard
                machine={selectedMachine}
                busy={busy}
                agentForm={agentForm}
                setAgentForm={setAgentForm}
                onAddAgent={onAddAgent}
                onRemove={onRemoveMachine}
                onOpenLocalPath={onOpenLocalPath}
                onRemoveAgent={onRemoveAgent}
              />
            ) : (
              <EmptyState icon={Server} text="No hosts configured." />
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function SettingsSection({
  title,
  detail,
  action,
  children,
}: {
  title: string;
  detail: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="rounded-xl border border-[#dfe3ec] bg-white p-4 shadow-sm">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-bold text-[#111827]">{title}</div>
          <div className="mt-1 text-sm text-[#667085]">{detail}</div>
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function HostListItem({
  machine,
  selected,
  onSelect,
}: {
  machine: MachineInfo;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "flex w-full items-center gap-2.5 rounded-xl border px-3 py-3 text-left transition-colors",
        selected
          ? "border-[#bdb7ff] bg-[#f6f4ff] shadow-sm"
          : "border-transparent bg-transparent hover:border-[#dfe3ec] hover:bg-white",
      )}
      onClick={onSelect}
    >
      <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-[#6f83f7] to-[#4e3ad5] text-white">
        <Server size={17} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-bold text-[#111827]">{machine.name}</span>
          <span className={cn("h-2 w-2 shrink-0 rounded-full", statusDotClass(machine.connectionStatus))} />
        </span>
        <span className="mt-1 block truncate text-xs text-[#667085]">
          {machine.providers.length} runtimes · {machine.onlineAgentCount}/{machine.agentCount} agents online
        </span>
      </span>
    </button>
  );
}

function MemberListItem({
  entry,
  selected,
  onSelect,
}: {
  entry: AgentMemberEntry;
  selected: boolean;
  onSelect: () => void;
}) {
  const actor = entry.agent.spec.actor;

  return (
    <button
      type="button"
      className={cn(
        "flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-colors",
        selected
          ? "border-[#bdb7ff] bg-[#f6f4ff] shadow-sm"
          : "border-transparent bg-transparent hover:border-[#dfe3ec] hover:bg-white",
      )}
      onClick={onSelect}
    >
      <ActorAvatar actor={actor} fallback={actor.id} small />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-bold text-[#111827]">
            {agentDisplayName(entry.agent)}
          </span>
          <span className={cn("h-2 w-2 shrink-0 rounded-full", statusDotClass(entry.agent.status))} />
        </span>
        <span className="mt-0.5 block truncate text-xs text-[#667085]">
          {entry.machine.name}
        </span>
      </span>
    </button>
  );
}

function MachineCard({
  machine,
  busy,
  agentForm,
  setAgentForm,
  onAddAgent,
  onRemove,
  onOpenLocalPath,
  onRemoveAgent,
}: {
  machine: MachineInfo;
  busy: string | null;
  agentForm: AgentFormState;
  setAgentForm: (form: AgentFormState) => void;
  onAddAgent: () => void;
  onRemove: (machineId: string) => void;
  onOpenLocalPath: (path: string) => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  const selectedProvider = resolveAgentProvider(agentForm, machine);
  const modelChoices = selectedProvider?.modelChoices ?? [];
  const canCreateAgent = machineCanCreateAgent(machine);
  const agentReady = Boolean(
    canCreateAgent && selectedProvider && agentForm.name.trim(),
  );
  const createStatusText = !canCreateAgent
    ? "This host is read-only for the current account."
    : !selectedProvider
      ? "No runtime detected for this host."
      : `${selectedProvider.name} on ${machine.name}`;
  const [agentComposerOpen, setAgentComposerOpen] = useState(false);

  useEffect(() => {
    setAgentComposerOpen(false);
  }, [machine.id]);

  function updateAgentForm(patch: Partial<AgentFormState>) {
    setAgentForm({
      ...agentForm,
      machineId: machine.id,
      providerId: selectedProvider?.id ?? agentForm.providerId,
      ...patch,
    });
  }

  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="flex min-w-0 items-start gap-4">
            <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-[#6f83f7] to-[#4e3ad5] text-white shadow-sm">
              <HardDrive size={25} />
            </div>
            <div className="min-w-0">
              <h2 className="truncate text-xl font-bold text-[#111827]">
                {machine.name}
              </h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
                <span
                  className={cn(
                    "h-2 w-2 rounded-full",
                    statusDotClass(machine.connectionStatus),
                  )}
                />
                <span>{capitalize(machine.connectionStatus)}</span>
                <span className="text-[#a0a6b3]">/</span>
                <span className="font-mono text-xs">{machine.id}</span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge variant="secondary">{machine.setupStatus}</Badge>
                <Badge variant="outline">{machine.kind}</Badge>
                {machine.readOnly && <Badge variant="warning">read only</Badge>}
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-start gap-6">
            <HostMetric label="Runtimes" value={machine.providers.length} />
            <HostMetric label="Agents" value={machine.agentCount} />
            <HostMetric label="Online" value={machine.onlineAgentCount} />
            {machine.canOpenLocalPath && (
              <Button
                variant="outline"
                size="sm"
                title="Open data root"
                onClick={() => onOpenLocalPath(machine.dataRoot)}
                className="rounded-lg border-[#dfe3ec] bg-white"
              >
                <HardDrive size={15} />
                Open Data
              </Button>
            )}
          </div>
        </div>
      </section>

      <HostDetailSection title="Name">
        <div className="text-sm font-semibold text-[#111827]">{machine.name}</div>
      </HostDetailSection>

      <HostDetailSection title="Info">
        <div className="divide-y divide-[#edf0f5]">
          <HostInfoRow label="Source">
            {machine.source || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Data Root" mono>
            {machine.dataRoot || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Config Dir" mono>
            {machine.configDir || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Connection Actor" mono>
            {machine.connectionActorId || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Serve Command" mono>
            {machine.serveCommand || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Detected Runtimes">
            <div className="flex flex-wrap gap-2">
              {machine.providers.length === 0 ? (
                <Badge variant="warning">no runtimes detected</Badge>
              ) : (
                machine.providers.map((provider) => (
                  <ProviderBadge key={provider.id} provider={provider} />
                ))
              )}
            </div>
          </HostInfoRow>
          <HostInfoRow label="Inventory">
            Revision {machine.inventoryRevision}
            {machine.inventoryObservedAt
              ? ` · observed ${formatTime(machine.inventoryObservedAt)}`
              : ""}
          </HostInfoRow>
        </div>
      </HostDetailSection>

      <HostDetailSection
        title="Agents on this Host"
        count={machine.agents.length}
        action={
          <Button
            onClick={() => setAgentComposerOpen((open) => !open)}
            disabled={!agentComposerOpen && !canCreateAgent}
            className="rounded-lg"
          >
            {agentComposerOpen ? <X size={15} /> : <Plus size={15} />}
            {agentComposerOpen ? "Close" : "Create Agent"}
          </Button>
        }
      >
        <div className="space-y-2">
          {machine.agents.length === 0 ? (
            <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
              No agents on this host.
            </div>
          ) : (
            machine.agents.map((agent) => (
              <HostAgentRow
                key={agent.spec.actor.id}
                agent={agent}
                machine={machine}
                busy={busy}
                onRemoveAgent={onRemoveAgent}
              />
            ))
          )}
        </div>

        {agentComposerOpen && (
          <form
            className="mt-5 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] p-4 shadow-sm"
            onSubmit={(event) => {
              event.preventDefault();
              onAddAgent();
            }}
          >
            <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.16em] text-[#596174]">
                <Bot size={14} />
                New Agent
              </div>
              <Badge variant={canCreateAgent ? "outline" : "warning"}>
                {canCreateAgent ? "available" : "read only"}
              </Badge>
            </div>

            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
              <select
                value={selectedProvider?.id ?? ""}
                onChange={(event) => {
                  const provider = machine.providers.find(
                    (item) => item.id === event.target.value,
                  );
                  setAgentForm({
                    ...agentForm,
                    machineId: machine.id,
                    providerId: event.target.value,
                    model: provider?.defaultModel ?? "",
                  });
                }}
                className="h-10 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm"
                disabled={machine.providers.length === 0 || !canCreateAgent}
              >
                {machine.providers.length === 0 ? (
                  <option value="">No runtimes</option>
                ) : (
                  machine.providers.map((provider) => (
                    <option key={provider.id} value={provider.id}>
                      {provider.name}
                    </option>
                  ))
                )}
              </select>
              <Input
                value={agentForm.name}
                onChange={(event) => updateAgentForm({ name: event.target.value })}
                placeholder="Agent name"
                className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                disabled={!canCreateAgent}
              />
              <Input
                value={agentForm.actorId}
                onChange={(event) => updateAgentForm({ actorId: event.target.value })}
                placeholder="Actor id (optional)"
                className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                disabled={!canCreateAgent}
              />
              {modelChoices.length > 0 ? (
                <select
                  value={agentForm.model || selectedProvider?.defaultModel || ""}
                  onChange={(event) => updateAgentForm({ model: event.target.value })}
                  className="h-10 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm"
                  disabled={!canCreateAgent}
                >
                  {modelChoices.map((choice) => (
                    <option key={choice.id} value={choice.id}>
                      {choice.label}
                    </option>
                  ))}
                </select>
              ) : (
                <Input
                  value={agentForm.model}
                  onChange={(event) => updateAgentForm({ model: event.target.value })}
                  placeholder="Model"
                  className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                  disabled={!canCreateAgent}
                />
              )}
              <label className="flex h-10 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm text-[#303849]">
                <input
                  type="checkbox"
                  checked={agentForm.autostart}
                  onChange={(event) =>
                    updateAgentForm({ autostart: event.target.checked })
                  }
                  disabled={!canCreateAgent}
                />
                Autostart
              </label>
              <Textarea
                value={agentForm.description}
                onChange={(event) => updateAgentForm({ description: event.target.value })}
                placeholder="Agent instructions"
                className="min-h-28 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2 xl:col-span-4"
                disabled={!canCreateAgent}
              />
            </div>

            <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
              <div className="text-xs font-medium text-[#667085]">
                {createStatusText}
              </div>
              <Button
                type="submit"
                disabled={busy === "agent:create" || !agentReady}
                className="rounded-lg"
              >
                {busy === "agent:create" ? (
                  <Loader2 className="animate-spin" size={15} />
                ) : (
                  <Plus size={15} />
                )}
                Create Agent
              </Button>
            </div>
          </form>
        )}
      </HostDetailSection>

      <HostDetailSection title="Actions">
        <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
          <div className="min-w-0">
            <div className="text-sm font-bold text-[#111827]">Delete Host</div>
            <div className="mt-1 text-sm text-[#667085]">
              Permanently remove this host after its agents are deleted.
            </div>
          </div>
          {!machine.readOnly ? (
            <Button
              variant="destructive"
              size="sm"
              title="Remove host"
              onClick={() => onRemove(machine.id)}
              disabled={busy === `machine:remove:${machine.id}`}
              className="rounded-lg"
            >
              <Trash2 size={15} />
              Delete Host
            </Button>
          ) : (
            <Badge variant="warning">read only</Badge>
          )}
        </div>
      </HostDetailSection>
    </div>
  );
}

function AgentMemberDetail({
  entry,
  busy,
  onUpdateAgent,
  onRemoveAgent,
}: {
  entry: AgentMemberEntry;
  busy: string | null;
  onUpdateAgent: (patch: AgentUpdatePatch) => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  const { machine, agent } = entry;
  const actor = agent.spec.actor;
  const [draft, setDraft] = useState<AgentSettingsDraft>(() =>
    agentSettingsDraft(machine, agent),
  );
  const selectedProvider = providerForAgent(machine, agent, draft.providerId);
  const modelChoices =
    selectedProvider?.modelChoices.length
      ? selectedProvider.modelChoices
      : agent.spec.models?.choices ?? [];
  const saving = busy === `agent:update:${actor.id}`;
  const removing = busy === `agent:remove:${actor.id}`;
  const canEdit = !machine.readOnly;

  useEffect(() => {
    setDraft(agentSettingsDraft(machine, agent));
  }, [machine.id, agent]);

  function updateDraft(patch: Partial<AgentSettingsDraft>) {
    setDraft((current) => ({ ...current, ...patch }));
  }

  function saveAgent() {
    onUpdateAgent({
      machineId: machine.id,
      actorId: actor.id,
      displayName: draft.displayName,
      description: draft.description,
      providerId: draft.providerId || undefined,
      model: draft.model,
      reasoningEffort: draft.reasoningEffort,
      autostart: draft.autostart,
      avatarUrl: draft.avatarUrl,
    });
  }

  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="flex min-w-0 items-start gap-4">
            <img
              alt=""
              src={draft.avatarUrl || actorAvatarUrl(actor, actor.id)}
              className="h-16 w-16 shrink-0 rounded-xl border border-white object-cover shadow-sm"
            />
            <div className="min-w-0">
              <h2 className="truncate text-xl font-bold text-[#111827]">
                {draft.displayName || agentDisplayName(agent)}
              </h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
                <span className={cn("h-2 w-2 rounded-full", statusDotClass(agent.status))} />
                <span>{capitalize(agent.status)}</span>
                <span className="text-[#a0a6b3]">/</span>
                <span className="font-mono text-xs">{actor.id}</span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge variant={agent.status === "online" ? "success" : "outline"}>
                  {agent.status}
                </Badge>
                <Badge variant="secondary">{machine.name}</Badge>
                {machine.readOnly && <Badge variant="warning">read only</Badge>}
              </div>
            </div>
          </div>
          <Button
            onClick={saveAgent}
            disabled={!canEdit || saving || !draft.displayName.trim()}
            className="rounded-lg"
          >
            {saving ? <Loader2 className="animate-spin" size={15} /> : <Check size={15} />}
            Save Changes
          </Button>
        </div>
      </section>

      <HostDetailSection title="Profile">
        <div className="grid gap-5 xl:grid-cols-[minmax(260px,0.42fr)_minmax(0,1fr)]">
          <div className="min-w-0">
            <div className="mb-3 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
              Avatar Library
            </div>
            <div className="grid max-h-64 grid-cols-[repeat(auto-fill,minmax(38px,1fr))] gap-2 overflow-y-auto rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-3 soft-scrollbar">
              {avatarLibraryUrls.map((url) => (
                <button
                  key={url}
                  type="button"
                  title={url.split("/").pop() ?? "Avatar"}
                  disabled={!canEdit}
                  className={cn(
                    "flex aspect-square items-center justify-center rounded-lg border bg-white p-1 transition-colors",
                    draft.avatarUrl === url
                      ? "border-[#8f82ff] ring-2 ring-[#e4e0ff]"
                      : "border-[#edf0f5] hover:border-[#c8c1ff]",
                  )}
                  onClick={() => updateDraft({ avatarUrl: url })}
                >
                  <img alt="" src={url} className="h-full w-full rounded-md object-cover" />
                </button>
              ))}
            </div>
          </div>

          <div className="grid content-start gap-3 md:grid-cols-2">
            <Input
              value={draft.displayName}
              onChange={(event) => updateDraft({ displayName: event.target.value })}
              placeholder="Display name"
              className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
              disabled={!canEdit}
            />
            <Input
              value={actor.id}
              readOnly
              className="h-10 rounded-lg border-[#dfe3ec] bg-[#fbfbfd] font-mono text-xs shadow-none"
            />
            <Textarea
              value={draft.description}
              onChange={(event) => updateDraft({ description: event.target.value })}
              placeholder="Agent instructions"
              className="min-h-32 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
              disabled={!canEdit}
            />
          </div>
        </div>
      </HostDetailSection>

      <HostDetailSection title="Runtime Configuration">
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <select
            value={selectedProvider?.id ?? draft.providerId}
            onChange={(event) => {
              const provider = machine.providers.find(
                (item) => item.id === event.target.value,
              );
              updateDraft({
                providerId: event.target.value,
                model: provider?.defaultModel ?? draft.model,
              });
            }}
            className="h-10 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm"
            disabled={!canEdit || machine.providers.length === 0}
          >
            {machine.providers.length === 0 ? (
              <option value="">No runtimes</option>
            ) : (
              machine.providers.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.name}
                </option>
              ))
            )}
          </select>
          {modelChoices.length > 0 ? (
            <select
              value={draft.model}
              onChange={(event) => updateDraft({ model: event.target.value })}
              className="h-10 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm"
              disabled={!canEdit}
            >
              {modelChoices.map((choice) => (
                <option key={choice.id} value={choice.id}>
                  {choice.label || choice.id}
                </option>
              ))}
            </select>
          ) : (
            <Input
              value={draft.model}
              onChange={(event) => updateDraft({ model: event.target.value })}
              placeholder="Model"
              className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
              disabled={!canEdit}
            />
          )}
          <select
            value={draft.reasoningEffort}
            onChange={(event) => updateDraft({ reasoningEffort: event.target.value })}
            className="h-10 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm"
            disabled={!canEdit}
          >
            {reasoningEffortChoices.map((choice) => (
              <option key={choice || "default"} value={choice}>
                {choice ? capitalize(choice) : "Default reasoning"}
              </option>
            ))}
          </select>
          <label className="flex h-10 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm text-[#303849]">
            <input
              type="checkbox"
              checked={draft.autostart}
              onChange={(event) => updateDraft({ autostart: event.target.checked })}
              disabled={!canEdit}
            />
            Autostart
          </label>
        </div>
      </HostDetailSection>

      <HostDetailSection title="Info">
        <div className="divide-y divide-[#edf0f5]">
          <HostInfoRow label="Host">{machine.name}</HostInfoRow>
          <HostInfoRow label="Actor ID" mono>{actor.id}</HostInfoRow>
          <HostInfoRow label="Profile Path" mono>{agent.profilePath || "Not set"}</HostInfoRow>
          <HostInfoRow label="Identity Path" mono>{agent.identityPath || "Not set"}</HostInfoRow>
          <HostInfoRow label="Soul Path" mono>{agent.soulPath || "Not set"}</HostInfoRow>
        </div>
      </HostDetailSection>

      <HostDetailSection title="Actions">
        <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
          <div className="min-w-0">
            <div className="text-sm font-bold text-[#111827]">Remove Agent</div>
            <div className="mt-1 text-sm text-[#667085]">
              Remove this member from {machine.name}.
            </div>
          </div>
          {!machine.readOnly ? (
            <Button
              variant="destructive"
              size="sm"
              title="Remove agent"
              onClick={() => onRemoveAgent(machine.id, actor.id)}
              disabled={removing}
              className="rounded-lg"
            >
              {removing ? <Loader2 className="animate-spin" size={15} /> : <Trash2 size={15} />}
              Remove Agent
            </Button>
          ) : (
            <Badge variant="warning">read only</Badge>
          )}
        </div>
      </HostDetailSection>
    </div>
  );
}

function HostMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="min-w-20 border-l border-[#dfe3ec] pl-4">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        {label}
      </div>
      <div className="mt-1 text-xl font-bold text-[#111827]">{value}</div>
    </div>
  );
}

function HostDetailSection({
  title,
  count,
  action,
  children,
}: {
  title: string;
  count?: number;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="border-b border-[#dfe3ec] px-6 py-5 lg:px-8">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
            {title}
          </div>
          {typeof count === "number" && (
            <span className="font-mono text-xs font-semibold text-[#9aa1ae]">
              {count}
            </span>
          )}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function ProviderBadge({ provider }: { provider: MachineAgentProviderInfo }) {
  const iconKey = agentProviderIconKey(provider.id, provider.name);
  return (
    <Badge variant="outline" className="gap-1.5">
      {iconKey && <AgentProviderIcon iconKey={iconKey} className="h-3.5 w-3.5" />}
      {provider.name}
      {provider.actorCount > 0 ? ` (${provider.actorCount})` : ""}
    </Badge>
  );
}

function HostInfoRow({
  label,
  children,
  mono,
}: {
  label: string;
  children: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="grid gap-2 py-3 first:pt-0 last:pb-0 sm:grid-cols-[160px_minmax(0,1fr)]">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        {label}
      </div>
      <div
        className={cn(
          "min-w-0 text-sm text-[#303849]",
          mono && "break-all font-mono text-xs text-[#485063]",
        )}
      >
        {children}
      </div>
    </div>
  );
}

function HostAgentRow({
  agent,
  machine,
  busy,
  onRemoveAgent,
}: {
  agent: MachineInfo["agents"][number];
  machine: MachineInfo;
  busy: string | null;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  const actor = agent.spec.actor;

  return (
    <div className="grid min-h-[62px] grid-cols-[minmax(0,1fr)_auto] items-center gap-4 rounded-xl border border-[#edf0f5] bg-[#fbfbfd] px-4 py-3">
      <div className="flex min-w-0 items-center gap-3">
        <ActorAvatar actor={actor} fallback={actor.id} small />
        <div className="min-w-0">
          <div className="truncate text-sm font-bold text-[#111827]">
            {actor.displayName || actor.id}
          </div>
          <div className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-[#667085]">
            <span className="font-mono">
              {agentModelValue(agent) || shortActorAlias(actor.id)}
            </span>
            <span className="text-[#a0a6b3]">/</span>
            <span>{agent.spec.autostart ? "autostart" : "manual"}</span>
          </div>
        </div>
      </div>
      <div className="flex items-center gap-3">
        <Badge variant={agent.status === "online" ? "success" : "outline"}>
          {agent.status}
        </Badge>
        {!machine.readOnly && (
          <Button
            variant="ghost"
            size="icon"
            title="Remove agent"
            onClick={() => onRemoveAgent(machine.id, actor.id)}
            disabled={busy === `agent:remove:${actor.id}`}
            className="rounded-lg"
          >
            <Trash2 size={15} />
          </Button>
        )}
      </div>
    </div>
  );
}

function ChannelPanel({
  actors,
  memberCandidates,
  channel,
  channelMessages,
  channelTasks,
  channelThreads,
  currentActorId,
  machines,
  threadStatsById,
  tab,
  busy,
  onClose,
  onSelectTab,
  onSelectThread,
  onInviteMember,
  onRemoveMember,
}: {
  actors: Record<string, Actor>;
  memberCandidates: Actor[];
  channel: Channel | null;
  channelMessages: Message[];
  channelTasks: Task[];
  channelThreads: Thread[];
  currentActorId: string | null;
  machines: MachineInfo[];
  threadStatsById: Record<string, ThreadActivityStats>;
  tab: ChannelPanelTab;
  busy: string | null;
  onClose: () => void;
  onSelectTab: (tab: ChannelPanelTab) => void;
  onSelectThread: (thread: Thread) => void;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
}) {
  return (
    <ChannelDetailPanel
      actors={actors}
      memberCandidates={memberCandidates}
      channel={channel}
      channelMessages={channelMessages}
      channelTasks={channelTasks}
      channelThreads={channelThreads}
      currentActorId={currentActorId}
      machines={machines}
      threadStatsById={threadStatsById}
      tab={tab}
      busy={busy}
      className="hidden min-h-0 min-w-0 flex-col bg-[#fbfbfd] xl:flex"
      onClose={onClose}
      onSelectTab={onSelectTab}
      onSelectThread={onSelectThread}
      onInviteMember={onInviteMember}
      onRemoveMember={onRemoveMember}
    />
  );
}

function ChannelDetailPanel({
  actors,
  memberCandidates,
  channel,
  channelMessages,
  channelTasks,
  channelThreads,
  currentActorId,
  machines,
  threadStatsById,
  tab,
  busy,
  className,
  onClose,
  onSelectTab,
  onSelectThread,
  onInviteMember,
  onRemoveMember,
}: {
  actors: Record<string, Actor>;
  memberCandidates: Actor[];
  channel: Channel | null;
  channelMessages: Message[];
  channelTasks: Task[];
  channelThreads: Thread[];
  currentActorId: string | null;
  machines: MachineInfo[];
  threadStatsById: Record<string, ThreadActivityStats>;
  tab: ChannelPanelTab;
  busy: string | null;
  className?: string;
  onClose: () => void;
  onSelectTab: (tab: ChannelPanelTab) => void;
  onSelectThread: (thread: Thread) => void;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
}) {
  const [memberQuery, setMemberQuery] = useState("");
  const members = channel
    ? channel.members.map((actorId) => actors[actorId] ?? fallbackActor(actorId))
    : [];
  const availableMembers = channel
    ? memberCandidates.filter((actor) => canAddChannelMember(channel, actor))
    : [];
  const filteredAvailableMembers = availableMembers.filter((actor) => {
    const query = memberQuery.trim().toLowerCase();
    if (!query) return true;
    return `${displayName(actor)} ${actor.id} ${actor.kind}`.toLowerCase().includes(query);
  });
  const rootMessagesById = new Map(channelMessages.map((message) => [message.id, message]));
  const memberRows = members.map((actor) => ({
    actor,
    presence: memberPresence(actor, machines, currentActorId),
  }));
  const onlineMembers = memberRows.filter((item) => item.presence.online);
  const offlineMembers = memberRows.filter((item) => !item.presence.online);
  const panelTitle = channelPanelTitle(tab);
  const panelDetail = channel
    ? `#${channel.title} · ${channelPanelDetail(tab, channelThreads.length, members.length, channelTasks.length)}`
    : "Not connected";
  const tabs: Array<{ id: ChannelPanelTab; label: string; count: number }> = [
    { id: "threads", label: "Threads", count: channelThreads.length },
    { id: "members", label: "Members", count: members.length },
    { id: "tasks", label: "Tasks", count: channelTasks.length },
  ];
  return (
    <aside className={cn("min-h-0 min-w-0 flex-col bg-[#fbfbfd]", className ?? "flex")}>
      <div className="border-b border-[#edf0f5] bg-white p-5">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-[#6784f4] to-[#4d3ed7] text-white shadow-sm">
              {tab === "threads" ? (
                <Split size={24} />
              ) : tab === "members" ? (
                <Users size={24} />
              ) : (
                <Check size={24} />
              )}
            </div>
            <div className="min-w-0">
              <div className="truncate text-lg font-bold text-[#111827]">
                {panelTitle}
              </div>
              <div className="mt-1 truncate text-sm text-[#485063]">
                {channel
                  ? `#${channel.title} · ${panelDetail}`
                  : "Not connected"}
              </div>
            </div>
          </div>
          <button className="composer-icon" type="button" title="Close panel" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
      </div>

      <div className="grid h-12 shrink-0 grid-cols-3 border-b border-[#edf0f5] bg-white px-5">
        {tabs.map((item) => (
          <button
            key={item.id}
            type="button"
            className={cn(
              "relative text-sm font-semibold capitalize text-[#667085]",
              tab === item.id && "text-[#503ed4]",
            )}
            onClick={() => onSelectTab(item.id)}
          >
            {item.label}
            {item.count > 0 && (
              <span className="ml-1 rounded-full bg-[#f1efff] px-1.5 py-0.5 text-[10px] text-[#5843d7]">
                {item.count}
              </span>
            )}
            {tab === item.id && (
              <span className="absolute inset-x-1 bottom-0 h-0.5 rounded-full bg-[#503ed4]" />
            )}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        {!channel ? (
          <EmptyState icon={Hash} text="Select a channel." />
        ) : tab === "threads" ? (
          <ChannelThreadsPanel
            actors={actors}
            rootMessagesById={rootMessagesById}
            threadStatsById={threadStatsById}
            threads={channelThreads}
            onSelectThread={onSelectThread}
          />
        ) : tab === "members" ? (
          <ChannelMembersPanel
            availableMembers={availableMembers}
            busy={busy}
            channel={channel}
            filteredAvailableMembers={filteredAvailableMembers}
            memberQuery={memberQuery}
            offlineMembers={offlineMembers}
            onlineMembers={onlineMembers}
            setMemberQuery={setMemberQuery}
            onInviteMember={onInviteMember}
            onRemoveMember={onRemoveMember}
          />
        ) : (
          <ChannelTasksPanel actors={actors} tasks={channelTasks} />
        )}
      </div>
    </aside>
  );
}

function ChannelThreadsPanel({
  actors,
  rootMessagesById,
  threadStatsById,
  threads,
  onSelectThread,
}: {
  actors: Record<string, Actor>;
  rootMessagesById: Map<string, Message>;
  threadStatsById: Record<string, ThreadActivityStats>;
  threads: Thread[];
  onSelectThread: (thread: Thread) => void;
}) {
  if (threads.length === 0) {
    return <EmptyState icon={Split} text="No threads in this channel." />;
  }
  return (
    <div className="space-y-3">
      {threads.map((thread) => (
        <ChannelThreadCard
          key={thread.id}
          actors={actors}
          rootMessage={rootMessagesById.get(thread.rootMessageId) ?? null}
          thread={thread}
          threadStats={threadStatsById[thread.id]}
          onSelect={() => onSelectThread(thread)}
        />
      ))}
    </div>
  );
}

function ChannelThreadCard({
  actors,
  rootMessage,
  thread,
  threadStats,
  onSelect,
}: {
  actors: Record<string, Actor>;
  rootMessage: Message | null;
  thread: Thread;
  threadStats?: ThreadActivityStats;
  onSelect: () => void;
}) {
  const starter = rootMessage ? actors[rootMessage.authorActorId] : undefined;
  const participants = threadParticipants(thread, actors, starter, threadStats);
  const replyCount = threadReplyCount(thread, threadStats);
  const lastReply = threadLastReplyLabel(thread, threadStats);
  const preview = rootMessage
    ? rootMessage.body || metadataText(rootMessage)
    : `Started from ${shortId(thread.rootMessageId)}`;
  const replyLabel =
    typeof replyCount === "number"
      ? `${replyCount}${threadStats?.hasMoreReplies ? "+" : ""} replies`
      : "Thread";
  return (
    <button
      type="button"
      className="w-full rounded-xl border border-[#e2e6ef] bg-white p-4 text-left shadow-[0_1px_2px_rgb(16_24_40_/_0.03)] transition hover:border-[#cdd3e5] hover:shadow-[0_10px_24px_rgb(16_24_40_/_0.07)]"
      onClick={onSelect}
    >
      <div className="flex items-start gap-3">
        <span className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-[#f1efff] text-[#503ed4]">
          <Split size={17} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-start justify-between gap-2">
            <span className="min-w-0">
              <span className="block truncate text-sm font-bold text-[#111827]">
                {thread.title}
              </span>
              <span className="mt-1 line-clamp-2 text-xs leading-5 text-[#596174]">
                {starter ? `${displayName(starter)}: ` : ""}
                {preview}
              </span>
            </span>
            <span className="shrink-0 text-xs font-medium text-[#8a93a5]">
              {rootMessage ? formatTime(rootMessage.createdAt) : shortId(thread.id, 5)}
            </span>
          </span>
          <span className="mt-3 flex min-w-0 items-center gap-2">
            <AvatarStack actors={participants} max={4} small />
            <span className="min-w-0 truncate text-xs font-bold text-[#503ed4]">
              {replyLabel}
            </span>
            {lastReply && (
              <span className="shrink-0 text-xs font-medium text-[#667085]">
                Last {lastReply}
              </span>
            )}
          </span>
        </span>
      </div>
    </button>
  );
}

function ChannelMembersPanel({
  availableMembers,
  busy,
  channel,
  filteredAvailableMembers,
  memberQuery,
  offlineMembers,
  onlineMembers,
  setMemberQuery,
  onInviteMember,
  onRemoveMember,
}: {
  availableMembers: Actor[];
  busy: string | null;
  channel: Channel;
  filteredAvailableMembers: Actor[];
  memberQuery: string;
  offlineMembers: ChannelMemberPanelItem[];
  onlineMembers: ChannelMemberPanelItem[];
  setMemberQuery: (value: string) => void;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
}) {
  return (
    <div className="space-y-4">
      <ChannelMemberGroup
        busy={busy}
        channel={channel}
        items={onlineMembers}
        title="在线"
        onRemoveMember={onRemoveMember}
      />
      <ChannelMemberGroup
        busy={busy}
        channel={channel}
        items={offlineMembers}
        title="离线"
        onRemoveMember={onRemoveMember}
      />
      {onlineMembers.length === 0 && offlineMembers.length === 0 && (
        <MutedLine>No explicit members.</MutedLine>
      )}
      <div className="member-picker-card">
        <div className="mb-3 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-[#667085]">
          <UserPlus size={13} />
          Add member
        </div>
        <label className="mb-3 flex h-9 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-[#667085]">
          <Search size={14} />
          <input
            value={memberQuery}
            onChange={(event) => setMemberQuery(event.target.value)}
            placeholder="Search people and agents"
            className="min-w-0 flex-1 bg-transparent text-sm text-[#303849] outline-none placeholder:text-[#98a2b3]"
          />
        </label>
        <div className="space-y-2">
          {filteredAvailableMembers.length === 0 ? (
            <MutedLine>
              {availableMembers.length === 0
                ? "No candidates available."
                : "No matching candidates."}
            </MutedLine>
          ) : (
            filteredAvailableMembers.slice(0, 8).map((actor) => {
              const inviteBusy = busy === `channel:invite:${channel.id}:${actor.id}`;
              return (
                <div key={actor.id} className="member-candidate-row">
                  <ActorAvatar actor={actor} fallback={actor.id} small />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-semibold text-[#303849]">
                      {displayName(actor)}
                    </div>
                    <div className="truncate text-xs text-[#667085]">
                      {actorKindLabel(actor)} · {shortActorAlias(actor.id)}
                    </div>
                  </div>
                  <Button
                    size="sm"
                    className="h-8 rounded-lg bg-[#503ed4] px-3 text-white hover:bg-[#4635c5]"
                    disabled={inviteBusy}
                    onClick={() => onInviteMember(channel.id, actor.id)}
                  >
                    {inviteBusy ? (
                      <Loader2 className="animate-spin" size={13} />
                    ) : (
                      <Plus size={13} />
                    )}
                    Add
                  </Button>
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

function ChannelMemberGroup({
  busy,
  channel,
  items,
  title,
  onRemoveMember,
}: {
  busy: string | null;
  channel: Channel;
  items: ChannelMemberPanelItem[];
  title: string;
  onRemoveMember: (channelId: string, actorId: string) => void;
}) {
  if (items.length === 0) return null;
  return (
    <section className="space-y-2">
      <div className="flex items-center gap-2 px-1 text-xs font-bold text-[#596174]">
        <span>{title}</span>
        <span className="rounded-full bg-[#eef0f6] px-1.5 py-0.5 text-[10px] text-[#667085]">
          {items.length}
        </span>
      </div>
      <div className="space-y-2">
        {items.map(({ actor, presence }) => (
          <ChannelMemberRow
            key={actor.id}
            actor={actor}
            busy={busy}
            channel={channel}
            presence={presence}
            onRemoveMember={onRemoveMember}
          />
        ))}
      </div>
    </section>
  );
}

function ChannelMemberRow({
  actor,
  busy,
  channel,
  presence,
  onRemoveMember,
}: {
  actor: Actor;
  busy: string | null;
  channel: Channel;
  presence: ChannelMemberPresence;
  onRemoveMember: (channelId: string, actorId: string) => void;
}) {
  const revokeBusy = busy === `channel:revoke:${channel.id}:${actor.id}`;
  return (
    <div className="flex items-center gap-3 rounded-xl border border-[#edf0f5] bg-white px-3 py-2.5 shadow-[0_1px_2px_rgb(16_24_40_/_0.03)]">
      <span className="relative shrink-0">
        <ActorAvatar actor={actor} fallback={actor.id} small />
        <span
          className={cn(
            "absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full border-2 border-white",
            statusDotClass(presence.status),
          )}
        />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <div className="truncate text-sm font-semibold text-[#303849]">
            {displayName(actor)}
          </div>
          <span className="shrink-0 rounded-md bg-[#f5f3ff] px-1.5 py-0.5 text-[10px] font-bold text-[#6652e8]">
            {actorKindLabel(actor)}
          </span>
        </div>
        <div className="truncate text-xs text-[#667085]">
          {presence.label}
          {actor.kind === "agent" && presence.status !== "offline"
            ? ` · ${presence.status}`
            : ""}
        </div>
      </div>
      {canRemoveChannelMember(channel, actor.id) && (
        <button
          type="button"
          title={`Remove ${displayName(actor)}`}
          disabled={revokeBusy}
          onClick={() => onRemoveMember(channel.id, actor.id)}
          className="composer-icon h-7 min-w-7 text-[#667085]"
        >
          {revokeBusy ? <Loader2 className="animate-spin" size={13} /> : <X size={13} />}
        </button>
      )}
    </div>
  );
}

function ChannelTasksPanel({
  actors,
  tasks,
}: {
  actors: Record<string, Actor>;
  tasks: Task[];
}) {
  const groups = channelTaskGroups(tasks);
  if (tasks.length === 0) {
    return <EmptyState icon={Check} text="No tasks in this channel." />;
  }
  return (
    <div className="space-y-5">
      {groups.map((group) => (
        <section key={group.id} className="space-y-2">
          <div className="flex items-center gap-2 px-1 text-xs font-bold text-[#596174]">
            <span>{group.title}</span>
            <span className="rounded-full bg-[#eef0f6] px-1.5 py-0.5 text-[10px] text-[#667085]">
              {group.tasks.length}
            </span>
          </div>
          <div className="space-y-2">
            {group.tasks.map((task) => (
              <ChannelTaskCard key={task.id} actors={actors} task={task} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function ChannelTaskCard({
  actors,
  task,
}: {
  actors: Record<string, Actor>;
  task: Task;
}) {
  const ownerId = task.ownerActorId || task.requesterActorId;
  const owner = ownerId ? actors[ownerId] ?? fallbackActor(ownerId) : null;
  const progress = taskProgressPercent(task);
  const done = task.status === "done";
  const active = task.status === "in_progress" || task.status === "waiting_review";
  return (
    <div className="rounded-xl border border-[#e2e6ef] bg-white p-3 shadow-[0_1px_2px_rgb(16_24_40_/_0.03)]">
      <div className="flex items-start gap-3">
        <span
          className={cn(
            "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md border",
            done
              ? "border-[#5b46e8] bg-[#5b46e8] text-white"
              : active
                ? "border-[#5b46e8] bg-[#f4f2ff] text-[#5b46e8]"
                : "border-[#b8bfce] bg-white text-transparent",
          )}
        >
          {done ? <Check size={12} /> : active ? <span className="h-2 w-2 rounded-full bg-current" /> : null}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="line-clamp-2 text-sm font-bold leading-5 text-[#111827]">
                {task.title}
              </div>
              <div className="mt-1 truncate text-xs text-[#667085]">
                Source {shortId(task.sourceMessageId)}
              </div>
            </div>
            <Badge
              variant="outline"
              title={task.id}
              className={cn(
                "shrink-0 whitespace-nowrap font-semibold",
                taskStatusBadgeClass(task.status),
              )}
            >
              Task #{task.number}
            </Badge>
          </div>
          {progress !== null && (
            <div className="mt-3 flex items-center gap-3">
              <span className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-[#e7e9f3]">
                <span
                  className="block h-full rounded-full bg-[#5b46e8]"
                  style={{ width: `${progress}%` }}
                />
              </span>
              <span className="w-9 text-right text-xs font-semibold text-[#667085]">
                {progress}%
              </span>
            </div>
          )}
          <div className="mt-3 flex items-center justify-between gap-2 text-xs text-[#667085]">
            <span className="flex min-w-0 items-center gap-1.5">
              <Clock size={13} />
              <span className="truncate">{formatShortDateTime(task.updatedAt)}</span>
            </span>
            {owner && (
              <span className="flex min-w-0 items-center gap-1.5">
                <ActorAvatar actor={owner} fallback={owner.id} small />
                <span className="max-w-[86px] truncate font-semibold text-[#485063]">
                  {displayName(owner)}
                </span>
              </span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function PageHeader({ title, detail }: { title: string; detail: string }) {
  return (
    <header className="flex h-[86px] shrink-0 items-center justify-between border-b border-[#e2e6ef] bg-white px-6">
      <h1 className="text-[22px] font-bold text-[#111827]">{title}</h1>
      <span className="text-sm font-medium text-[#667085]">{detail}</span>
    </header>
  );
}

function ErrorBanner({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <div className="border-b border-red-200 bg-red-50 px-4 py-2 text-sm font-medium text-red-700">
      {error}
    </div>
  );
}

function EmptyState({
  icon: Icon,
  text,
}: {
  icon: ComponentType<{ size?: string | number; className?: string }>;
  text: string;
}) {
  return (
    <div className="flex min-h-80 flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] text-[#667085]">
      <Icon size={28} />
      <div className="text-sm">{text}</div>
    </div>
  );
}

function MutedLine({ children }: { children: ReactNode }) {
  return <div className="text-sm text-muted-foreground">{children}</div>;
}

function Avatar({ account }: { account: HumanAccount }) {
  const src = account.avatarUrl || avatarUrlForSeed(account.actorId || accountName(account));
  return (
    <img
      alt=""
      src={src}
      className="h-10 w-10 rounded-xl border border-white object-cover shadow-sm"
    />
  );
}

function ActorAvatar({
  actor,
  fallback,
  small,
}: {
  actor?: Actor;
  fallback: string;
  small?: boolean;
}) {
  const src = actorAvatarUrl(actor, fallback);
  return (
    <img
      alt=""
      src={src}
      className={cn(
        "shrink-0 rounded-xl border border-white object-cover shadow-sm",
        small ? "h-7 w-7" : "h-10 w-10",
      )}
      title={actor?.id ?? fallback}
    />
  );
}

function AgentMessageAvatar({
  actor,
  fallback,
  machines,
  onOpenAgentSettings,
  preferredPlacement = "right",
  small,
}: {
  actor?: Actor;
  fallback: string;
  machines: MachineInfo[];
  onOpenAgentSettings: (actorId: string) => void;
  preferredPlacement?: "left" | "right";
  small?: boolean;
}) {
  const actorId = actor?.id ?? fallback;
  const entry = findAgentMemberEntry(machines, actorId);
  const avatarActor = actor ?? entry?.agent.spec.actor;
  const [open, setOpen] = useState(false);
  const [badgeExpanded, setBadgeExpanded] = useState(false);
  const [popoverStyle, setPopoverStyle] = useState<CSSProperties>({});
  const anchorRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const closeTimerRef = useRef<number | null>(null);

  const clearCloseTimer = useCallback(() => {
    if (closeTimerRef.current === null) return;
    window.clearTimeout(closeTimerRef.current);
    closeTimerRef.current = null;
  }, []);

  const updatePopoverPosition = useCallback((expanded = badgeExpanded) => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const margin = 16;
    const gap = 12;
    const fallbackWidth =
      (expanded ? agentMessageBadgeDetailWidth : agentMessageBadgeCompactWidth) *
      agentMessageBadgePopoverScale;
    const fallbackHeight =
      (expanded ? agentMessageBadgeDetailHeight : agentMessageBadgeCompactHeight) *
      agentMessageBadgePopoverScale;
    const content = popoverRef.current?.firstElementChild;
    const contentRect = content?.getBoundingClientRect();
    const visualWidth =
      contentRect && contentRect.width > 0
        ? Math.min(contentRect.width, window.innerWidth - margin * 2)
        : Math.min(fallbackWidth, window.innerWidth - margin * 2);
    const visualHeight =
      contentRect && contentRect.height > 0 ? contentRect.height : fallbackHeight;
    const maxVisualHeight = Math.max(120, window.innerHeight - margin * 2);
    const clampedVisualHeight = Math.min(visualHeight, maxVisualHeight);
    let left =
      preferredPlacement === "left"
        ? rect.left - visualWidth - gap
        : rect.right + gap;

    if (left + visualWidth > window.innerWidth - margin) {
      left = rect.left - visualWidth - gap;
    }
    if (left < margin) {
      left = Math.min(window.innerWidth - margin - visualWidth, rect.right + gap);
    }
    if (left < margin) left = margin;

    const maxTop = Math.max(margin, window.innerHeight - margin - clampedVisualHeight);
    const top = Math.max(margin, Math.min(rect.top - 12, maxTop));
    const availableVisualHeight = Math.max(120, window.innerHeight - top - margin);
    setPopoverStyle({
      left,
      top,
      "--agent-message-avatar-popover-max-height": `${availableVisualHeight / agentMessageBadgePopoverScale}px`,
    } as CSSProperties);
  }, [badgeExpanded, preferredPlacement]);

  const openPopover = useCallback(() => {
    clearCloseTimer();
    if (!open) setBadgeExpanded(false);
    updatePopoverPosition(false);
    setOpen(true);
  }, [clearCloseTimer, open, updatePopoverPosition]);

  const scheduleClose = useCallback(() => {
    clearCloseTimer();
    closeTimerRef.current = window.setTimeout(() => setOpen(false), 180);
  }, [clearCloseTimer]);

  useEffect(() => {
    if (!open) return;
    updatePopoverPosition();
    const reposition = () => updatePopoverPosition();
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  }, [open, updatePopoverPosition]);

  useLayoutEffect(() => {
    if (!open) return;
    updatePopoverPosition();
    const popover = popoverRef.current;
    if (!popover || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => updatePopoverPosition());
    observer.observe(popover);
    if (popover.firstElementChild) observer.observe(popover.firstElementChild);
    return () => observer.disconnect();
  }, [badgeExpanded, open, updatePopoverPosition]);

  useEffect(() => {
    if (!open) setBadgeExpanded(false);
  }, [open]);

  useEffect(() => () => clearCloseTimer(), [clearCloseTimer]);

  if (!entry || avatarActor?.kind !== "agent") {
    return <ActorAvatar actor={actor} fallback={fallback} small={small} />;
  }

  const badgeProps = agentIdentityBadgeProps(entry);
  const entryActorId = entry.agent.spec.actor.id;
  const display = displayName(avatarActor);

  function openSettings() {
    setOpen(false);
    onOpenAgentSettings(entryActorId);
  }

  const scaledPopoverStyle = {
    ...popoverStyle,
    "--agent-message-avatar-popover-scale": agentMessageBadgePopoverScale,
  } as CSSProperties;

  const popover =
    open && typeof document !== "undefined"
      ? createPortal(
          <div
            ref={popoverRef}
            className="agent-message-avatar-popover"
            style={scaledPopoverStyle}
            onFocus={openPopover}
            onMouseEnter={openPopover}
            onMouseLeave={scheduleClose}
            onKeyDown={(event) => {
              if (event.key === "Escape") setOpen(false);
            }}
          >
            <AgentIdentityBadge
              {...badgeProps}
              onAvatarClick={openSettings}
              onExpandedChange={setBadgeExpanded}
            />
          </div>,
          document.body,
        )
      : null;

  return (
    <>
      <span className="agent-message-avatar">
        <button
          ref={anchorRef}
          type="button"
          className="agent-message-avatar__button"
          title={`Open ${display} agent settings`}
          aria-label={`Open ${display} agent settings`}
          aria-haspopup="dialog"
          aria-expanded={open}
          onClick={openSettings}
          onFocus={openPopover}
          onBlur={scheduleClose}
          onMouseEnter={openPopover}
          onMouseLeave={scheduleClose}
        >
          <ActorAvatar actor={avatarActor} fallback={fallback} small={small} />
        </button>
      </span>
      {popover}
    </>
  );
}

function AvatarStack({
  actors,
  max,
  small,
}: {
  actors: Actor[];
  max: number;
  small?: boolean;
}) {
  const visible = actors.slice(0, max);
  const overflow = Math.max(0, actors.length - visible.length);
  return (
    <div className="flex items-center">
      {visible.map((actor, index) => (
        <img
          key={`${actor.id}:${index}`}
          alt=""
          src={actorAvatarUrl(actor, actor.id)}
          className={cn(
            "-ml-2 rounded-full border-2 border-white object-cover shadow-sm first:ml-0",
            small ? "h-6 w-6" : "h-8 w-8",
          )}
          title={displayName(actor)}
        />
      ))}
      {overflow > 0 && (
        <span
          className={cn(
            "-ml-2 inline-flex items-center justify-center rounded-full border-2 border-white bg-[#f1efff] text-[10px] font-bold text-[#5843d7]",
            small ? "h-6 min-w-6 px-1" : "h-8 min-w-8 px-1.5",
          )}
        >
          +{overflow}
        </span>
      )}
    </div>
  );
}

function useStickToBottomScroll({
  contentKey,
  itemCount,
  scrollKey,
}: {
  contentKey: string;
  itemCount: number;
  scrollKey: string;
}) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);

  useLayoutEffect(() => {
    stickToBottomRef.current = true;
  }, [scrollKey]);

  useLayoutEffect(() => {
    if (itemCount === 0) {
      stickToBottomRef.current = true;
      return;
    }
    const element = scrollRef.current;
    if (!element || !stickToBottomRef.current) return;
    const frame = window.requestAnimationFrame(() => {
      element.scrollTop = element.scrollHeight;
    });
    return () => window.cancelAnimationFrame(frame);
  }, [contentKey, itemCount, scrollKey]);

  const onScroll = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    const distanceFromBottom =
      element.scrollHeight - element.scrollTop - element.clientHeight;
    stickToBottomRef.current = distanceFromBottom < 160;
  }, []);

  return { onScroll, ref: scrollRef };
}

function initialViewportWidth() {
  return typeof window === "undefined" ? 1440 : window.innerWidth;
}

function loadPanelSizes(): PanelSizes {
  if (typeof window === "undefined") return defaultPanelSizes;
  try {
    const raw = window.localStorage.getItem(panelLayoutStorageKey);
    if (!raw) return defaultPanelSizes;
    const parsed = JSON.parse(raw) as Partial<PanelSizes>;
    return fitPanelSizes(
      {
        sidebar:
          typeof parsed.sidebar === "number"
            ? parsed.sidebar
            : defaultPanelSizes.sidebar,
        detail:
          typeof parsed.detail === "number"
            ? parsed.detail
            : defaultPanelSizes.detail,
      },
      initialViewportWidth(),
      initialViewportWidth() >= detailPanelBreakpoint,
    );
  } catch {
    return defaultPanelSizes;
  }
}

function savePanelSizes(sizes: PanelSizes) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(panelLayoutStorageKey, JSON.stringify(sizes));
  } catch {
    /* local-only preference; ignore quota or privacy-mode failures */
  }
}

function fitPanelSizes(
  sizes: PanelSizes,
  viewportWidth: number,
  detailVisible: boolean,
): PanelSizes {
  const sidebarMaxForViewport = detailVisible
    ? viewportWidth -
      railWidth -
      resizeHandleWidth * 2 -
      mainMinWidth -
      detailMinWidth
    : viewportWidth - railWidth - resizeHandleWidth - 240;
  const sidebar = clampNumber(
    sizes.sidebar,
    sidebarMinWidth,
    Math.max(sidebarMinWidth, Math.min(sidebarMaxWidth, sidebarMaxForViewport)),
  );
  const detailMaxForViewport =
    viewportWidth -
    railWidth -
    resizeHandleWidth * 2 -
    sidebar -
    mainMinWidth;
  const detail = clampNumber(
    sizes.detail,
    detailMinWidth,
    Math.max(detailMinWidth, Math.min(detailMaxWidth, detailMaxForViewport)),
  );
  return { sidebar, detail };
}

function clampNumber(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function channelGroupStorageKey(workspace: Workspace | null) {
  return `loom:channel-groups:v1:${workspace?.id ?? "global"}`;
}

function loadChannelGroups(key: string): ChannelGroup[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? normalizeChannelGroups(JSON.parse(raw)) : [];
  } catch {
    return [];
  }
}

function saveChannelGroups(key: string, groups: ChannelGroup[]) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(key, JSON.stringify(groups));
  } catch {
    /* local-only preference; ignore quota or privacy-mode failures */
  }
}

function normalizeChannelGroups(value: unknown): ChannelGroup[] {
  if (!Array.isArray(value)) return [];
  const seenGroupIds = new Set<string>();
  return value.flatMap((item, index) => {
    if (!item || typeof item !== "object") return [];
    const candidate = item as Partial<ChannelGroup>;
    const rawId =
      typeof candidate.id === "string" && candidate.id.trim()
        ? candidate.id.trim()
        : `local-${index}`;
    const id = seenGroupIds.has(rawId) ? `${rawId}-${index}` : rawId;
    seenGroupIds.add(id);
    const title =
      typeof candidate.title === "string" && candidate.title.trim()
        ? candidate.title.trim()
        : "Untitled";
    const channelIds = Array.isArray(candidate.channelIds)
      ? Array.from(
          new Set(
            candidate.channelIds.filter(
              (channelId): channelId is string =>
                typeof channelId === "string" && channelId.length > 0,
            ),
          ),
        )
      : [];
    return [
      {
        id,
        title,
        channelIds,
        collapsed: Boolean(candidate.collapsed),
      },
    ];
  });
}

function channelGroupSections(
  groups: ChannelGroup[],
  channels: Channel[],
): ChannelGroupSection[] {
  const channelsById = new Map(channels.map((channel) => [channel.id, channel]));
  const assigned = new Set<string>();
  const sections: ChannelGroupSection[] = groups.map((group) => {
    const groupChannels = group.channelIds.flatMap((channelId) => {
      const channel = channelsById.get(channelId);
      if (!channel || assigned.has(channel.id)) return [];
      assigned.add(channel.id);
      return [channel];
    });
    return {
      id: group.id,
      title: group.title,
      channels: groupChannels,
      collapsed: group.collapsed,
      local: true,
    };
  });
  const ungroupedChannels = channels.filter((channel) => !assigned.has(channel.id));
  if (groups.length === 0 || ungroupedChannels.length > 0) {
    sections.push({
      id: ungroupedChannelGroupId,
      title: groups.length === 0 ? "Channels" : "Ungrouped",
      channels: ungroupedChannels,
      collapsed: false,
      local: false,
    });
  }
  return sections;
}

function flattenThreads(
  threadsByChannel: Record<string, Thread[]>,
  channels: Channel[],
): ThreadWithChannel[] {
  const channelsById = new Map(channels.map((channel) => [channel.id, channel]));
  return Object.values(threadsByChannel)
    .flatMap((threads) =>
      threads.flatMap((thread) => {
        const channel = channelsById.get(thread.channelId);
        return channel ? [{ ...thread, channel }] : [];
      }),
    )
    .sort((a, b) => a.title.localeCompare(b.title));
}

function agentMemberEntries(machines: MachineInfo[]): AgentMemberEntry[] {
  return machines.flatMap((machine) =>
    machine.agents.map((agent) => ({ machine, agent })),
  );
}

function agentDisplayName(agent: MachineInfo["agents"][number]) {
  return displayName(agent.spec.actor);
}

function agentModelValue(agent: MachineInfo["agents"][number]) {
  return agent.spec.model ?? agent.spec.models?.default ?? "";
}

function agentDescriptionValue(agent: MachineInfo["agents"][number]) {
  return agent.spec.identity?.description ?? "";
}

function agentReasoningEffort(agent: MachineInfo["agents"][number]) {
  const value = agent.spec.actor._meta?.reasoningEffort;
  return typeof value === "string" ? value : "";
}

function agentAvatarValue(agent: MachineInfo["agents"][number]) {
  return metadataAvatarUrl(agent.spec.actor._meta) ?? actorAvatarUrl(agent.spec.actor, agent.spec.actor.id);
}

function providerForAgent(
  machine: MachineInfo,
  agent: MachineInfo["agents"][number],
  preferredProviderId?: string,
) {
  const preferred = machine.providers.find((provider) => provider.id === preferredProviderId);
  if (preferred) return preferred;
  const model = agentModelValue(agent);
  return (
    machine.providers.find(
      (provider) =>
        provider.defaultModel === model ||
        provider.modelChoices.some((choice) => choice.id === model),
    ) ??
    (machine.providers.length === 1 ? machine.providers[0] : undefined) ??
    machine.providers[0]
  );
}

function agentSettingsDraft(
  machine: MachineInfo,
  agent: MachineInfo["agents"][number],
): AgentSettingsDraft {
  const provider = providerForAgent(machine, agent);
  return {
    displayName: agentDisplayName(agent),
    description: agentDescriptionValue(agent),
    providerId: provider?.id ?? "",
    model: agentModelValue(agent) || provider?.defaultModel || "",
    reasoningEffort: agentReasoningEffort(agent),
    autostart: Boolean(agent.spec.autostart),
    avatarUrl: agentAvatarValue(agent),
  };
}

function agentIdentityBadgeProps(entry: AgentMemberEntry): AgentIdentityBadgeProps {
  const provider = providerForAgent(entry.machine, entry.agent);
  const providerName = provider?.name || provider?.id || "AI Runtime";
  const iconKey = agentProviderIconKey(provider?.id, providerName);
  return {
    avatarUrl: agentAvatarValue(entry.agent),
    agentName: agentDisplayName(entry.agent),
    providerName,
    providerMark: providerMark(providerName),
    providerIcon: iconKey ? <AgentProviderIcon iconKey={iconKey} /> : undefined,
    modelName: agentModelLabel(entry.agent, provider),
    usedTokensLabel: agentContextUsedLabel(entry.agent),
    remainingLabel: agentContextRemainingLabel(entry.agent),
    online: isOnlinePresenceStatus(entry.agent.status, entry.agent),
  };
}

function agentModelLabel(
  agent: MachineInfo["agents"][number],
  provider?: MachineAgentProviderInfo,
) {
  const model = agentModelValue(agent);
  const modelChoices = [
    ...(provider?.modelChoices ?? []),
    ...(agent.spec.models?.choices ?? []),
  ];
  return modelChoices.find((choice) => choice.id === model)?.label || model || "Default model";
}

function providerMark(providerName: string) {
  if (/anthropic|claude/i.test(providerName)) return "AI";
  const words = providerName.match(/[A-Za-z0-9]+/g) ?? [];
  if (words.length >= 2) {
    const first = words[0]?.[0] ?? "";
    const second = words[1]?.[0] ?? "";
    return `${first}${second}`.toUpperCase() || "AI";
  }
  return providerName.trim().slice(0, 2).toUpperCase() || "AI";
}

function agentContextUsedLabel(agent: MachineInfo["agents"][number]) {
  const meta = agent.spec._meta;
  const explicit = metadataString(meta, ["contextUsedLabel", "usedTokensLabel"]);
  if (explicit) return explicit;
  const used = metadataNumber(meta, ["contextUsedTokens", "usedTokens"]);
  const total = metadataNumber(meta, ["contextWindowTokens", "totalTokens", "maxTokens"]);
  if (typeof used === "number" && typeof total === "number" && total > 0) {
    return `${compactTokenCount(used)} / ${compactTokenCount(total)} tokens`;
  }
  return "112.0K / 128.0K tokens";
}

function agentContextRemainingLabel(agent: MachineInfo["agents"][number]) {
  const meta = agent.spec._meta;
  const explicit = metadataString(meta, ["contextRemainingLabel", "remainingLabel"]);
  if (explicit) return explicit;
  const remainingPercent = metadataNumber(meta, ["contextRemainingPercent", "remainingPercent"]);
  if (typeof remainingPercent === "number") {
    const normalized = remainingPercent <= 1 ? remainingPercent * 100 : remainingPercent;
    return `${Math.max(0, Math.round(normalized))}%`;
  }
  const used = metadataNumber(meta, ["contextUsedTokens", "usedTokens"]);
  const total = metadataNumber(meta, ["contextWindowTokens", "totalTokens", "maxTokens"]);
  if (typeof used === "number" && typeof total === "number" && total > 0) {
    return `${Math.max(0, Math.round(((total - used) / total) * 100))}%`;
  }
  return "13%";
}

function compactTokenCount(value: number) {
  if (value >= 1000) return `${(value / 1000).toFixed(1)}K`;
  return `${Math.round(value)}`;
}

function actorAvatarUrl(actor: Actor | undefined, fallback: string) {
  const metaAvatar = actor?._meta ? metadataAvatarUrl(actor._meta) : null;
  if (metaAvatar) return metaAvatar;
  const seed = `${actor?.kind ?? "actor"}:${actor?.id ?? fallback}:${actor ? displayName(actor) : ""}`;
  return avatarUrlForSeed(seed, actor?.kind);
}

function metadataAvatarUrl(meta: unknown) {
  if (!meta || typeof meta !== "object") return null;
  const value = (meta as { avatarUrl?: unknown }).avatarUrl;
  return typeof value === "string" && value.trim() ? value : null;
}

function avatarUrlForSeed(seed: string, kind: Actor["kind"] = "human") {
  const hash = stableHash(seed);
  const index =
    kind === "agent"
      ? agentAvatarIndexes[hash % agentAvatarIndexes.length]
      : (hash % avatarCount) + 1;
  return `/avatars/avatar-${String(index).padStart(2, "0")}.png`;
}

function stableHash(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash;
}

function accountToActor(account: HumanAccount): Actor {
  return {
    id: account.actorId,
    kind: "human",
    displayName: accountName(account),
    _meta: { avatarUrl: account.avatarUrl },
  };
}

function accountName(account: HumanAccount) {
  return account.nickname || account.realName || account.email || account.staffId;
}

function workspaceInitials(workspace: Workspace) {
  const words = workspace.name
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  const initials =
    words.length > 1
      ? `${words[0][0] ?? ""}${words[1][0] ?? ""}`
      : (words[0] ?? workspace.id).slice(0, 2);
  return initials.toUpperCase();
}

function displayName(actor: Actor) {
  return actor.displayName || actor.id;
}

function capitalize(value: string) {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}

function actorName(actors: Record<string, Actor>, actorId: string) {
  return actors[actorId] ? displayName(actors[actorId]) : actorId;
}

function channelTopic(channel: Channel | null | undefined) {
  return typeof channel?.topic === "string" ? channel.topic.trim() : "";
}

function connectionLabel(connection: ConnectionState) {
  if (connection === "open") return "Connected";
  if (connection === "connecting") return "Connecting";
  if (connection === "error") return "Connection error";
  if (connection === "closed") return "Disconnected";
  return "Idle";
}

function channelPanelTitle(tab: ChannelPanelTab) {
  if (tab === "threads") return "线程";
  if (tab === "members") return "成员";
  return "任务";
}

function channelPanelDetail(
  tab: ChannelPanelTab,
  threadCount: number,
  memberCount: number,
  taskCount: number,
) {
  if (tab === "threads") return `${threadCount} active threads`;
  if (tab === "members") return `${memberCount} members`;
  return `${taskCount} tasks`;
}

function actorKindLabel(actor: Actor) {
  if (actor.kind === "agent") return "智能体";
  if (actor.kind === "service") return "服务";
  return "成员";
}

function memberPresence(
  actor: Actor,
  machines: MachineInfo[],
  currentActorId: string | null,
): ChannelMemberPresence {
  if (actor.id === currentActorId) {
    return { online: true, label: "在线", status: "online" };
  }
  if (actor.kind === "agent") {
    const entry = findAgentMemberEntry(machines, actor.id);
    if (!entry) return { online: false, label: "离线", status: "offline" };
    const rawStatus = entry.agent.status || "offline";
    const online = isOnlinePresenceStatus(rawStatus, entry.agent);
    return {
      online,
      label: online ? "在线" : "离线",
      status: rawStatus,
    };
  }
  return { online: false, label: "离线", status: "offline" };
}

function findAgentMemberEntry(
  machines: MachineInfo[],
  actorId: string,
): AgentMemberEntry | null {
  for (const machine of machines) {
    const agent = machine.agents.find((item) => item.spec.actor.id === actorId);
    if (agent) return { machine, agent };
  }
  return null;
}

function isOnlinePresenceStatus(
  status: string,
  agent: MachineInfo["agents"][number],
) {
  const normalized = status.toLowerCase();
  if (["online", "connected", "running", "busy", "idle", "active"].includes(normalized)) {
    return true;
  }
  if (["offline", "stopped", "disconnected", "failed", "error", "exited"].includes(normalized)) {
    return false;
  }
  return Boolean(agent.pid || agent.sessionId);
}

function statusDotClass(status: string) {
  const normalized = status.toLowerCase();
  if (["online", "connected", "running", "busy", "idle", "active"].includes(normalized)) {
    return "bg-emerald-400";
  }
  if (normalized === "connecting" || normalized === "pending") return "bg-amber-400";
  if (normalized === "error" || normalized === "failed") return "bg-red-400";
  return "bg-[#98a2b3]";
}

function taskStatusBadgeClass(status: Task["status"]) {
  switch (status) {
    case "todo":
      return "border-slate-200 bg-slate-100 text-slate-700";
    case "claimed":
      return "border-violet-200 bg-violet-50 text-violet-700";
    case "in_progress":
      return "border-blue-200 bg-blue-50 text-blue-700";
    case "waiting_review":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "done":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "failed":
      return "border-red-200 bg-red-50 text-red-700";
    case "canceled":
      return "border-slate-200 bg-slate-100 text-slate-500";
  }
}

function channelTaskGroups(tasks: Task[]) {
  const groups = [
    {
      id: "todo",
      title: "待办",
      tasks: tasks.filter((task) => task.status === "todo" || task.status === "claimed"),
    },
    {
      id: "active",
      title: "进行中",
      tasks: tasks.filter(
        (task) => task.status === "in_progress" || task.status === "waiting_review",
      ),
    },
    {
      id: "done",
      title: "已完成",
      tasks: tasks.filter((task) => task.status === "done"),
    },
    {
      id: "other",
      title: "其他",
      tasks: tasks.filter(
        (task) =>
          task.status === "failed" ||
          task.status === "canceled" ||
          ![
            "todo",
            "claimed",
            "in_progress",
            "waiting_review",
            "done",
          ].includes(task.status),
      ),
    },
  ];
  return groups.filter((group) => group.tasks.length > 0);
}

function taskProgressPercent(task: Task) {
  if (task.status === "in_progress") return 60;
  if (task.status === "waiting_review") return 85;
  if (task.status === "done") return 100;
  return null;
}

function formatShortDateTime(value: string) {
  const date = parseMessageDate(value);
  if (!date) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function reconnectDelayMs(attempt: number) {
  return Math.min(15_000, 500 * 2 ** Math.max(0, attempt - 1));
}

function normalizeAgentForm(form: AgentFormState, machines: MachineInfo[]): AgentFormState {
  const machine = resolveAgentMachine(form, machines);
  const provider = resolveAgentProvider(form, machine);
  return {
    ...form,
    machineId: machine?.id ?? "",
    providerId: provider?.id ?? "",
    model: form.model || provider?.defaultModel || "",
  };
}

function agentFormForMachine(
  form: AgentFormState,
  machine?: MachineInfo,
): AgentFormState {
  const provider = resolveAgentProvider(form, machine);
  return {
    ...form,
    machineId: machine?.id ?? "",
    providerId: provider?.id ?? "",
    model: provider?.defaultModel || "",
  };
}

function resolveAgentMachine(
  form: AgentFormState,
  machines: MachineInfo[],
): MachineInfo | undefined {
  const current = machines.find((item) => item.id === form.machineId);
  return (
    current ??
    machines.find((machine) => machineCanCreateAgent(machine) && machine.providers.length > 0) ??
    machines.find(machineCanCreateAgent) ??
    machines[0]
  );
}

function resolveAgentProvider(
  form: AgentFormState,
  machine?: MachineInfo,
): MachineAgentProviderInfo | undefined {
  return (
    machine?.providers.find((item) => item.id === form.providerId) ??
    machine?.providers[0]
  );
}

function machineCanCreateAgent(machine: MachineInfo) {
  return (
    !machine.readOnly &&
    (machine.capabilities.includes("agent.create") ||
      (machine.canCommand && machine.capabilities.includes("machine.command")))
  );
}

function mentionAudience(
  body: string,
  actors: Record<string, Actor>,
  selfActorId?: string,
): AudienceRef[] {
  const audience: AudienceRef[] = [];
  const actorList = Object.values(actors);
  for (const rawToken of mentionTokens(body)) {
    const key = rawToken.toLowerCase();
    if (key === "all") {
      audience.push({ kind: "all", id: "all", display: "@all" });
      continue;
    }
    if (key === "agents") {
      audience.push({ kind: "agents", id: "agents", display: "@agents" });
      continue;
    }
    if (key === "humans") {
      audience.push({ kind: "humans", id: "humans", display: "@humans" });
      continue;
    }
    const actor = actorList.find((candidate) => {
      if (candidate.id === selfActorId) return false;
      return (
        candidate.id.toLowerCase() === key ||
        displayName(candidate).toLowerCase() === key ||
        shortActorAlias(candidate.id).toLowerCase() === key
      );
    });
    if (actor) audience.push({ kind: "actor", id: actor.id, display: `@${rawToken}` });
  }
  return uniqueAudience(audience);
}

function mentionTokens(body: string) {
  return Array.from(body.matchAll(/@([^\s,.;:!?()[\]{}<>"'`]+)/g), (match) => match[1]);
}

type ActiveMention = {
  start: number;
  end: number;
  query: string;
};

type MentionOption = {
  kind: "all" | "actor";
  id: string;
  token: string;
  title: string;
  detail: string;
  start: number;
  end: number;
  actor?: Actor;
};

function activeMentionQuery(body: string, caretIndex: number): ActiveMention | null {
  const prefix = body.slice(0, caretIndex);
  const match = /(^|\s)@([^\s@]*)$/.exec(prefix);
  if (!match) return null;
  const query = match[2] ?? "";
  return {
    start: match.index + match[1].length,
    end: caretIndex,
    query,
  };
}

function mentionCandidates(agents: Actor[], active: ActiveMention): MentionOption[] {
  const query = active.query.toLowerCase();
  const options: MentionOption[] = [
    {
      kind: "all",
      id: "all",
      token: "@all",
      title: "All",
      detail: "Notify everyone in this channel",
      start: active.start,
      end: active.end,
    },
    ...agents.map((agent) => {
      const token = mentionTokenForActor(agent);
      return {
        kind: "actor" as const,
        id: agent.id,
        token,
        title: displayName(agent),
        detail: agent.id,
        start: active.start,
        end: active.end,
        actor: agent,
      };
    }),
  ];
  if (!query) return options;
  return options.filter((option) =>
    [option.token.slice(1), option.title, option.detail]
      .map((value) => value.toLowerCase())
      .some((value) => value.includes(query)),
  );
}

function uniqueAudience(audience: AudienceRef[]) {
  const seen = new Set<string>();
  return audience.filter((entry) => {
    const key = `${entry.kind}:${entry.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function audienceWakesAgent(audience: AudienceRef, actors: Record<string, Actor>) {
  if (audience.kind === "all" || audience.kind === "agents") return true;
  if (audience.kind !== "actor") return false;
  return actors[audience.id]?.kind === "agent";
}

function mentionTokenForActor(actor: Actor) {
  const name = displayName(actor).trim();
  if (name && !/[\s,.;:!?()[\]{}<>"'`@]/.test(name)) {
    return `@${name}`;
  }
  return `@${shortActorAlias(actor.id)}`;
}

function shortActorAlias(actorId: string) {
  return (
    actorId.replace(
      /^(actor_agent_|actor_human_|actor_service_|actor_)/,
      "",
    ) || actorId
  );
}

function canUseAsThreadRoot(message: Message) {
  return (
    message.kind !== "system" &&
    message.scope.kind === "channel" &&
    !message.parentMessageId &&
    !message.threadRootMessageId
  );
}

function isChannelMember(channel: Channel, actorId: string) {
  return channel.visibility === "public" || channel.members.includes(actorId);
}

function isExplicitChannelMember(channel: Channel, actorId: string) {
  return channel.members.includes(actorId);
}

function canAddChannelMember(channel: Channel, actor: Actor) {
  return actor.kind !== "service" && !isExplicitChannelMember(channel, actor.id);
}

function canRemoveChannelMember(channel: Channel, actorId: string) {
  if (!isExplicitChannelMember(channel, actorId)) return false;
  if (channel.visibility === "public") return true;
  return channel.members[0] !== actorId;
}

function fallbackActor(actorId: string): Actor {
  if (actorId.startsWith("actor_agent_")) return { id: actorId, kind: "agent" };
  if (actorId.startsWith("actor_service_")) return { id: actorId, kind: "service" };
  return { id: actorId, kind: "human" };
}

function isWorkflowMessage(message: Message) {
  return (
    message.kind === "task_update" ||
    message.intent === "assign_task" ||
    typeof message.metadata?.assignmentId === "string" ||
    /^Assignment\s+\S+.*\bcompleted\b/i.test(message.body.trim())
  );
}

function isWorkflowResultMessage(message: Message, workflowSourceIds: Set<string>) {
  return (
    message.kind === "agent" &&
    Boolean(message.parentMessageId && workflowSourceIds.has(message.parentMessageId))
  );
}

function isHiddenProtocolMessage(message: Message) {
  return /^(accepted|declined):\s*(accepted|declined)$/i.test(message.body.trim());
}

function workflowSummary(message: Message, actors: Record<string, Actor>) {
  const taskNumber = numberMetadata(message, "taskNumber");
  const taskLabel = taskNumber ? `Task #${taskNumber}` : "Task";
  const recipient = message.audience.find((audience) => audience.kind === "actor")?.id;
  if (message.intent === "assign_task") {
    return recipient
      ? `${taskLabel} assigned to ${actorName(actors, recipient)}`
      : `${taskLabel} assigned`;
  }
  if (/completed/i.test(message.body)) {
    return `${taskLabel} completed`;
  }
  return `${taskLabel} updated`;
}

function workflowResultSummary(message: Message) {
  const resultLine = message.body
    .split("\n")
    .map((line) => line.trim())
    .find((line) => /^Result summary:/i.test(line));
  if (resultLine) return resultLine.replace(/^Result summary:\s*/i, "");

  const usefulLines = message.body
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .filter((line) => !/^let me\b/i.test(line))
    .filter((line) => !/\bloom\b.*\b(cli|socket|PATH)\b/i.test(line))
    .filter((line) => !/^found a loom binary/i.test(line));
  return usefulLines.at(-1) ?? "Task result posted.";
}

function numberMetadata(message: Message, key: string) {
  const value = message.metadata?.[key];
  return typeof value === "number" ? value : null;
}

function sameScope(a: ScopeRef, b: ScopeRef) {
  return a.kind === b.kind && a.id === b.id;
}

function sortChannels(items: Channel[]) {
  return [...items].sort((a, b) => a.title.localeCompare(b.title));
}

function sortThreads(items: Thread[]) {
  return [...items]
    .filter((thread) => !thread.archivedAt)
    .sort((a, b) => a.title.localeCompare(b.title));
}

function sortMessages(items: Message[]) {
  return [...items].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
}

function normalizeMessage(message: Message): Message {
  return {
    ...message,
    attachments: message.attachments ?? [],
    reactions: message.reactions ?? [],
  };
}

function upsertMessage(items: Message[], message: Message) {
  return upsert(items, normalizeMessage(message));
}

function groupMessagesByDate(messages: Message[]) {
  const groups = new Map<
    string,
    { key: string; label: string; messages: Message[] }
  >();
  for (const message of messages) {
    const key = messageDateKey(message.createdAt);
    const group = groups.get(key);
    if (group) {
      group.messages.push(message);
    } else {
      groups.set(key, {
        key,
        label: messageDateLabel(message.createdAt),
        messages: [message],
      });
    }
  }
  return Array.from(groups.values());
}

function messageDateKey(value: string) {
  const date = parseMessageDate(value);
  if (!date) return "undated";
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function messageDateLabel(value: string) {
  const date = parseMessageDate(value);
  if (!date) return "Undated";
  const today = startOfLocalDay(new Date());
  const day = startOfLocalDay(date);
  const diffDays = Math.round((today.getTime() - day.getTime()) / 86_400_000);
  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  const sameYear = date.getFullYear() === today.getFullYear();
  return new Intl.DateTimeFormat(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(date);
}

function parseMessageDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

function startOfLocalDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function sortTasks(items: Task[]) {
  return [...items].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
}

function upsert<T extends { id: string }>(items: T[], item: T) {
  const index = items.findIndex((candidate) => candidate.id === item.id);
  if (index === -1) return [...items, item];
  const next = [...items];
  next[index] = { ...next[index], ...item };
  return next;
}

function messageKind(message: Message) {
  const kind = message.metadata?.kind;
  return typeof kind === "string" ? kind : "chat";
}

function messageIsActionRequestFor(message: Message, actorId: string | null) {
  if (!actorId || messageKind(message) !== "action.request") return false;
  return message.audience.some(
    (audience) => audience.kind === "actor" && audience.id === actorId,
  );
}

function actionChoices(message: Message): ActionChoice[] {
  const raw = message.metadata?.choices;
  if (Array.isArray(raw)) {
    const parsed = raw
      .flatMap((choice): ActionChoice[] => {
        if (!choice || typeof choice !== "object") return [];
        const record = choice as Record<string, unknown>;
        const id = String(record.id ?? record.label ?? "");
        if (!id) return [];
        const label = String(record.label ?? id);
        return [
          {
            id,
            label,
            accepted: !/reject|decline|cancel|no/i.test(label),
            votes: votesFromChoice(record),
          },
        ];
      });
    if (parsed.length > 0) return parsed;
  }
  return [];
}

function bodyPollFromMessage(message: Message): BodyPoll | null {
  const lines = message.body.split("\n");
  const choices: ActionChoice[] = [];
  const questionLines: string[] = [];
  let foundChoice = false;
  for (const line of lines) {
    const match = /^\s*([A-Za-z])[\).]\s+(.+?)\s*$/.exec(line);
    if (match) {
      foundChoice = true;
      choices.push({
        id: match[1].toUpperCase(),
        label: match[2],
        accepted: true,
      });
    } else if (!foundChoice || line.trim()) {
      questionLines.push(line);
    }
  }
  if (choices.length < 2) return null;
  const question = questionLines.join("\n").trim() || messageTitle(message);
  return { question, choices };
}

function threadParticipants(
  thread: Thread,
  actors: Record<string, Actor>,
  rootAuthor?: Actor,
  stats?: ThreadActivityStats,
) {
  const metaActorIds = stats?.participantActorIds.length
    ? stats.participantActorIds
    : metadataStringArray(thread._meta, [
        "participantActorIds",
        "participants",
        "replyActorIds",
      ]);
  const candidates = [
    ...(rootAuthor ? [rootAuthor] : []),
    ...metaActorIds.map((actorId) => actors[actorId] ?? fallbackActor(actorId)),
  ];
  const seen = new Set<string>();
  return candidates.filter((actor) => {
    if (seen.has(actor.id)) return false;
    seen.add(actor.id);
    return true;
  });
}

function threadReplyCount(thread: Thread, stats?: ThreadActivityStats) {
  if (typeof stats?.replyCount === "number") return stats.replyCount;
  const replyCount = metadataNumber(thread._meta, ["replyCount", "replies"]);
  if (replyCount !== null) return Math.max(0, replyCount);
  const messageCount = metadataNumber(thread._meta, ["messageCount"]);
  return messageCount === null ? null : Math.max(0, messageCount - 1);
}

function threadLastReplyLabel(thread: Thread, stats?: ThreadActivityStats) {
  const raw =
    stats?.lastReplyAt ??
    metadataString(thread._meta, ["lastReplyAt", "lastMessageAt", "updatedAt"]);
  return raw ? formatTime(raw) : null;
}

function emptyThreadStats(): ThreadActivityStats {
  return {
    replyCount: 0,
    replyMessageIds: [],
    participantActorIds: [],
    hasMoreReplies: false,
    lastReplyAt: null,
  };
}

function threadStatsFromMessages(
  messages: Message[],
  hasMoreReplies = false,
): ThreadActivityStats {
  const sorted = sortMessages(messages).filter(
    (message) => !isHiddenProtocolMessage(message),
  );
  const participantActorIds = uniqueStrings(
    sorted.map((message) => message.authorActorId),
  );
  return {
    replyCount: sorted.length,
    replyMessageIds: sorted.map((message) => message.id),
    participantActorIds,
    hasMoreReplies,
    lastReplyAt: sorted.at(-1)?.createdAt ?? null,
  };
}

function upsertThreadStatsMessage(
  current: Record<string, ThreadActivityStats>,
  message: Message,
) {
  if (message.scope.kind !== "thread" || isHiddenProtocolMessage(message)) return current;
  const previous = current[message.scope.id] ?? emptyThreadStats();
  const knownMessage = previous.replyMessageIds.includes(message.id);
  return {
    ...current,
    [message.scope.id]: {
      replyCount: knownMessage ? previous.replyCount : previous.replyCount + 1,
      replyMessageIds: knownMessage
        ? previous.replyMessageIds
        : [...previous.replyMessageIds, message.id],
      participantActorIds: uniqueStrings([
        ...previous.participantActorIds,
        message.authorActorId,
      ]),
      hasMoreReplies: previous.hasMoreReplies,
      lastReplyAt:
        !previous.lastReplyAt || message.createdAt > previous.lastReplyAt
          ? message.createdAt
          : previous.lastReplyAt,
    },
  };
}

function uniqueStrings(values: string[]) {
  return Array.from(new Set(values.filter((value) => value.length > 0)));
}

function metadataString(meta: unknown, keys: string[]) {
  if (!meta || typeof meta !== "object") return null;
  const record = meta as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return null;
}

function metadataNumber(meta: unknown, keys: string[]) {
  if (!meta || typeof meta !== "object") return null;
  const record = meta as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return null;
}

function metadataStringArray(meta: unknown, keys: string[]) {
  if (!meta || typeof meta !== "object") return [];
  const record = meta as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (Array.isArray(value)) {
      return value.filter(
        (item): item is string => typeof item === "string" && item.length > 0,
      );
    }
  }
  return [];
}

function votesFromChoice(record: Record<string, unknown>) {
  for (const key of ["votes", "voteCount", "count"]) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  const actorIds = record.actorIds ?? record.voterIds ?? record.votesByActor;
  if (Array.isArray(actorIds)) return actorIds.length;
  return undefined;
}

function attachmentTitle(value: string) {
  const clean = value.trim();
  if (!clean) return "Attachment";
  try {
    const url = new URL(clean);
    return decodeURIComponent(url.pathname.split("/").filter(Boolean).at(-1) ?? url.hostname);
  } catch {
    return clean.split(/[\\/]/).filter(Boolean).at(-1) ?? clean;
  }
}

function attachmentKind(value: string) {
  const lower = value.toLowerCase();
  if (lower.endsWith(".fig") || lower.includes("figma")) return "Figma File";
  if (lower.endsWith(".pdf")) return "PDF File";
  if (lower.endsWith(".doc") || lower.endsWith(".docx") || lower.includes("doc")) {
    return "Google Doc";
  }
  if (lower.endsWith(".sheet") || lower.endsWith(".xlsx") || lower.endsWith(".csv")) {
    return "Spreadsheet";
  }
  if (lower.match(/\.(png|jpe?g|webp|gif)$/)) return "Image";
  return "File";
}

function messageTitle(message: Message) {
  const title = message.metadata?.title;
  if (typeof title === "string" && title.trim()) return title;
  return message.body.trim().split("\n")[0] || shortId(message.id);
}

function metadataText(message: Message) {
  const reason = message.metadata?.reason;
  if (typeof reason === "string") return reason;
  return messageTitle(message);
}

function threadTitle(message: Message) {
  return messageTitle(message).slice(0, 80);
}

function channelFromMessage(message: Message) {
  if (message.scope.kind === "channel") return message.scope.id;
  const match = message.target.match(/^#([^:]+)/);
  return match?.[1] ?? message.scope.id;
}

function threadIdForMessage(
  threadsByChannel: Record<string, Thread[]>,
  message: Message,
) {
  const channelId = channelFromMessage(message);
  const root = message.threadRootMessageId ?? message.parentMessageId ?? message.id;
  return (
    threadsByChannel[channelId]?.find((thread) => thread.rootMessageId === root)
      ?.id ?? null
  );
}

function errorText(err: unknown) {
  return err instanceof Error ? err.message : String(err);
}
