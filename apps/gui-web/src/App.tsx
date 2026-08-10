import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Loader2 } from "lucide-react";
import {
  type ChannelMemberConfig,
  type MessageContextResult,
  type ScopeRef,
  type Workspace,
} from "@/ipc/types";
import { detailPanelBreakpoint, machineStatusPollIntervalMs } from "@/lib/constants";
import type { AgentFormState } from "@/lib/types";
import { defaultWakeSpec } from "@/lib/wake-utils";
import { useI18n } from "@/lib/i18n";
import { channelMentionAgentActors } from "@/lib/channel-utils";
import { WorkspaceShell } from "@/containers/WorkspaceShell";
import { OnboardingView } from "@/components/views/OnboardingView";
import { WebConnectionView } from "@/components/views/WebConnectionView";
import { ErrorBoundary } from "@/components/shared/ErrorBoundary";
import {
  ServerPasswordDialog,
  type ServerPasswordPrompt,
} from "@/components/shared/ServerPasswordDialog";
import {
  configureWebConnection,
  hasWebConnectionConfig,
  isWebMode,
} from "@/ipc/bridge";
import * as D from "@/lib/derived";
import { usePanelResize } from "@/hooks/usePanelResize";
import { useStreamHandler } from "@/hooks/useStreamHandler";
import { useChannelGroups } from "@/hooks/useChannelGroups";
import { useConnectionLifecycle } from "@/hooks/useConnectionLifecycle";
import { useChannelScope } from "@/hooks/useChannelScope";
import { useActions } from "@/hooks/useActions";
import { useRunsList } from "@/hooks/useRunsList";
import { useWorkspaceConnection } from "@/hooks/useWorkspaceConnection";
import { useAppStores } from "@/hooks/useAppStores";
import { useAppEffects } from "@/hooks/useAppEffects";


export function App() {
  const { t } = useI18n();
  const {
    config, setConfig, workspace, setWorkspace, connection, setConnection,
    error, setError, notice, setNotice, busy, setBusy, workspaceForm, setWorkspaceForm,
    view, setView, settingsAgentId, setSettingsAgentId, channelPanelTab, setChannelPanelTab,
    searchPanelOpen, setSearchPanelOpen, selectedRunId, setSelectedRunId,
    panelSizes, setPanelSizes, viewportWidth, setViewportWidth, resizingPanel, setResizingPanel,
    replyTo, setReplyTo,
    channels, setChannels, channelGroups, setChannelGroups, threadsByChannel, setThreadsByChannel,
    activeChannelId, setActiveChannelId, activeThreadId, setActiveThreadId,
    activeDirectActorId, setActiveDirectActorId, directScopesByActorId, setDirectScopesByActorId,
    messages, setMessages, threadMessages, setThreadMessages, directMessages, setDirectMessages,
    threadStatsById, setThreadStatsById, draft, setDraft, threadDraft, setThreadDraft,
    directDraft, setDirectDraft,
    actors, setActors, runs, setRuns, inbox, setInbox, machines, setMachines,
    tasks, setTasks,
  } = useAppStores();

  // Component-local state
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
  const [messageAnchorId, setMessageAnchorId] = useState<string | null>(null);
  const [serverPasswordPrompt, setServerPasswordPrompt] = useState<ServerPasswordPrompt | null>(null);

  const activeScopeRef = useRef<ScopeRef | null>(null);
  const activeThreadScopeRef = useRef<ScopeRef | null>(null);
  const activeDirectScopeRef = useRef<ScopeRef | null>(null);
  const activeDirectActorIdRef = useRef<string | null>(null);
  const actorIdRef = useRef<string | null>(null);
  const pendingMessageContextRef = useRef<MessageContextResult | null>(null);
  const workspaceRef = useRef<Workspace | null>(null);
  const autoReconnectRef = useRef(false);
  const hasOpenedConnectionRef = useRef(false);
  const reconnectTimerRef = useRef<number | null>(null);
  const reconnectAttemptRef = useRef(0);
  const serverPasswordsRef = useRef(new Map<string, string>());
  const serverPasswordResolverRef = useRef<((password: string | null) => void) | null>(null);

  const requestServerPassword = useCallback((prompt: ServerPasswordPrompt) => {
    serverPasswordResolverRef.current?.(null);
    return new Promise<string | null>((resolve) => {
      serverPasswordResolverRef.current = resolve;
      setServerPasswordPrompt(prompt);
    });
  }, []);
  const submitServerPassword = useCallback((password: string) => {
    const resolve = serverPasswordResolverRef.current;
    serverPasswordResolverRef.current = null;
    setServerPasswordPrompt(null);
    resolve?.(password);
  }, []);
  const cancelServerPassword = useCallback(() => {
    const resolve = serverPasswordResolverRef.current;
    serverPasswordResolverRef.current = null;
    setServerPasswordPrompt(null);
    resolve?.(null);
  }, []);

  useEffect(() => () => serverPasswordResolverRef.current?.(null), []);

  const account = config.account ?? null;
  const workspaces = config.workspaces ?? [];
  const visibleChannels = D.deriveVisibleChannels(channels);
  const activeChannel = D.deriveActiveChannel(visibleChannels, activeChannelId);
  const channelGroupsKey = D.deriveChannelGroupsKey(workspace);
  const channelThreads = D.deriveChannelThreads(activeChannel, threadsByChannel);
  const activeChannelMemberConfigs = activeChannel
    ? channelMemberConfigsByChannel[activeChannel.id] ?? {}
    : {};
  const activeThread = D.deriveActiveThread(channelThreads, activeThreadId);
  const target = D.deriveTarget(activeChannel);
  const threadMessageTarget = D.deriveThreadMessageTarget(activeThread);
  const activeScope = D.deriveActiveScope(activeChannel);
  const activeThreadScope = D.deriveActiveThreadScope(activeThread);
  const allThreads = D.deriveAllThreads(threadsByChannel, visibleChannels);
  const channelIdsKey = visibleChannels.map((channel) => channel.id).join("|");
  const actorList = D.deriveActorList(actors);
  const agentActors = D.deriveAgentActors(actorList);
  const agentActorIdsKey = agentActors.map((actor) => actor.id).join("|");
  const activeDirectActor = D.deriveActiveDirectActor(actorList, activeDirectActorId, machines);
  const activeDirectTarget = D.deriveActiveDirectTarget(activeDirectActor);
  const activeDirectScope = D.deriveActiveDirectScope(activeDirectActor, workspace, channels, directScopesByActorId);
  const memberCandidates = D.deriveMemberCandidates(actorList);
  const channelAgentActors = activeChannel
    ? channelMentionAgentActors(activeChannel, actors)
    : [];
  const channelTasks = D.deriveChannelTasks(activeChannel, tasks);
  const tasksBySourceMessageId = D.deriveTasksBySourceMessageId(tasks);
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

  const {
    applyConfig,
    pushNotice,
    prepareLocalServerSpace,
    clearReconnectTimer,
    refreshInbox,
    refreshActors,
    applyMachines,
    loadMachines,
    loadConfig,
    loadWorkspaceData,
    connectWorkspace,
    connectionGenerationRef,
  } = useWorkspaceConnection({
    config,
    setConfig,
    workspace,
    setWorkspace,
    connection,
    setConnection,
    setError,
    setNotice,
    setBusy,
    setWorkspaceForm,
    setView,
    setChannels,
    setActiveChannelId,
    setActors,
    setTasks,
    setInbox,
    setMachines,
    setAgentForm,
    setOnboardingActive,
    setConfigLoaded,
    account,
    workspaceRef,
    autoReconnectRef,
    hasOpenedConnectionRef,
    reconnectTimerRef,
    reconnectAttemptRef,
    actorIdRef,
    serverPasswordsRef,
    requestServerPassword,
  });

  const {
    removeChannelFromGroupsLocally,
    applyRemoteChannelLayout,
    addChannelGroup,
    renameChannelGroup,
    removeChannelGroup,
    toggleChannelGroup,
    moveChannelToGroup,
  } = useChannelGroups({
    channelGroups,
    setChannelGroups,
    channelGroupsKey,
    connection,
    workspaceId: workspace?.id,
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
    removeChannelFromGroupsLocally,
    applyRemoteChannelLayout,
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
    connectionGenerationRef,
  });

  useEffect(() => {
    activeDirectActorIdRef.current = activeDirectActor?.id ?? null;
  }, [activeDirectActor?.id]);

  // A new search session should not retain the previous result's highlight or
  // context seed. This also makes reopening the same hit scroll to it again.
  useEffect(() => {
    if (!searchPanelOpen) return;
    setMessageAnchorId(null);
    pendingMessageContextRef.current = null;
  }, [searchPanelOpen]);

  useEffect(() => {
    const actorDirectoryVisible =
      view === "settings" ||
      (view === "chat" && channelPanelTab === "members" && Boolean(activeChannel));
    if (connection !== "open" || !actorDirectoryVisible) return;

    const refresh = () => {
      void refreshActors().catch(() => {});
    };
    refresh();
    const interval = window.setInterval(refresh, machineStatusPollIntervalMs);
    return () => window.clearInterval(interval);
  }, [activeChannel?.id, channelPanelTab, connection, refreshActors, view]);

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
    pendingMessageContextRef,
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
    updateChannelVisibility,
    deleteChannel,
    sendMessage,
    sendThreadMessage,
    sendDirectMessage,
    startThread,
    toggleMessageReaction,
    cancelRun,
    openScope,
    openMessageContext,
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
    setMessageAnchorId,
    draft,
    threadDraft,
    directDraft,
    replyTo,
    target,
    threadMessageTarget,
    activeDirectTarget,
    activeDirectActor,
    activeChannel,
    channels,
    channelThreads,
    threadsByChannel,
    visibleChannels,
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
    pendingMessageContextRef,
    applyConfig,
    applyMachines,
    loadMachines,
    loadWorkspaceData,
    connectWorkspace,
    clearReconnectTimer,
    pushNotice,
    applyChannelDeleted,
  });

  const saveWebConnection = useCallback(async (args: {
    serverUrl: string;
    actorId: string;
    displayName: string;
  }) => {
    setBusy("web:connect");
    setError(null);
    try {
      const next = configureWebConnection(args);
      applyConfig(next);
      const workspaceId = next.active ?? next.workspaces[0]?.id;
      if (!workspaceId) throw new Error("Web workspace was not saved");
      const connected = await connectWorkspace(workspaceId, { quiet: true });
      if (connected) {
        finishOnboarding();
        pushNotice(`Connected to ${connected.name}`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(null);
    }
  }, [applyConfig, connectWorkspace, finishOnboarding, pushNotice]);

  const chatEmpty =
    connection === "open"
      ? activeChannel
        ? "No messages in this channel."
        : "No channels."
      : "No space connection.";

  // Runs view: initial list comes from run.list (stream only covers subscribed
  // scopes); sorted openedAt DESC, stable.
  const {
    loading: runsLoading,
    error: runsError,
    refresh: refreshRuns,
  } = useRunsList({
    connection,
    workspaceId: workspace?.id ?? null,
    view,
    setRuns,
  });
  const runsList = useMemo(
    () => Object.values(runs).sort(
      (a, b) => b.openedAt.localeCompare(a.openedAt) || b.id.localeCompare(a.id),
    ),
    [runs],
  );
  const selectedRun = selectedRunId ? runs[selectedRunId] ?? null : null;

  const showWorkspaceChrome =
    view !== "spaces" && view !== "account" && view !== "system";
  const showChatDetail =
    view === "chat" &&
    (Boolean(activeThread) || (Boolean(channelPanelTab) && Boolean(activeChannel)) || searchPanelOpen);
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

  useAppEffects({
    panelSizes,
    setViewportWidth,
    workspaceId: workspace?.id,
    setActiveDirectActorId,
    setDirectMessages,
    setDirectDraft,
    setDirectScopesByActorId,
    view,
    agentActorIdsKey,
    agentActors,
    cleanupPanelResize,
  });

  if (!configLoaded) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-[#f5f6fa] text-[#303849]">
        <div className="flex items-center gap-3 rounded-xl border border-[#dfe3ec] bg-white px-4 py-3 text-sm font-semibold shadow-sm">
          <Loader2 className="animate-spin text-[#5843d7]" size={18} />
          {t("Loading Loom")}
        </div>
      </div>
    );
  }

  if (isWebMode() && !hasWebConnectionConfig()) {
    return (
      <ErrorBoundary>
        <WebConnectionView
          busy={busy}
          error={error}
          onConnect={saveWebConnection}
        />
      </ErrorBoundary>
    );
  }

  if (onboardingActive) {
    return (
      <>
        <ErrorBoundary>
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
        onCreateAgent={createAgent}
        onFinish={finishOnboarding}
          />
        </ErrorBoundary>
        <ServerPasswordDialog
          prompt={serverPasswordPrompt}
          onCancel={cancelServerPassword}
          onSubmit={submitServerPassword}
        />
      </>
    );
  }

  const shellProps = {
    shellStyle, showWorkspaceChrome, showChatDetail, resizingPanel, notice,
    account, busy, connection, workspace, workspaces, selectWorkspace, setView,
    view, visibleChannels, channelGroups, activeChannelId, activeDirectActorId,
    activeThreadId, agentActors, runs, runsList, selectedRun, setSelectedRunId,
    cancelRun, openScope, openMessageContext, messageAnchorId,
    searchPanelOpen, setSearchPanelOpen, machines, threadsByChannel,
    runsLoading, runsError, refreshRuns,
    createChannelWithTitle, addChannelGroup, moveChannelToGroup, deleteChannel,
    renameChannel, updateChannelVisibility, removeChannelGroup, renameChannelGroup, toggleChannelGroup,
    setActiveChannelId, setActiveThreadId, setActiveDirectActorId, setChannelPanelTab,
    resizePanelByKeyboard, startPanelResize, error, target, channelPanelTab,
    activeThread, activeThreadTask, activeScope, activeThreadScope, messages,
    threadMessages, threadStatsById, tasksBySourceMessageId, channelThreads,
    chatEmpty, draft, threadDraft, replyTo, channelAgentActors,
    prepareLocalServerSpace, allThreads, threadMessageTarget, activeDirectActor,
    activeDirectScope, activeDirectTarget, directMessages, directDraft, inbox, tasks, workspaceForm,
    agentForm, settingsAgentId, actors, setDraft, setThreadDraft, setDirectDraft,
    setReplyTo, setWorkspaceForm, setAgentForm, sendMessage, sendThreadMessage,
    sendDirectMessage, startThread, toggleMessageReaction, answerAction,
    answerDirectAction, openAgentSettings, consumeSettingsAgentTarget,
    addWorkspace, removeWorkspace, logout, updateAccountAvatar, checkMachines,
    createMachine, removeMachine, createAgent, updateAgent, addAgentSkill,
    removeAgent, openLocalPath, setError, activeChannel, memberCandidates,
    activeChannelMemberConfigs, channelTasks, inviteMemberToChannel,
    removeMemberFromChannel, saveMemberWorkspace, clearMemberWorkspace,
  };

  return (
    <>
      <ErrorBoundary>
        <WorkspaceShell {...shellProps} />
      </ErrorBoundary>
      <ServerPasswordDialog
        prompt={serverPasswordPrompt}
        onCancel={cancelServerPassword}
        onSubmit={submitServerPassword}
      />
    </>
  );
}
