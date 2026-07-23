import { useState } from "react";
import type {
  Channel,
  MachineInfo,
  Message,
  Workspace,
  Actor,
  Run,
  Task,
  Thread,
  ScopeRef,
  InboxListEntry,
  HumanAccount,
} from "@/ipc/types";
import type {
  AgentFormState,
  ChannelGroup,
  ChannelPanelTab,
  ConnectionState,
  ThreadActivityStats,
  View,
  WorkspaceFormState,
} from "@/lib/types";
import { ErrorBanner, NoSpaceConnectionGuide } from "@/components/shared/PageComponents";
import { ChatHeader } from "@/components/layout/ChatHeader";
import { MessageFeed } from "@/components/chat/MessageFeed";
import { Composer } from "@/components/chat/Composer";
import { RemoteFilePanel } from "@/components/chat/RemoteFilePanel";
import { ThreadsView } from "@/components/views/ThreadsView";
import { ChannelsView } from "@/components/views/ChannelsView";
import { DirectMessagesView } from "@/components/views/DirectMessagesView";
import { InboxView } from "@/components/views/InboxView";
import { TasksView } from "@/components/views/TasksView";
import { SpacesView } from "@/components/views/SpacesView";
import { AccountView } from "@/components/views/AccountView";
import { SettingsView } from "@/components/views/SettingsView";
import { actorName } from "@/lib/format-utils";
import { channelFromMessage, threadIdForMessage } from "@/lib/message-utils";
import { directPeerForMessage } from "@/lib/channel-utils";

export interface MainContentProps {
  view: View;
  error: string | null;
  busy: string | null;
  connection: ConnectionState;
  workspace: Workspace | null;
  account: HumanAccount | null;
  // Chat view
  activeChannel: Channel | null;
  target: string | null;
  channelPanelTab: ChannelPanelTab | null;
  activeThread: Thread | null;
  activeThreadTask: Task | null;
  activeScope: ScopeRef | null;
  activeThreadScope: ScopeRef | null;
  messages: Message[];
  threadMessages: Message[];
  threadStatsById: Record<string, ThreadActivityStats>;
  tasksBySourceMessageId: Record<string, Task>;
  channelThreads: Thread[];
  chatEmpty: string;
  draft: string;
  threadDraft: string;
  replyTo: Message | null;
  channelAgentActors: Actor[];
  prepareLocalServerSpace: () => void;
  // Threads view
  visibleChannels: Channel[];
  allThreads: (Thread & { channel: Channel })[];
  activeChannelId: string | null;
  threadMessageTarget: string | null;
  // Direct view
  agentActors: Actor[];
  activeDirectActor: Actor | null;
  activeDirectTarget: string | null;
  directMessages: Message[];
  directDraft: string;
  // Inbox view
  inbox: InboxListEntry[];
  // Tasks view
  tasks: Task[];
  // Spaces view
  workspaceForm: WorkspaceFormState;
  workspaces: Workspace[];
  // Settings view
  agentForm: AgentFormState;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  settingsAgentId: string | null;
  // Shared data
  actors: Record<string, Actor>;
  threadsByChannel: Record<string, Thread[]>;
  channelGroups: ChannelGroup[];
  // Actions
  setView: (view: View) => void;
  setActiveChannelId: (id: string) => void;
  setActiveThreadId: (id: string | null) => void;
  setChannelPanelTab: (tab: ChannelPanelTab | null | ((current: ChannelPanelTab | null) => ChannelPanelTab | null)) => void;
  setActiveDirectActorId: (id: string | null) => void;
  setDraft: (draft: string) => void;
  setThreadDraft: (draft: string) => void;
  setDirectDraft: (draft: string) => void;
  setReplyTo: (replyTo: Message | null) => void;
  setWorkspaceForm: (form: WorkspaceFormState) => void;
  setAgentForm: (updater: AgentFormState | ((current: AgentFormState) => AgentFormState)) => void;
  sendMessage: (attachments?: import("@/lib/attachment-utils").PendingAttachment[]) => Promise<void>;
  sendThreadMessage: (attachments?: import("@/lib/attachment-utils").PendingAttachment[]) => Promise<void>;
  sendDirectMessage: () => Promise<void>;
  startThread: (message: Message) => Promise<void>;
  toggleMessageReaction: (message: Message, emoji: string) => Promise<void>;
  answerAction: (message: Message, optionId: string, accepted: boolean) => Promise<void>;
  answerDirectAction: (message: Message, optionId: string, accepted: boolean) => Promise<void>;
  openAgentSettings: (actorId: string) => void;
  consumeSettingsAgentTarget: () => void;
  createChannelWithTitle: (title: string) => Promise<void>;
  deleteChannel: (channel: Channel) => Promise<void>;
  renameChannel: (channel: Channel, title: string) => Promise<void>;
  addWorkspace: () => Promise<Workspace | null>;
  removeWorkspace: (id: string) => Promise<void>;
  selectWorkspace: (id: string) => Promise<Workspace | null>;
  logout: () => Promise<void>;
  updateAccountAvatar: (url: string) => Promise<void>;
  checkMachines: () => Promise<void>;
  createMachine: (args: { name: string; dataRoot?: string }) => Promise<MachineInfo | null>;
  removeMachine: (id: string) => Promise<void>;
  createAgent: (form?: AgentFormState) => Promise<boolean>;
  updateAgent: (patch: import("@/lib/types").AgentUpdatePatch) => Promise<void>;
  addAgentSkill: (machineId: string, actorId: string, source: string) => Promise<boolean>;
  removeAgent: (machineId: string, actorId: string) => Promise<void>;
  openLocalPath: (path: string) => Promise<void>;
  setError: (error: string | null) => void;
}

export function MainContent(props: MainContentProps) {
  const p = props;
  const [remoteFilePanel, setRemoteFilePanel] = useState<{
    channelId: string;
    target: string;
  } | null>(null);

  if (p.view === "chat") {
    return (
      <>
        <ChatHeader
          channel={p.activeChannel}
          target={p.target}
          connection={p.connection}
          activePanel={p.activeThread ? null : p.channelPanelTab}
          onOpenPanel={(panel) => {
            p.setActiveThreadId(null);
            p.setChannelPanelTab((current) => (current === panel ? null : panel));
          }}
          runs={p.runs}
          agentActors={p.agentActors}
          scopeId={p.activeScope?.id}
          actors={p.actors}
          onOpenFolder={() => {
            const ch = p.activeChannel;
            if (!ch) {
              p.setError("No active channel");
              return;
            }
            const localMachine = p.machines.find((m) => m.canOpenLocalPath);
            if (localMachine?.dataRoot) {
              const sep = localMachine.dataRoot.includes("\\") && !localMachine.dataRoot.includes("/") ? "\\" : "/";
              const path = `${localMachine.dataRoot}${sep}workspaces${sep}channel${sep}${ch.id}${sep}`;
              p.openLocalPath(path);
            } else {
              setRemoteFilePanel({
                channelId: ch.id,
                target: p.target ?? `#${ch.id}`,
              });
            }
          }}
        />
        {p.error && (
          <div className="border-b border-red-200 bg-red-50 px-4 py-2 text-sm font-medium text-red-700">
            {p.error}
          </div>
        )}
        <MessageFeed
          actors={p.actors}
          feedKey={p.target ?? "channel:none"}
          machines={p.machines}
          runs={p.runs}
          messages={p.messages}
          tasksBySourceMessageId={p.tasksBySourceMessageId}
          channelThreads={p.channelThreads}
          threadStatsById={p.threadStatsById}
          emptyText={p.chatEmpty}
          emptyAction={
            p.connection === "open" ? null : (
              <NoSpaceConnectionGuide onUseLocalServer={p.prepareLocalServerSpace} />
            )
          }
          onReply={p.setReplyTo}
          onStartThread={p.startThread}
          onToggleReaction={p.toggleMessageReaction}
          onAnswerAction={p.answerAction}
          onOpenAgentSettings={p.openAgentSettings}
          currentActorId={p.workspace?.actorId ?? null}
          busy={p.busy}
        />
        <Composer
          draft={p.draft}
          setDraft={p.setDraft}
          disabled={p.connection !== "open" || !p.target}
          replyTo={p.replyTo}
          actorName={p.replyTo ? actorName(p.actors, p.replyTo.authorActorId) : ""}
          onClearReply={() => p.setReplyTo(null)}
          onSend={p.sendMessage}
          mentionAgents={p.channelAgentActors}
          busy={p.busy === "message:send"}
        />
        {remoteFilePanel && (
          <RemoteFilePanel
            channelId={remoteFilePanel.channelId}
            target={remoteFilePanel.target}
            onClose={() => setRemoteFilePanel(null)}
          />
        )}
      </>
    );
  }

  if (p.view === "threads") {
    return (
      <>
        <ErrorBanner error={p.error} />
        <ThreadsView
          actors={p.actors}
          channels={p.visibleChannels}
          messages={p.messages}
          machines={p.machines}
          runs={p.runs}
          threadMessages={p.threadMessages}
          threadStatsById={p.threadStatsById}
          threads={p.allThreads}
          activeChannelId={p.activeChannelId}
          activeThread={p.activeThread}
          activeThreadTask={p.activeThreadTask}
          currentActorId={p.workspace?.actorId ?? null}
          threadDraft={p.threadDraft}
          setThreadDraft={p.setThreadDraft}
          onSelectThread={(thread) => {
            p.setActiveChannelId(thread.channelId);
            p.setActiveThreadId(thread.id);
            p.setChannelPanelTab(null);
          }}
          onCloseThread={() => p.setActiveThreadId(null)}
          onSendThreadMessage={p.sendThreadMessage}
          onToggleReaction={p.toggleMessageReaction}
          onOpenAgentSettings={p.openAgentSettings}
          onOpenThreadFolder={() => {
            const thread = p.activeThread;
            if (!thread) return;
            const localMachine = p.machines.find((m) => m.canOpenLocalPath);
            if (localMachine?.dataRoot) {
              const sep = localMachine.dataRoot.includes("\\") && !localMachine.dataRoot.includes("/") ? "\\" : "/";
              const path = `${localMachine.dataRoot}${sep}workspaces${sep}thread${sep}${thread.id}${sep}`;
              p.openLocalPath(path);
            } else {
              setRemoteFilePanel({
                channelId: thread.channelId,
                target: p.threadMessageTarget ?? `#${thread.channelId}:${thread.rootMessageId}`,
              });
            }
          }}
          busy={p.busy}
          disabled={p.connection !== "open" || !p.threadMessageTarget}
        />
        {remoteFilePanel && (
          <RemoteFilePanel
            channelId={remoteFilePanel.channelId}
            target={remoteFilePanel.target}
            onClose={() => setRemoteFilePanel(null)}
          />
        )}
      </>
    );
  }

  if (p.view === "channels") {
    return (
      <>
        <ErrorBanner error={p.error} />
        <ChannelsView
          actors={p.actors}
          busy={p.busy}
          channels={p.visibleChannels}
          channelGroups={p.channelGroups}
          threadsByChannel={p.threadsByChannel}
          activeChannel={p.activeChannel}
          onSelectChannel={(channelId) => {
            p.setActiveChannelId(channelId);
            p.setActiveThreadId(null);
            p.setChannelPanelTab(null);
          }}
          onDeleteChannel={p.deleteChannel}
        />
      </>
    );
  }

  if (p.view === "direct") {
    return (
      <>
        <ErrorBanner error={p.error} />
        <DirectMessagesView
          actors={p.actors}
          agents={p.agentActors}
          busy={p.busy}
          currentActorId={p.workspace?.actorId ?? null}
          disabled={p.connection !== "open" || !p.activeDirectActor}
          draft={p.directDraft}
          linkedChannel={p.activeChannel}
          machines={p.machines}
          runs={p.runs}
          messages={p.directMessages}
          selectedAgent={p.activeDirectActor}
          setDraft={p.setDirectDraft}
          onOpenLinkedChannel={(channelId) => {
            p.setView("chat");
            p.setActiveChannelId(channelId);
            p.setActiveThreadId(null);
            p.setChannelPanelTab(null);
          }}
          onSelectAgent={p.setActiveDirectActorId}
          onAnswerAction={p.answerDirectAction}
          onSend={p.sendDirectMessage}
          onToggleReaction={p.toggleMessageReaction}
          onOpenAgentSettings={p.openAgentSettings}
        />
      </>
    );
  }

  if (p.view === "inbox") {
    return (
      <>
        <ErrorBanner error={p.error} />
        <InboxView
          actors={p.actors}
          inbox={p.inbox}
          onOpen={(message) => {
            if (!message) return;
            const directPeerId = directPeerForMessage(message, p.workspace?.actorId ?? null);
            if (directPeerId && p.actors[directPeerId]?.kind === "agent") {
              p.setView("direct");
              p.setActiveDirectActorId(directPeerId);
              p.setActiveThreadId(null);
              p.setChannelPanelTab(null);
              return;
            }
            p.setView("chat");
            p.setActiveChannelId(channelFromMessage(message));
            p.setActiveThreadId(threadIdForMessage(p.threadsByChannel, message));
            p.setChannelPanelTab(null);
          }}
          onAnswer={p.answerAction}
          busy={p.busy}
        />
      </>
    );
  }

  if (p.view === "tasks") {
    return (
      <>
        <ErrorBanner error={p.error} />
        <TasksView tasks={p.tasks} channels={p.visibleChannels} />
      </>
    );
  }

  if (p.view === "spaces") {
    return (
      <>
        <ErrorBanner error={p.error} />
        <SpacesView
          busy={p.busy}
          connection={p.connection}
          workspace={p.workspace}
          workspaceForm={p.workspaceForm}
          setWorkspaceForm={p.setWorkspaceForm}
          workspaces={p.workspaces}
          onAddWorkspace={p.addWorkspace}
          onRemoveWorkspace={p.removeWorkspace}
          onSelectWorkspace={p.selectWorkspace}
        />
      </>
    );
  }

  if (p.view === "account") {
    return (
      <>
        <ErrorBanner error={p.error} />
        <AccountView
          account={p.account}
          busy={p.busy}
          onLogout={p.logout}
          onAvatarChange={p.updateAccountAvatar}
        />
      </>
    );
  }

  // settings (default)
  return (
    <>
      <ErrorBanner error={p.error} />
      <SettingsView
        actors={p.actors}
        busy={p.busy}
        agentForm={p.agentForm}
        setAgentForm={p.setAgentForm}
        machines={p.machines}
        runs={p.runs}
        targetAgentId={p.settingsAgentId}
        onConsumeTargetAgent={p.consumeSettingsAgentTarget}
        onCheckMachines={p.checkMachines}
        onCreateMachine={p.createMachine}
        onRemoveMachine={p.removeMachine}
        onAddAgent={p.createAgent}
        onUpdateAgent={p.updateAgent}
        onAddAgentSkill={p.addAgentSkill}
        onRemoveAgent={p.removeAgent}
        onOpenLocalPath={p.openLocalPath}
      />
    </>
  );
}
