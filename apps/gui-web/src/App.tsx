import {
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { Loader2 } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import {
  type ChannelMemberConfig,
  type DesktopConfig,
  type MachineInfo,
  type ScopeRef,
  type Workspace
} from "@/ipc/types";

import { cn } from "@/lib/utils";

import type {
  AgentFormState,
} from "@/lib/types";
import {
  detailPanelBreakpoint,
  localServerCommand,
} from "@/lib/constants";
import {
  defaultWorkspaceForm,
} from "@/lib/server-url";
import { defaultWakeSpec } from "@/lib/wake-utils";

import {
  channelMentionAgentActors,
  directPeerForMessage,
  isDirectChannel,
  loadChannelGroups,
  sortChannels,
} from "@/lib/channel-utils";

import {
  channelFromMessage,
  threadIdForMessage,
} from "@/lib/message-utils";

import {
  accountToActor,
  actorName,
  errorText,
  initialViewportWidth,
  normalizeAgentForm,
  savePanelSizes,
  sortTasks,
} from "@/lib/format-utils";

// Zustand stores (P2)
import { useConnectionStore } from "@/store/connectionStore";
import { useUIStore } from "@/store/uiStore";
import { useChannelStore } from "@/store/channelStore";
import { useMessageStore } from "@/store/messageStore";
import { useActorStore } from "@/store/actorStore";
import { useTaskStore } from "@/store/taskStore";

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

import {
  deriveActiveChannel,
  deriveActiveDirectActor,
  deriveActiveDirectScope,
  deriveActiveDirectTarget,
  deriveActiveScope,
  deriveActiveThread,
  deriveActiveThreadScope,
  deriveAgentActors,
  deriveAllThreads,
  deriveActorList,
  deriveChannelGroupsKey,
  deriveChannelTasks,
  deriveChannelThreads,
  deriveMemberCandidates,
  deriveTarget,
  deriveTasksBySourceMessageId,
  deriveThreadMessageTarget,
  deriveVisibleChannels,
} from "@/lib/derived";
import { usePanelResize } from "@/hooks/usePanelResize";
import { useStreamHandler } from "@/hooks/useStreamHandler";
import { useChannelGroups } from "@/hooks/useChannelGroups";
import { useConnectionLifecycle } from "@/hooks/useConnectionLifecycle";
import { useChannelScope } from "@/hooks/useChannelScope";
import { useActions } from "@/hooks/useActions";


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
    wake: defaultWakeSpec(),
  });
  const [configLoaded, setConfigLoaded] = useState(false);
  const [onboardingActive, setOnboardingActive] = useState(false);
  const [channelMemberConfigsByChannel, setChannelMemberConfigsByChannel] = useState<
    Record<string, Record<string, ChannelMemberConfig>>
  >({});

  const activeScopeRef = useRef<ScopeRef | null>(null);
  const activeThreadScopeRef = useRef<ScopeRef | null>(null);
  const activeDirectScopeRef = useRef<ScopeRef | null>(null);
  const activeDirectActorIdRef = useRef<string | null>(null);
  const actorIdRef = useRef<string | null>(null);
  const workspaceRef = useRef<Workspace | null>(null);
  const autoReconnectRef = useRef(false);
  const hasOpenedConnectionRef = useRef(false);
  const reconnectTimerRef = useRef<number | null>(null);
  const reconnectAttemptRef = useRef(0);

  const account = config.account ?? null;
  const workspaces = config.workspaces ?? [];
  const visibleChannels = deriveVisibleChannels(channels);
  const activeChannel = deriveActiveChannel(visibleChannels, activeChannelId);
  const channelGroupsKey = deriveChannelGroupsKey(workspace);
  const channelThreads = deriveChannelThreads(activeChannel, threadsByChannel);
  const activeChannelMemberConfigs = activeChannel
    ? channelMemberConfigsByChannel[activeChannel.id] ?? {}
    : {};
  const activeThread = deriveActiveThread(channelThreads, activeThreadId);
  const target = deriveTarget(activeChannel);
  const threadMessageTarget = deriveThreadMessageTarget(activeThread);
  const activeScope = deriveActiveScope(activeChannel);
  const activeThreadScope = deriveActiveThreadScope(activeThread);
  const allThreads = deriveAllThreads(threadsByChannel, visibleChannels);
  const channelIdsKey = visibleChannels.map((channel) => channel.id).join("|");
  const actorList = deriveActorList(actors);
  const agentActors = deriveAgentActors(actorList);
  const agentActorIdsKey = agentActors.map((actor) => actor.id).join("|");
  const activeDirectActor = deriveActiveDirectActor(agentActors, activeDirectActorId, machines);
  const activeDirectTarget = deriveActiveDirectTarget(activeDirectActor);
  const activeDirectScope = deriveActiveDirectScope(activeDirectActor, workspace, channels, directScopesByActorId);
  const memberCandidates = deriveMemberCandidates(actorList);
  const channelAgentActors = activeChannel
    ? channelMentionAgentActors(activeChannel, actors)
    : [];
  const channelTasks = deriveChannelTasks(activeChannel, tasks);
  const tasksBySourceMessageId = deriveTasksBySourceMessageId(tasks);
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

  const {
    updateChannelGroups,
    addChannelGroup,
    renameChannelGroup,
    removeChannelGroup,
    toggleChannelGroup,
    moveChannelToGroup,
  } = useChannelGroups({
    channelGroups,
    setChannelGroups,
    channelGroupsKey,
  });

  const { handleStream, applyChannelDeleted } = useStreamHandler({
    setChannels,
    setDirectScopesByActorId,
    setThreadsByChannel,
    setMessages,
    setThreadMessages,
    setDirectMessages,
    setThreadStatsById,
    setRuns,
    setTasks,
    setInbox,
    setError,
    activeScopeRef,
    activeThreadScopeRef,
    activeDirectScopeRef,
    activeDirectActorIdRef,
    actorIdRef,
    threadsByChannel,
    channels,
    activeChannelId,
    setChannelPanelTab,
    setReplyTo,
    setActiveChannelId,
    setActiveThreadId,
    updateChannelGroups,
    refreshInbox,
  });

  useConnectionLifecycle({
    loadConfig,
    loadMachines,
    handleStream,
    connectWorkspace,
    setConnection,
    setError,
    connection,
    account,
    workspace,
    workspaceRef,
    autoReconnectRef,
    hasOpenedConnectionRef,
    reconnectTimerRef,
    reconnectAttemptRef,
    clearReconnectTimer,
  });

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

  useChannelScope({
    connection,
    activeChannel,
    visibleChannels,
    channelIdsKey,
    activeScope,
    target,
    activeThreadScope,
    threadMessageTarget,
    activeDirectActor,
    activeDirectScope,
    activeDirectTarget,
    activeScopeRef,
    activeThreadScopeRef,
    activeDirectScopeRef,
    setActors,
    setChannelMemberConfigsByChannel,
    setThreadsByChannel,
    setMessages,
    setThreadMessages,
    setDirectMessages,
    setThreadStatsById,
    setError,
  });

  const {
    setLocalIdentity,
    logout,
    updateAccountAvatar,
    addWorkspace,
    removeWorkspace,
    checkMachines,
    createMachine,
    startLocalHost,
    removeMachine,
    createAgent,
    removeAgent,
    updateAgent,
    addAgentSkill,
    inviteMemberToChannel,
    removeMemberFromChannel,
    saveMemberWorkspace,
    clearMemberWorkspace,
    openLocalPath,
    createChannelWithTitle,
    renameChannel,
    deleteChannel,
    sendMessage,
    sendThreadMessage,
    sendDirectMessage,
    startThread,
    toggleMessageReaction,
    answerAction,
    answerDirectAction,
    selectWorkspace,
    finishOnboarding,
  } = useActions({
    setBusy,
    setError,
    setConnection,
    setChannels,
    setActors,
    setRuns,
    setInbox,
    setTasks,
    setMessages,
    setThreadMessages,
    setDirectMessages,
    setThreadStatsById,
    setDraft,
    setThreadDraft,
    setDirectDraft,
    setReplyTo,
    setActiveChannelId,
    setActiveThreadId,
    setActiveDirectActorId,
    setDirectScopesByActorId,
    setWorkspace,
    setWorkspaceForm,
    setChannelPanelTab,
    setOnboardingActive,
    setAgentForm,
    setChannelMemberConfigsByChannel,
    setThreadsByChannel,
    setView,
    draft,
    threadDraft,
    directDraft,
    replyTo,
    target,
    threadMessageTarget,
    activeDirectTarget,
    activeDirectActor,
    activeChannel,
    channelThreads,
    actors,
    machines,
    workspace,
    connection,
    workspaceForm,
    agentForm,
    workspaceRef,
    autoReconnectRef,
    hasOpenedConnectionRef,
    reconnectAttemptRef,
    activeScopeRef,
    activeThreadScopeRef,
    activeDirectScopeRef,
    actorIdRef,
    applyConfig,
    applyMachines,
    loadMachines,
    loadWorkspaceData,
    connectWorkspace,
    clearReconnectTimer,
    pushNotice,
    applyChannelDeleted,
  });

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

  const {
    shellStyle,
    cleanupPanelResize,
    startPanelResize,
    resizePanelByKeyboard,
  } = usePanelResize({
    panelSizes,
    setPanelSizes,
    viewportWidth,
    detailVisibleInGrid,
    showChatDetail,
    setResizingPanel,
  });

  useEffect(() => {
    return () => {
      cleanupPanelResize();
      document.body.classList.remove("is-resizing-panels");
    };
  }, [cleanupPanelResize]);

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
              onAvatarChange={updateAccountAvatar}
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
              onAddAgentSkill={addAgentSkill}
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
            channelMemberConfigs={activeChannelMemberConfigs}
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
            onSaveMemberWorkspace={saveMemberWorkspace}
            onClearMemberWorkspace={clearMemberWorkspace}
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
