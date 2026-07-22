import { useState } from "react";
import type {
  Channel,
  ChannelMemberConfig,
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
  AgentUpdatePatch,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { Rail } from "@/components/layout/Rail";
import { ResizeHandle } from "@/components/layout/ResizeHandle";
import { Sidebar } from "@/components/layout/Sidebar";
import { ThreadPanel } from "@/components/chat/ThreadPanel";
import { ChannelPanel } from "@/components/panels/ChannelPanels";
import { MainContent } from "@/containers/MainContent";
import { RemoteFilePanel } from "@/components/chat/RemoteFilePanel";

export interface WorkspaceShellProps {
  // Shell state
  shellStyle: React.CSSProperties;
  showWorkspaceChrome: boolean;
  showChatDetail: boolean;
  resizingPanel: "sidebar" | "detail" | null;
  notice: string | null;
  // Rail
  account: HumanAccount | null;
  busy: string | null;
  connection: ConnectionState;
  workspace: Workspace | null;
  workspaces: Workspace[];
  selectWorkspace: (id: string) => Promise<Workspace | null>;
  setView: (view: View) => void;
  // Sidebar
  view: View;
  visibleChannels: Channel[];
  channelGroups: ChannelGroup[];
  activeChannelId: string | null;
  activeDirectActorId: string | null;
  activeThreadId: string | null;
  agentActors: Actor[];
  runs: Record<string, Run>;
  machines: MachineInfo[];
  threadsByChannel: Record<string, Thread[]>;
  createChannelWithTitle: (title: string) => Promise<void>;
  addChannelGroup: (title: string) => void;
  moveChannelToGroup: (channelId: string, groupId: string) => void;
  deleteChannel: (channel: Channel) => Promise<void>;
  renameChannel: (channel: Channel, title: string) => Promise<void>;
  removeChannelGroup: (groupId: string) => void;
  renameChannelGroup: (groupId: string, title: string) => void;
  toggleChannelGroup: (groupId: string) => void;
  setActiveChannelId: (id: string) => void;
  setActiveThreadId: (id: string | null) => void;
  setActiveDirectActorId: (id: string | null) => void;
  setChannelPanelTab: (tab: ChannelPanelTab | null | ((current: ChannelPanelTab | null) => ChannelPanelTab | null)) => void;
  // Resize
  resizePanelByKeyboard: (panel: "sidebar" | "detail", delta: number) => void;
  startPanelResize: (event: React.PointerEvent<HTMLButtonElement>, panel: "sidebar" | "detail") => void;
  // MainContent (pass-through)
  error: string | null;
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
  allThreads: (Thread & { channel: Channel })[];
  threadMessageTarget: string | null;
  activeDirectActor: Actor | null;
  activeDirectTarget: string | null;
  directMessages: Message[];
  directDraft: string;
  inbox: InboxListEntry[];
  tasks: Task[];
  workspaceForm: WorkspaceFormState;
  agentForm: AgentFormState;
  settingsAgentId: string | null;
  actors: Record<string, Actor>;
  // Setters / actions
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
  addWorkspace: () => Promise<Workspace | null>;
  removeWorkspace: (id: string) => Promise<void>;
  logout: () => Promise<void>;
  updateAccountAvatar: (url: string) => Promise<void>;
  checkMachines: () => Promise<void>;
  createMachine: (args: { name: string; dataRoot?: string }) => Promise<MachineInfo | null>;
  removeMachine: (id: string) => Promise<void>;
  createAgent: (form?: AgentFormState) => Promise<boolean>;
  updateAgent: (patch: AgentUpdatePatch) => Promise<void>;
  addAgentSkill: (machineId: string, actorId: string, source: string) => Promise<boolean>;
  removeAgent: (machineId: string, actorId: string) => Promise<void>;
  openLocalPath: (path: string) => Promise<void>;
  setError: (error: string | null) => void;
  // Detail panel
  activeChannel: Channel | null;
  memberCandidates: Actor[];
  activeChannelMemberConfigs: Record<string, ChannelMemberConfig>;
  channelTasks: Task[];
  inviteMemberToChannel: (channelId: string, actorId: string) => Promise<void>;
  removeMemberFromChannel: (channelId: string, actorId: string) => Promise<void>;
  saveMemberWorkspace: (channelId: string, actorId: string, workspaceDir: string) => Promise<boolean>;
  clearMemberWorkspace: (channelId: string, actorId: string) => Promise<boolean>;
}

export function WorkspaceShell(props: WorkspaceShellProps) {
  const p = props;
  const [threadFilePanel, setThreadFilePanel] = useState(false);

  return (
    <div
      className={cn(
        "app-shell h-screen w-screen overflow-hidden bg-[#f5f6fa] text-foreground",
        p.showWorkspaceChrome && "app-shell-workspace",
        p.showChatDetail && "app-shell-detail",
      )}
      style={p.shellStyle}
    >
      <Rail
        account={p.account}
        busy={p.busy}
        connection={p.connection}
        workspace={p.workspace}
        workspaces={p.workspaces}
        onSelectWorkspace={p.selectWorkspace}
        onOpenHome={() => p.setView("chat")}
        onOpenSpaces={() => p.setView("spaces")}
        onOpenAccount={() => p.setView("account")}
      />
      {p.showWorkspaceChrome && (
        <Sidebar
          view={p.view}
          setView={p.setView}
          busy={p.busy}
          channels={p.visibleChannels}
          channelGroups={p.channelGroups}
          connection={p.connection}
          hasWorkspace={Boolean(p.workspace)}
          workspaceName={p.workspace?.name ?? null}
          activeChannelId={p.activeChannelId}
          activeDirectActorId={p.activeDirectActorId}
          activeThreadId={p.activeThreadId}
          directAgents={p.agentActors}
          runs={p.runs}
          machines={p.machines}
          threadsByChannel={p.threadsByChannel}
          onAddChannel={(title) => {
            void p.createChannelWithTitle(title);
          }}
          onAddChannelGroup={p.addChannelGroup}
          onMoveChannelToGroup={p.moveChannelToGroup}
          onDeleteChannel={p.deleteChannel}
          onRenameChannel={p.renameChannel}
          onRemoveChannelGroup={p.removeChannelGroup}
          onRenameChannelGroup={p.renameChannelGroup}
          onSelectChannel={(id) => {
            p.setView("chat");
            p.setActiveChannelId(id);
            p.setActiveThreadId(null);
            p.setChannelPanelTab(null);
          }}
          onSelectThread={(thread) => {
            p.setView("chat");
            p.setActiveChannelId(thread.channelId);
            p.setActiveThreadId(thread.id);
            p.setChannelPanelTab(null);
          }}
          onSelectDirectAgent={(actorId) => {
            p.setView("direct");
            p.setActiveDirectActorId(actorId);
            p.setActiveThreadId(null);
            p.setChannelPanelTab(null);
          }}
          onToggleChannelGroup={p.toggleChannelGroup}
        />
      )}
      {p.showWorkspaceChrome && (
        <ResizeHandle
          active={p.resizingPanel === "sidebar"}
          label="Resize sidebar"
          onKeyboardResize={(delta) => p.resizePanelByKeyboard("sidebar", delta)}
          onPointerDown={(event) => p.startPanelResize(event, "sidebar")}
        />
      )}
      <main className="flex min-h-0 min-w-0 flex-col bg-white">
        <MainContent
          view={p.view}
          error={p.error}
          busy={p.busy}
          connection={p.connection}
          workspace={p.workspace}
          account={p.account}
          activeChannel={p.activeChannel ?? null}
          target={p.target}
          channelPanelTab={p.channelPanelTab}
          activeThread={p.activeThread}
          activeThreadTask={p.activeThreadTask}
          activeScope={p.activeScope}
          activeThreadScope={p.activeThreadScope}
          messages={p.messages}
          threadMessages={p.threadMessages}
          threadStatsById={p.threadStatsById}
          tasksBySourceMessageId={p.tasksBySourceMessageId}
          channelThreads={p.channelThreads}
          chatEmpty={p.chatEmpty}
          draft={p.draft}
          threadDraft={p.threadDraft}
          replyTo={p.replyTo}
          channelAgentActors={p.channelAgentActors}
          prepareLocalServerSpace={p.prepareLocalServerSpace}
          visibleChannels={p.visibleChannels}
          allThreads={p.allThreads}
          activeChannelId={p.activeChannelId}
          threadMessageTarget={p.threadMessageTarget}
          agentActors={p.agentActors}
          activeDirectActor={p.activeDirectActor}
          activeDirectTarget={p.activeDirectTarget}
          directMessages={p.directMessages}
          directDraft={p.directDraft}
          inbox={p.inbox}
          tasks={p.tasks}
          workspaceForm={p.workspaceForm}
          workspaces={p.workspaces}
          agentForm={p.agentForm}
          machines={p.machines}
          runs={p.runs}
          settingsAgentId={p.settingsAgentId}
          actors={p.actors}
          threadsByChannel={p.threadsByChannel}
          channelGroups={p.channelGroups}
          setView={p.setView}
          setActiveChannelId={p.setActiveChannelId}
          setActiveThreadId={p.setActiveThreadId}
          setChannelPanelTab={p.setChannelPanelTab}
          setActiveDirectActorId={p.setActiveDirectActorId}
          setDraft={p.setDraft}
          setThreadDraft={p.setThreadDraft}
          setDirectDraft={p.setDirectDraft}
          setReplyTo={p.setReplyTo}
          setWorkspaceForm={p.setWorkspaceForm}
          setAgentForm={p.setAgentForm}
          sendMessage={p.sendMessage}
          sendThreadMessage={p.sendThreadMessage}
          sendDirectMessage={p.sendDirectMessage}
          startThread={p.startThread}
          toggleMessageReaction={p.toggleMessageReaction}
          answerAction={p.answerAction}
          answerDirectAction={p.answerDirectAction}
          openAgentSettings={p.openAgentSettings}
          consumeSettingsAgentTarget={p.consumeSettingsAgentTarget}
          createChannelWithTitle={p.createChannelWithTitle}
          deleteChannel={p.deleteChannel}
          renameChannel={p.renameChannel}
          addWorkspace={p.addWorkspace}
          removeWorkspace={p.removeWorkspace}
          selectWorkspace={p.selectWorkspace}
          logout={p.logout}
          updateAccountAvatar={p.updateAccountAvatar}
          checkMachines={p.checkMachines}
          createMachine={p.createMachine}
          removeMachine={p.removeMachine}
          createAgent={p.createAgent}
          updateAgent={p.updateAgent}
          addAgentSkill={p.addAgentSkill}
          removeAgent={p.removeAgent}
          openLocalPath={p.openLocalPath}
          setError={p.setError}
        />
      </main>
      {p.showChatDetail && (
        <ResizeHandle
          active={p.resizingPanel === "detail"}
          className="hidden xl:block"
          label="Resize details panel"
          onKeyboardResize={(delta) => p.resizePanelByKeyboard("detail", delta)}
          onPointerDown={(event) => p.startPanelResize(event, "detail")}
        />
      )}
      {p.showChatDetail && (
        p.activeThread ? (
          <ThreadPanel
            actors={p.actors}
            channel={p.activeChannel ?? null}
            channelMessages={p.messages}
            currentActorId={p.workspace?.actorId ?? null}
            disabled={p.connection !== "open" || !p.threadMessageTarget}
            draft={p.threadDraft}
            mentionAgents={p.channelAgentActors}
            machines={p.machines}
            runs={p.runs}
            messages={p.threadMessages}
            setDraft={p.setThreadDraft}
            task={p.activeThreadTask}
            thread={p.activeThread}
            busy={p.busy}
            className="flex min-h-0 min-w-0 flex-col bg-white"
            onClose={() => p.setActiveThreadId(null)}
            onSend={p.sendThreadMessage}
            onToggleReaction={p.toggleMessageReaction}
            onOpenAgentSettings={p.openAgentSettings}
            onOpenFolder={() => setThreadFilePanel(true)}
            scopeId={p.activeThreadScope?.id}
          />
        ) : p.channelPanelTab && p.activeChannel ? (
          <ChannelPanel
            actors={p.actors}
            memberCandidates={p.memberCandidates}
            channel={p.activeChannel}
            channelMessages={p.messages}
            channelMemberConfigs={p.activeChannelMemberConfigs}
            channelTasks={p.channelTasks}
            channelThreads={p.channelThreads}
            currentActorId={p.workspace?.actorId ?? null}
            machines={p.machines}
            runs={p.runs}
            threadStatsById={p.threadStatsById}
            tab={p.channelPanelTab}
            busy={p.busy}
            onClose={() => p.setChannelPanelTab(null)}
            onSelectTab={p.setChannelPanelTab}
            onSelectThread={(thread) => {
              p.setActiveThreadId(thread.id);
              p.setChannelPanelTab(null);
            }}
            onInviteMember={p.inviteMemberToChannel}
            onRemoveMember={p.removeMemberFromChannel}
            onSaveMemberWorkspace={p.saveMemberWorkspace}
            onClearMemberWorkspace={p.clearMemberWorkspace}
          />
        ) : null
      )}
      {p.notice && (
        <div className="fixed bottom-4 left-1/2 z-50 -translate-x-1/2 rounded-md border border-border bg-popover px-4 py-2 text-sm shadow-soft">
          {p.notice}
        </div>
      )}
      {threadFilePanel && p.activeChannel && p.threadMessageTarget && (
        <RemoteFilePanel
          channelId={p.activeChannel.id}
          target={p.threadMessageTarget}
          onClose={() => setThreadFilePanel(false)}
        />
      )}
    </div>
  );
}
