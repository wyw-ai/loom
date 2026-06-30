import {
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import type { CSSProperties, PointerEvent } from "react";
import { Loader2 } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import {
  channelTarget,
  scopeKey,
  threadTarget,
  type Channel,
  type DesktopConfig,
  type MachineInfo,
  type Message,
  type Run,
  type ScopeRef,
  type StreamUpdate,
  type Task,
  type Thread,
  type Workspace
} from "@/ipc/types";

import { cn } from "@/lib/utils";

import type {
  AgentFormState,
  AgentUpdatePatch,
  ChannelGroup,
  PanelResizeDrag,
  PanelResizeKind,
} from "@/lib/types";
import {
  defaultAgentPromptAssembly,
  detailPanelBreakpoint,
  localServerCommand,
  machineStatusPollIntervalMs,
  mainMinWidth,
  ungroupedChannelGroupId
} from "@/lib/constants";
import {
  defaultWorkspaceForm,
  normalizeWorkspaceFormServerUrl,
} from "@/lib/server-url";

import {
  channelGroupStorageKey,
  channelMentionActors,
  channelMentionAgentActors,
  directChannelPeerId,
  directMessageTarget,
  directPeerForMessage,
  directScopeForActor,
  flattenThreads,
  isChannelMentionActor,
  isDirectChannel,
  loadChannelGroups,
  messageBelongsToDirectActor,
  normalizeChannelGroups,
  sameScope,
  saveChannelGroups,
  sortChannels,
} from "@/lib/channel-utils";

import {
  channelFromMessage,
  emptyThreadStats,
  messageIsActionRequestFor,
  normalizeMessage,
  sortMessages,
  threadIdForMessage,
  threadStatsFromMessages,
  threadTitle,
  upsertMessage,
  upsertThreadStatsMessage,
} from "@/lib/message-utils";

import {
  accountName,
  accountToActor,
  actorName,
  audienceWakesAgent,
  displayName,
  errorText,
  filterEmptyEnvKeys,
  findAgentMemberEntry,
  fitPanelSizes,
  initialViewportWidth,
  machineCanCreateAgent,
  mentionAudience,
  normalizeAgentForm,
  reconnectDelayMs,
  resolveAgentMachine,
  resolveAgentProvider,
  savePanelSizes,
  sortTasks,
  sortThreads,
  uniqueAudience,
  uniqueActorsById,
  upsert,
} from "@/lib/format-utils";

// Zustand stores (P2)
import { useConnectionStore } from "@/store/connectionStore";
import { useUIStore } from "@/store/uiStore";
import { useChannelStore } from "@/store/channelStore";
import { useMessageStore } from "@/store/messageStore";
import { useActorStore } from "@/store/actorStore";
import { useTaskStore } from "@/store/taskStore";
import { applyMessageToUsageStore } from "@/store/usageStore";

// P3 — Extracted layout / shared components
import { Rail } from "@/components/layout/Rail";
import { ResizeHandle } from "@/components/layout/ResizeHandle";
import { Sidebar } from "@/components/layout/Sidebar";
import { ChatHeader } from "@/components/layout/ChatHeader";

// P4 — Extracted chat / message components
import { MessageFeed } from "@/components/chat/MessageFeed";
import { Composer } from "@/components/chat/Composer";
import { ThreadPanel } from "@/components/chat/ThreadPanel";

// P5 — Extracted shared / view / panel components
import { ErrorBanner, NoSpaceConnectionGuide } from "@/components/shared/PageComponents";
import { ThreadsView } from "@/components/views/ThreadsView";
import { ChannelsView } from "@/components/views/ChannelsView";
import { DirectMessagesView } from "@/components/views/DirectMessagesView";
import { InboxView } from "@/components/views/InboxView";
import { TasksView } from "@/components/views/TasksView";
import { SpacesView } from "@/components/views/SpacesView";
import { AccountView } from "@/components/views/AccountView";
import { SettingsView } from "@/components/views/SettingsView";
import { OnboardingView } from "@/components/views/OnboardingView";
import { ChannelPanel } from "@/components/panels/ChannelPanels";


export function App() {
  // --- Connection store ---
  const config = useConnectionStore((s) => s.config);
  const setConfig = useConnectionStore((s) => s.setConfig);
  const workspace = useConnectionStore((s) => s.workspace);
  const setWorkspace = useConnectionStore((s) => s.setWorkspace);
  const connection = useConnectionStore((s) => s.connection);
  const setConnection = useConnectionStore((s) => s.setConnection);
  const error = useConnectionStore((s) => s.error);
  const setError = useConnectionStore((s) => s.setError);
  const notice = useConnectionStore((s) => s.notice);
  const setNotice = useConnectionStore((s) => s.setNotice);
  const busy = useConnectionStore((s) => s.busy);
  const setBusy = useConnectionStore((s) => s.setBusy);
  const workspaceForm = useConnectionStore((s) => s.workspaceForm);
  const setWorkspaceForm = useConnectionStore((s) => s.setWorkspaceForm);

  // --- UI store ---
  const view = useUIStore((s) => s.view);
  const setView = useUIStore((s) => s.setView);
  const settingsAgentId = useUIStore((s) => s.settingsAgentId);
  const setSettingsAgentId = useUIStore((s) => s.setSettingsAgentId);
  const channelPanelTab = useUIStore((s) => s.channelPanelTab);
  const setChannelPanelTab = useUIStore((s) => s.setChannelPanelTab);
  const panelSizes = useUIStore((s) => s.panelSizes);
  const setPanelSizes = useUIStore((s) => s.setPanelSizes);
  const viewportWidth = useUIStore((s) => s.viewportWidth);
  const setViewportWidth = useUIStore((s) => s.setViewportWidth);
  const resizingPanel = useUIStore((s) => s.resizingPanel);
  const setResizingPanel = useUIStore((s) => s.setResizingPanel);
  const replyTo = useUIStore((s) => s.replyTo);
  const setReplyTo = useUIStore((s) => s.setReplyTo);

  // --- Channel store ---
  const channels = useChannelStore((s) => s.channels);
  const setChannels = useChannelStore((s) => s.setChannels);
  const channelGroups = useChannelStore((s) => s.channelGroups);
  const setChannelGroups = useChannelStore((s) => s.setChannelGroups);
  const threadsByChannel = useChannelStore((s) => s.threadsByChannel);
  const setThreadsByChannel = useChannelStore((s) => s.setThreadsByChannel);
  const activeChannelId = useChannelStore((s) => s.activeChannelId);
  const setActiveChannelId = useChannelStore((s) => s.setActiveChannelId);
  const activeThreadId = useChannelStore((s) => s.activeThreadId);
  const setActiveThreadId = useChannelStore((s) => s.setActiveThreadId);
  const activeDirectActorId = useChannelStore((s) => s.activeDirectActorId);
  const setActiveDirectActorId = useChannelStore((s) => s.setActiveDirectActorId);
  const directScopesByActorId = useChannelStore((s) => s.directScopesByActorId);
  const setDirectScopesByActorId = useChannelStore((s) => s.setDirectScopesByActorId);

  // --- Message store ---
  const messages = useMessageStore((s) => s.messages);
  const setMessages = useMessageStore((s) => s.setMessages);
  const threadMessages = useMessageStore((s) => s.threadMessages);
  const setThreadMessages = useMessageStore((s) => s.setThreadMessages);
  const directMessages = useMessageStore((s) => s.directMessages);
  const setDirectMessages = useMessageStore((s) => s.setDirectMessages);
  const threadStatsById = useMessageStore((s) => s.threadStatsById);
  const setThreadStatsById = useMessageStore((s) => s.setThreadStatsById);
  const draft = useMessageStore((s) => s.draft);
  const setDraft = useMessageStore((s) => s.setDraft);
  const threadDraft = useMessageStore((s) => s.threadDraft);
  const setThreadDraft = useMessageStore((s) => s.setThreadDraft);
  const directDraft = useMessageStore((s) => s.directDraft);
  const setDirectDraft = useMessageStore((s) => s.setDirectDraft);

  // --- Actor store ---
  const actors = useActorStore((s) => s.actors);
  const setActors = useActorStore((s) => s.setActors);
  const runs = useActorStore((s) => s.runs);
  const setRuns = useActorStore((s) => s.setRuns);
  const inbox = useActorStore((s) => s.inbox);
  const setInbox = useActorStore((s) => s.setInbox);
  const machines = useActorStore((s) => s.machines);
  const setMachines = useActorStore((s) => s.setMachines);

  // --- Task store ---
  const tasks = useTaskStore((s) => s.tasks);
  const setTasks = useTaskStore((s) => s.setTasks);

  // Component-local state (to be extracted in P3-P6)
  const [agentForm, setAgentForm] = useState<AgentFormState>({
    machineId: "",
    providerId: "",
    actorId: "",
    name: "Echo",
    description: "",
    instructions: "Reply concisely and report completed work.",
    model: "",
    autostart: true,
    env: {},
  });
  const [configLoaded, setConfigLoaded] = useState(false);
  const [onboardingActive, setOnboardingActive] = useState(false);

  const activeScopeRef = useRef<ScopeRef | null>(null);
  const activeThreadScopeRef = useRef<ScopeRef | null>(null);
  const activeDirectScopeRef = useRef<ScopeRef | null>(null);
  const activeDirectActorIdRef = useRef<string | null>(null);
  const actorIdRef = useRef<string | null>(null);
  const targetRef = useRef<string | null>(null);
  const workspaceRef = useRef<Workspace | null>(null);
  const autoReconnectRef = useRef(false);
  const hasOpenedConnectionRef = useRef(false);
  const reconnectTimerRef = useRef<number | null>(null);
  const reconnectAttemptRef = useRef(0);
  const panelResizeDragRef = useRef<PanelResizeDrag | null>(null);
  const panelResizeCleanupRef = useRef<(() => void) | null>(null);

  const account = config.account ?? null;
  const workspaces = config.workspaces ?? [];
  const activeWorkspaceId = workspace?.id ?? null;
  const visibleChannels = channels.filter((channel) => !isDirectChannel(channel));
  const activeChannel =
    visibleChannels.find((channel) => channel.id === activeChannelId) ?? null;
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
  const allThreads = flattenThreads(threadsByChannel, visibleChannels);
  const channelIdsKey = visibleChannels.map((channel) => channel.id).join("|");
  const actorList = Object.values(actors).sort((a, b) =>
    displayName(a).localeCompare(displayName(b)),
  );
  const agentActors = actorList.filter((actor) => actor.kind === "agent");
  const agentActorIdsKey = agentActors.map((actor) => actor.id).join("|");
  const activeDirectActor =
    agentActors.find((actor) => actor.id === activeDirectActorId) ??
    (activeDirectActorId
      ? findAgentMemberEntry(machines, activeDirectActorId)?.agent.spec.actor ?? null
      : null);
  const activeDirectTarget = activeDirectActor
    ? directMessageTarget(activeDirectActor.id)
    : null;
  const activeDirectScope =
    activeDirectActor && workspace
      ? directScopeForActor(channels, workspace.actorId, activeDirectActor.id) ??
        directScopesByActorId[activeDirectActor.id] ??
        null
      : null;
  const memberCandidates = uniqueActorsById(
    actorList.filter((actor) => actor.kind !== "service"),
  );
  const channelAgentActors = activeChannel
    ? channelMentionAgentActors(activeChannel, actors)
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
  const consumeSettingsAgentTarget = useCallback(() => {
    setSettingsAgentId(null);
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

  const prepareLocalServerSpace = useCallback(() => {
    setWorkspaceForm(defaultWorkspaceForm());
    setView("spaces");
    if (navigator.clipboard?.writeText) {
      void navigator.clipboard
        .writeText(localServerCommand)
        .then(() => pushNotice("Server command copied"))
        .catch(() => pushNotice("Open Spaces after starting the server"));
      return;
    }
    pushNotice("Open Spaces after starting the server");
  }, [pushNotice]);

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
      if (!next.account || next.workspaces.length === 0) {
        setOnboardingActive(true);
      }
      void loadMachines().catch(() => {});
    } catch (err) {
      setError(errorText(err));
    } finally {
      setConfigLoaded(true);
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
        const nextVisibleChannels = nextChannels.filter((channel) => !isDirectChannel(channel));
        setChannels(nextChannels);
        setActiveChannelId((currentChannel) =>
          currentChannel &&
          nextVisibleChannels.some((channel) => channel.id === currentChannel)
            ? currentChannel
            : nextVisibleChannels[0]?.id ?? null,
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
    async (
      workspaceId: string,
      options: { automatic?: boolean; quiet?: boolean; reconnect?: boolean } = {},
    ): Promise<Workspace | null> => {
      const automatic = options.automatic === true;
      const quiet = options.quiet === true;
      const reconnecting =
        options.reconnect === true ||
        (automatic && hasOpenedConnectionRef.current && autoReconnectRef.current);
      if (workspaceRef.current?.id === workspaceId && connection === "open") {
        return workspaceRef.current;
      }
      if (!automatic) {
        autoReconnectRef.current = true;
        reconnectAttemptRef.current = 0;
      }
      clearReconnectTimer();
      if (!automatic && !quiet) setBusy(`connect:${workspaceId}`);
      setConnection("connecting");
      setError(reconnecting ? "Connection lost. Reconnecting..." : null);
      try {
        const result = await ipc.connect(workspaceId);
        workspaceRef.current = result.workspace;
        hasOpenedConnectionRef.current = true;
        autoReconnectRef.current = true;
        setWorkspace(result.workspace);
        setConnection("open");
        setError(null);
        reconnectAttemptRef.current = 0;
        await loadWorkspaceData(result.workspace);
        if (!quiet) {
          pushNotice(
            automatic
              ? `Reconnected to ${result.workspace.name}`
              : `Connected to ${result.workspace.name}`,
          );
        }
        return result.workspace;
      } catch (err) {
        setConnection("error");
        if (reconnecting) {
          setError("Connection lost. Reconnecting...");
        } else {
          autoReconnectRef.current = false;
          setError(automatic || quiet ? null : errorText(err));
        }
        return null;
      } finally {
        if (!automatic && !quiet) setBusy(null);
      }
    },
    [clearReconnectTimer, connection, loadWorkspaceData, pushNotice],
  );

  useEffect(() => {
    let unlistenStream: (() => void) | null = null;
    let unlistenConnection: (() => void) | null = null;

    void loadConfig();
    void ipc.onStream((update) => handleStream(update)).then((off) => {
      unlistenStream = off;
    });
    void ipc.onConnection((event) => {
      if (event.state === "closed") {
        setConnection("closed");
        if (
          hasOpenedConnectionRef.current &&
          autoReconnectRef.current &&
          workspaceRef.current
        ) {
          setError("Connection lost. Reconnecting...");
        } else {
          autoReconnectRef.current = false;
          setError(null);
        }
        void loadMachines(true).catch(() => {});
      } else {
        hasOpenedConnectionRef.current = true;
        if (workspaceRef.current) autoReconnectRef.current = true;
        reconnectAttemptRef.current = 0;
        setConnection("open");
        setError(null);
        void loadMachines(true).catch(() => {});
      }
    }).then((off) => {
      unlistenConnection = off;
    });

    return () => {
      unlistenStream?.();
      unlistenConnection?.();
    };
  }, [loadConfig, loadMachines]);

  useEffect(() => {
    workspaceRef.current = workspace;
  }, [workspace]);

  useEffect(() => {
    if (!account || !activeWorkspaceId || connection !== "idle") return;
    autoReconnectRef.current = false;
    hasOpenedConnectionRef.current = false;
    reconnectAttemptRef.current = 0;
    void connectWorkspace(activeWorkspaceId, { automatic: true, quiet: true });
  }, [account, activeWorkspaceId, connectWorkspace, connection]);

  useEffect(() => {
    activeDirectActorIdRef.current = activeDirectActor?.id ?? null;
  }, [activeDirectActor?.id]);

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
    setActiveDirectActorId(null);
    setDirectMessages([]);
    setDirectDraft("");
    setDirectScopesByActorId({});
  }, [workspace?.id]);

  useEffect(() => {
    if (view !== "direct") return;
    setActiveDirectActorId((current) => current ?? agentActors[0]?.id ?? null);
  }, [agentActorIdsKey, view]);

  useEffect(() => {
    if (
      !account ||
      !autoReconnectRef.current ||
      !hasOpenedConnectionRef.current ||
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
      void connectWorkspace(workspace.id, { automatic: true, reconnect: true });
    }, delay);
    reconnectTimerRef.current = timer;

    return () => {
      if (reconnectTimerRef.current === timer) {
        window.clearTimeout(timer);
        reconnectTimerRef.current = null;
      }
    };
  }, [account, connectWorkspace, connection, workspace]);

  useEffect(() => {
    if (connection !== "open") return;

    const refreshMachines = () => {
      void loadMachines(true).catch(() => {});
    };
    refreshMachines();

    const interval = window.setInterval(refreshMachines, machineStatusPollIntervalMs);
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refreshMachines();
    };
    document.addEventListener("visibilitychange", refreshWhenVisible);

    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [connection, loadMachines]);

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
    if (connection !== "open" || visibleChannels.length === 0) return;
    let alive = true;
    void Promise.allSettled(
      visibleChannels.map((channel) =>
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

  useEffect(() => {
    activeDirectScopeRef.current = activeDirectScope;
    if (!activeDirectActor || !activeDirectTarget || connection !== "open") {
      setDirectMessages([]);
      return;
    }
    if (!activeDirectScope) {
      setDirectMessages([]);
      return;
    }

    let alive = true;
    setDirectMessages([]);
    void ipc.scopeSubscribe(activeDirectScope).catch(() => {});
    void ipc
      .messageList({ target: activeDirectTarget, limit: 150 })
      .then((result) => {
        if (!alive) return;
        setError(null);
        setDirectMessages(sortMessages(result.messages.map(normalizeMessage)));
      })
      .catch((err) => setError(errorText(err)));

    return () => {
      alive = false;
      void ipc.scopeUnsubscribe(activeDirectScope).catch(() => {});
    };
  }, [
    activeDirectActor?.id,
    activeDirectScope ? scopeKey(activeDirectScope) : null,
    activeDirectTarget,
    connection,
  ]);

  function handleStream(update: StreamUpdate) {
    switch (update.kind) {
      case "channel.created":
      case "channel.updated":
      case "channel.invited": {
        const channel = update.data.channel as Channel | undefined;
        if (channel) {
          setChannels((current) => sortChannels(upsert(current, channel)));
          const peerActorId = directChannelPeerId(channel, actorIdRef.current);
          if (peerActorId) {
            setDirectScopesByActorId((current) => ({
              ...current,
              [peerActorId]: { kind: "channel", id: channel.id },
            }));
          }
        }
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
              applyMessageToUsageStore(message);
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
        if (
          (update.scope &&
            activeDirectScopeRef.current &&
            sameScope(update.scope, activeDirectScopeRef.current)) ||
          messageBelongsToDirectActor(
            message,
            activeDirectActorIdRef.current,
            actorIdRef.current,
          )
        ) {
          setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
        }
        return;
      }
      case "message.updated": {
        const message = update.data.message as Message | undefined;
        if (!message) return;
              applyMessageToUsageStore(message);
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
        if (
          (update.scope &&
            activeDirectScopeRef.current &&
            sameScope(update.scope, activeDirectScopeRef.current)) ||
          messageBelongsToDirectActor(
            message,
            activeDirectActorIdRef.current,
            actorIdRef.current,
          )
        ) {
          setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
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
    const deletedChannel = channels.find((channel) => channel.id === channelId) ?? null;
    const fallbackChannelId =
      channels.find((channel) => channel.id !== channelId && !isDirectChannel(channel))
        ?.id ?? null;
    setChannels((current) => current.filter((channel) => channel.id !== channelId));
    const deletedDirectPeerId = directChannelPeerId(deletedChannel, actorIdRef.current);
    if (deletedDirectPeerId) {
      setDirectScopesByActorId((current) => {
        const next = { ...current };
        delete next[deletedDirectPeerId];
        return next;
      });
    }
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

  async function setLocalIdentity(args: {
    userId: string;
    nickname: string;
    actorId: string;
  }): Promise<boolean> {
    setBusy("account:set-local");
    setError(null);
    try {
      const result = await ipc.accountSetLocal(args);
      autoReconnectRef.current = false;
      hasOpenedConnectionRef.current = false;
      reconnectAttemptRef.current = 0;
      clearReconnectTimer();
      applyConfig(result.config);
      workspaceRef.current = null;
      actorIdRef.current = null;
      setConnection("idle");
      setChannels([]);
      setActors({});
      setRuns({});
      setInbox([]);
      setTasks([]);
      setMessages([]);
      setThreadMessages([]);
      setDirectMessages([]);
      setDirectDraft("");
      setActiveDirectActorId(null);
      setDirectScopesByActorId({});
      pushNotice(`Signed in as ${accountName(result.account)}`);
      return true;
    } catch (err) {
      setError(errorText(err));
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function logout() {
    setBusy("logout");
    try {
      autoReconnectRef.current = false;
      hasOpenedConnectionRef.current = false;
      reconnectAttemptRef.current = 0;
      clearReconnectTimer();
      applyConfig(await ipc.accountLogout());
      workspaceRef.current = null;
      setWorkspace(null);
      setConnection("idle");
      setChannels([]);
      setActors({});
      setRuns({});
      setInbox([]);
      setTasks([]);
      setMessages([]);
      setThreadMessages([]);
      setDirectMessages([]);
      setDirectDraft("");
      setActiveDirectActorId(null);
      setDirectScopesByActorId({});
      setOnboardingActive(true);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function addWorkspace(): Promise<Workspace | null> {
    const hasTarget = workspaceForm.advanced
      ? workspaceForm.serverUrl.trim()
      : workspaceForm.host.trim();
    if (!workspaceForm.name.trim() || !hasTarget) return null;
    setBusy("workspace:add");
    setError(null);
    try {
      const serverUrl = normalizeWorkspaceFormServerUrl(workspaceForm);
      const next = await ipc.workspaceAdd({
        name: workspaceForm.name.trim(),
        serverUrl,
        activate: true,
      });
      applyConfig(next);
      await loadMachines();
      setWorkspaceForm(defaultWorkspaceForm());
      const saved =
        next.workspaces.find((item) => item.serverUrl === serverUrl) ??
        next.workspaces.find((item) => item.id === next.active) ??
        null;
      if (saved) {
        await connectWorkspace(saved.id, { quiet: true });
        pushNotice(`Connected to ${saved.name}`);
      } else {
        pushNotice(`Server ${workspaceForm.name.trim()} added`);
      }
      return saved;
    } catch (err) {
      setError(errorText(err));
      return null;
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
        try {
          await ipc.disconnect();
        } catch {
          /* local state still closes */
        }
        autoReconnectRef.current = false;
        hasOpenedConnectionRef.current = false;
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
      pushNotice("Registered hosts refreshed");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function createMachine(args: {
    name: string;
    dataRoot?: string;
  }): Promise<MachineInfo | null> {
    const name = args.name.trim();
    if (!name) {
      setError("Host name is required.");
      return null;
    }
    setBusy("machine:create");
    setError(null);
    try {
      const result = await ipc.machineCreate({
        name,
        dataRoot: args.dataRoot?.trim() || undefined,
      });
      applyMachines(result.machines);
      pushNotice(`Host ${name} registration prepared`);
      return (
        result.machines.find(
          (machine) => machine.source === "local_registration" && machine.name === name,
        ) ??
        result.machines.find((machine) => machine.name === name) ??
        null
      );
    } catch (err) {
      setError(errorText(err));
      return null;
    } finally {
      setBusy(null);
    }
  }

  async function startLocalHost(): Promise<boolean> {
    if (connection !== "open" || !workspaceRef.current) {
      setError("Connect to a server before starting a local host.");
      return false;
    }
    const liveHost = machines.find(
      (machine) =>
        machine.source === "server_inventory" && machine.connectionStatus === "online",
    );
    if (liveHost) {
      pushNotice(`${liveHost.name} is already online`);
      return true;
    }

    let machine =
      machines.find((item) => item.source === "local_registration") ?? null;
    if (!machine) {
      machine = await createMachine({ name: "Local Host" });
      if (!machine) return false;
    }

    setBusy("machine:start");
    setError(null);
    try {
      const result = await ipc.machineStart(machine.id);
      applyMachines(result.machines);
      pushNotice(`Local host started (pid ${result.pid})`);
      window.setTimeout(() => {
        void loadMachines(true).catch(() => {});
      }, 1200);
      return true;
    } catch (err) {
      setError(errorText(err));
      return false;
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
      pushNotice("Registered host removed");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function createAgent(form: AgentFormState = agentForm): Promise<boolean> {
    const name = form.name.trim();
    const machine = resolveAgentMachine(form, machines);
    const provider = resolveAgentProvider(form, machine);
    if (!machine) {
      setError("Register a host before creating an agent.");
      return false;
    }
    if (!machineCanCreateAgent(machine)) {
      setError(`Host ${machine.name} is read-only or does not support agent creation.`);
      return false;
    }
    if (!provider) {
      setError(`No agent runtime is available for ${machine.name}.`);
      return false;
    }
    if (!name) {
      setError("Agent name is required.");
      return false;
    }
    setBusy("agent:create");
    setError(null);
    try {
      const result = await ipc.machineAgentCreate({
        machineId: machine.id,
        providerId: provider.id,
        actorId: form.actorId.trim() || undefined,
        name,
        description: form.description.trim(),
        instructions: form.instructions.trim(),
        promptAssembly: defaultAgentPromptAssembly,
        model: form.model.trim() || provider.defaultModel || "",
        autostart: form.autostart,
        env: filterEmptyEnvKeys(form.env),
      });
      applyMachines(result.machines);
      setAgentForm((current) =>
        normalizeAgentForm({ ...current, actorId: "", name: "Echo" }, result.machines),
      );
      if (workspace && connection === "open") {
        await loadWorkspaceData(workspace);
      }
      pushNotice(`Agent ${name} added`);
      return true;
    } catch (err) {
      setError(errorText(err));
      return false;
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
        instructions: patch.instructions.trim(),
        providerId: patch.providerId,
        model: patch.model.trim(),
        reasoningEffort: patch.reasoningEffort.trim(),
        autostart: patch.autostart,
        avatarUrl: patch.avatarUrl.trim(),
        env: filterEmptyEnvKeys(patch.env),
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
    const currentWorkspace = workspaceRef.current ?? workspace;
    if (!title) return;
    if (!currentWorkspace) {
      setError("Add or select a space before creating a channel.");
      return;
    }
    setBusy("channel:create");
    try {
      let channelWorkspace = currentWorkspace;
      if (connection !== "open") {
        const connected = await connectWorkspace(currentWorkspace.id, { quiet: true });
        if (!connected) return;
        channelWorkspace = connected;
      }
      const result = await ipc.channelCreate({
        title,
        actorId: channelWorkspace.actorId,
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
      const mentionableActors = activeChannel
        ? channelMentionActors(activeChannel, actors)
        : [];
      const mentionedAudience = mentionAudience(
        body,
        mentionableActors,
        workspace?.actorId,
      );
      const replyAudience =
        repliedActor && repliedActor.id !== workspace?.actorId
          ? [{ kind: "actor" as const, id: repliedActor.id }]
          : [];
      const directedTo = uniqueAudience([...replyAudience, ...mentionedAudience]);
      const unavailableAgents = activeChannel
        ? directedTo.filter(
            (audience) =>
              audience.kind === "actor" &&
              !isChannelMentionActor(activeChannel, audience.id),
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
      const mentionableActors = activeChannel
        ? channelMentionActors(activeChannel, actors)
        : [];
      const mentionedAudience = mentionAudience(
        body,
        mentionableActors,
        workspace?.actorId,
      );
      const unavailableAgents = activeChannel
        ? mentionedAudience.filter(
            (audience) =>
              audience.kind === "actor" &&
              !isChannelMentionActor(activeChannel, audience.id),
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

  async function sendDirectMessage() {
    const body = directDraft.trim();
    if (!body || !activeDirectActor || !activeDirectTarget) return;
    const directMentions = mentionAudience(
      body,
      Object.values(actors),
      workspace?.actorId,
    );
    if (directMentions.length > 0) {
      setError("Direct messages do not support @ mentions.");
      return;
    }
    setBusy(`direct:message:send:${activeDirectActor.id}`);
    setError(null);
    try {
      const result = await ipc.messageSend({
        target: activeDirectTarget,
        body,
        deliveryPolicy: "wake_agent",
        intent: "request_action",
      });
      const message = normalizeMessage(result.message);
      activeDirectScopeRef.current = message.scope;
      setDirectScopesByActorId((current) => ({
        ...current,
        [activeDirectActor.id]: message.scope,
      }));
      setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
      setDirectDraft("");
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
      const updatedMessage = normalizeMessage(result.message);
      if (
        activeScopeRef.current &&
        sameScope(updatedMessage.scope, activeScopeRef.current)
      ) {
        setMessages((current) => sortMessages(upsertMessage(current, updatedMessage)));
      }
      if (
        activeThreadScopeRef.current &&
        sameScope(updatedMessage.scope, activeThreadScopeRef.current)
      ) {
        setThreadMessages((current) => sortMessages(upsertMessage(current, updatedMessage)));
      }
      if (
        activeDirectScopeRef.current &&
        sameScope(updatedMessage.scope, activeDirectScopeRef.current)
      ) {
        setDirectMessages((current) => sortMessages(upsertMessage(current, updatedMessage)));
      }
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

  async function answerDirectAction(message: Message, optionId: string, accepted: boolean) {
    if (!workspace) return;
    const peerActorId =
      directPeerForMessage(message, workspace.actorId) ?? activeDirectActor?.id ?? null;
    const responseTarget = peerActorId
      ? directMessageTarget(peerActorId)
      : message.target;
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
    view === "direct" ||
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
  const selectWorkspace = async (workspaceId: string): Promise<Workspace | null> => {
    setView("chat");
    setChannelPanelTab(null);
    if (workspace?.id !== workspaceId || connection !== "open") {
      return connectWorkspace(workspaceId);
    }
    return workspaceRef.current ?? workspace ?? null;
  };

  const finishOnboarding = () => {
    setOnboardingActive(false);
    setView("chat");
    if (connection === "open") {
      void loadMachines(true).catch(() => {});
    }
  };

  if (!configLoaded) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-[#f5f6fa] text-[#303849]">
        <div className="flex items-center gap-3 rounded-xl border border-[#dfe3ec] bg-white px-4 py-3 text-sm font-semibold shadow-sm">
          <Loader2 className="animate-spin text-[#5843d7]" size={18} />
          Loading Loom
        </div>
      </div>
    );
  }

  if (onboardingActive) {
    return (
      <OnboardingView
        account={account}
        busy={busy}
        connection={connection}
        error={error}
        machines={machines}
        workspace={workspace}
        workspaceForm={workspaceForm}
        setWorkspaceForm={setWorkspaceForm}
        workspaces={workspaces}
        onSaveIdentity={setLocalIdentity}
        onAddWorkspace={addWorkspace}
        onSelectWorkspace={selectWorkspace}
        onRemoveWorkspace={removeWorkspace}
        onCheckMachines={checkMachines}
        onStartLocalHost={startLocalHost}
        onFinish={finishOnboarding}
      />
    );
  }

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
        onOpenHome={() => setView("chat")}
        onOpenSpaces={() => setView("spaces")}
        onOpenAccount={() => setView("account")}
      />
      {showWorkspaceChrome && (
        <Sidebar
          view={view}
          setView={setView}
          busy={busy}
          channels={visibleChannels}
          channelGroups={channelGroups}
          connection={connection}
          hasWorkspace={Boolean(workspace)}
          workspaceName={workspace?.name ?? null}
          activeChannelId={activeChannelId}
          activeDirectActorId={activeDirectActorId}
          activeThreadId={activeThreadId}
          directAgents={agentActors}
          runs={runs}
          machines={machines}
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
          onSelectDirectAgent={(actorId) => {
            setView("direct");
            setActiveDirectActorId(actorId);
            setActiveThreadId(null);
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
              runs={runs}
              agentActors={agentActors}
              scopeId={activeScope?.id}
              actors={actors}
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
              runs={runs}
              messages={messages}
              tasksBySourceMessageId={tasksBySourceMessageId}
              channelThreads={channelThreads}
              threadStatsById={threadStatsById}
              emptyText={chatEmpty}
              emptyAction={
                connection === "open" ? null : (
                  <NoSpaceConnectionGuide onUseLocalServer={prepareLocalServerSpace} />
                )
              }
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
              channels={visibleChannels}
              messages={messages}
              machines={machines}
              runs={runs}
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
              channels={visibleChannels}
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
        ) : view === "direct" ? (
          <>
            <ErrorBanner error={error} />
            <DirectMessagesView
              actors={actors}
              agents={agentActors}
              busy={busy}
              currentActorId={workspace?.actorId ?? null}
              disabled={connection !== "open" || !activeDirectActor}
              draft={directDraft}
              linkedChannel={activeChannel}
              machines={machines}
              runs={runs}
              messages={directMessages}
              selectedAgent={activeDirectActor}
              setDraft={setDirectDraft}
              onOpenLinkedChannel={(channelId) => {
                setView("chat");
                setActiveChannelId(channelId);
                setActiveThreadId(null);
                setChannelPanelTab(null);
              }}
              onSelectAgent={setActiveDirectActorId}
              onAnswerAction={answerDirectAction}
              onSend={sendDirectMessage}
              onToggleReaction={toggleMessageReaction}
              onOpenAgentSettings={openAgentSettings}
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
                const directPeerId = directPeerForMessage(message, workspace?.actorId ?? null);
                if (directPeerId && actors[directPeerId]?.kind === "agent") {
                  setView("direct");
                  setActiveDirectActorId(directPeerId);
                  setActiveThreadId(null);
                  setChannelPanelTab(null);
                  return;
                }
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
            <TasksView tasks={tasks} channels={visibleChannels} />
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
              onLogout={logout}
            />
          </>
        ) : (
          <>
            <ErrorBanner error={error} />
            <SettingsView
              busy={busy}
              agentForm={agentForm}
              setAgentForm={setAgentForm}
              machines={machines}
              runs={runs}
              targetAgentId={settingsAgentId}
              onConsumeTargetAgent={consumeSettingsAgentTarget}
              onCheckMachines={checkMachines}
              onCreateMachine={createMachine}
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
            mentionAgents={channelAgentActors}
            machines={machines}
            runs={runs}
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
            scopeId={activeThreadScope?.id}
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
            runs={runs}
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
