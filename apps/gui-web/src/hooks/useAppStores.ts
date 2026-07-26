import { useConnectionStore } from "@/store/connectionStore";
import { useUIStore } from "@/store/uiStore";
import { useChannelStore } from "@/store/channelStore";
import { useMessageStore } from "@/store/messageStore";
import { useActorStore } from "@/store/actorStore";
import { useTaskStore } from "@/store/taskStore";

export function useAppStores() {
  // Connection store
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

  // UI store
  const view = useUIStore((s) => s.view);
  const setView = useUIStore((s) => s.setView);
  const settingsAgentId = useUIStore((s) => s.settingsAgentId);
  const setSettingsAgentId = useUIStore((s) => s.setSettingsAgentId);
  const channelPanelTab = useUIStore((s) => s.channelPanelTab);
  const setChannelPanelTab = useUIStore((s) => s.setChannelPanelTab);
  const searchPanelOpen = useUIStore((s) => s.searchPanelOpen);
  const setSearchPanelOpen = useUIStore((s) => s.setSearchPanelOpen);
  const selectedRunId = useUIStore((s) => s.selectedRunId);
  const setSelectedRunId = useUIStore((s) => s.setSelectedRunId);
  const panelSizes = useUIStore((s) => s.panelSizes);
  const setPanelSizes = useUIStore((s) => s.setPanelSizes);
  const viewportWidth = useUIStore((s) => s.viewportWidth);
  const setViewportWidth = useUIStore((s) => s.setViewportWidth);
  const resizingPanel = useUIStore((s) => s.resizingPanel);
  const setResizingPanel = useUIStore((s) => s.setResizingPanel);
  const replyTo = useUIStore((s) => s.replyTo);
  const setReplyTo = useUIStore((s) => s.setReplyTo);

  // Channel store
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

  // Message store
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

  // Actor store
  const actors = useActorStore((s) => s.actors);
  const setActors = useActorStore((s) => s.setActors);
  const runs = useActorStore((s) => s.runs);
  const setRuns = useActorStore((s) => s.setRuns);
  const inbox = useActorStore((s) => s.inbox);
  const setInbox = useActorStore((s) => s.setInbox);
  const machines = useActorStore((s) => s.machines);
  const setMachines = useActorStore((s) => s.setMachines);

  // Task store
  const tasks = useTaskStore((s) => s.tasks);
  const setTasks = useTaskStore((s) => s.setTasks);

  return {
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
  };
}
