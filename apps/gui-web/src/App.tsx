import { useCallback, useEffect, useRef, useState } from "react";
import type { ComponentType, ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import {
  Bell,
  Bot,
  Check,
  Circle,
  Github,
  Hash,
  HardDrive,
  Inbox,
  Loader2,
  LogOut,
  MessageSquare,
  PanelRight,
  Plus,
  RefreshCw,
  Send,
  Settings,
  Sparkles,
  Split,
  Trash2,
  User,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import {
  channelTarget,
  scopeKey,
  threadTarget,
  type Actor,
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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { cn, formatTime, shortId } from "@/lib/utils";

type ConnectionState = "idle" | "connecting" | "open" | "closed" | "error";
type View = "chat" | "inbox" | "tasks" | "settings";
type AgentFormState = {
  machineId: string;
  providerId: string;
  actorId: string;
  name: string;
  description: string;
  model: string;
  autostart: boolean;
};

const terminalRunStatuses = new Set(["completed", "failed", "canceled"]);

export function App() {
  const [config, setConfig] = useState<DesktopConfig>({
    workspaces: [],
    account: null,
  });
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("idle");
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("chat");
  const [channels, setChannels] = useState<Channel[]>([]);
  const [threadsByChannel, setThreadsByChannel] = useState<Record<string, Thread[]>>({});
  const [activeChannelId, setActiveChannelId] = useState<string | null>(null);
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [actors, setActors] = useState<Record<string, Actor>>({});
  const [runs, setRuns] = useState<Record<string, Run>>({});
  const [inbox, setInbox] = useState<InboxListEntry[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [machines, setMachines] = useState<MachineInfo[]>([]);
  const [draft, setDraft] = useState("");
  const [newChannelTitle, setNewChannelTitle] = useState("");
  const [workspaceForm, setWorkspaceForm] = useState({
    name: "Local",
    serverUrl: "ws://127.0.0.1:7878/rpc",
  });
  const [machineForm, setMachineForm] = useState({
    name: "Local Daemon",
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
  const [taskDraft, setTaskDraft] = useState<Message | null>(null);

  const activeScopeRef = useRef<ScopeRef | null>(null);
  const actorIdRef = useRef<string | null>(null);
  const targetRef = useRef<string | null>(null);

  const account = config.account ?? null;
  const workspaces = config.workspaces ?? [];
  const activeChannel = channels.find((channel) => channel.id === activeChannelId) ?? null;
  const channelThreads = activeChannel
    ? threadsByChannel[activeChannel.id] ?? []
    : [];
  const activeThread =
    channelThreads.find((thread) => thread.id === activeThreadId) ?? null;
  const target = activeChannel
    ? activeThread
      ? threadTarget(activeThread)
      : channelTarget(activeChannel.id)
    : null;
  const activeScope: ScopeRef | null = activeChannel
    ? activeThread
      ? { kind: "thread", id: activeThread.id }
      : { kind: "channel", id: activeChannel.id }
    : null;
  const openRuns = Object.values(runs).filter(
    (run) => !terminalRunStatuses.has(run.status),
  );
  const actorList = Object.values(actors).sort((a, b) =>
    displayName(a).localeCompare(displayName(b)),
  );
  const agentActors = actorList.filter((actor) => actor.kind === "agent");
  const channelTasks = activeChannel
    ? tasks.filter((task) => task.channelId === activeChannel.id)
    : tasks;

  const applyConfig = useCallback((next: DesktopConfig) => {
    setConfig(next);
    const active = next.active
      ? next.workspaces.find((candidate) => candidate.id === next.active)
      : next.workspaces[0];
    if (active) {
      setWorkspace((current) =>
        current && next.workspaces.some((candidate) => candidate.id === current.id)
          ? current
          : active,
      );
    } else {
      setWorkspace(null);
    }
  }, []);

  const pushNotice = useCallback((text: string) => {
    setNotice(text);
    window.setTimeout(() => setNotice(null), 3200);
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
    async (workspaceId: string) => {
      setBusy(`connect:${workspaceId}`);
      setConnection("connecting");
      setError(null);
      try {
        const result = await ipc.connect(workspaceId);
        setWorkspace(result.workspace);
        setConnection("open");
        await loadWorkspaceData(result.workspace);
        pushNotice(`Connected to ${result.workspace.name}`);
      } catch (err) {
        setConnection("error");
        setError(errorText(err));
      } finally {
        setBusy(null);
      }
    },
    [loadWorkspaceData, pushNotice],
  );

  const disconnect = useCallback(async () => {
    try {
      await ipc.disconnect();
    } catch {
      /* local state still closes */
    }
    setConnection("closed");
  }, []);

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
        setError(event.reason ?? "connection closed");
      } else {
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
    if (!activeChannel || connection !== "open") return;
    let alive = true;
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
  }, [activeChannel?.id, connection]);

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
        setMessages(sortMessages(result.messages));
      })
      .catch((err) => setError(errorText(err)));

    return () => {
      alive = false;
      void ipc.scopeUnsubscribe(activeScope).catch(() => {});
    };
  }, [activeScope ? scopeKey(activeScope) : null, connection, target]);

  function handleStream(update: StreamUpdate) {
    switch (update.kind) {
      case "channel.created":
      case "channel.invited": {
        const channel = update.data.channel as Channel | undefined;
        if (channel) setChannels((current) => sortChannels(upsert(current, channel)));
        return;
      }
      case "channel.revoked": {
        const channelId = update.data.channelId as string | undefined;
        if (!channelId) return;
        setChannels((current) => current.filter((channel) => channel.id !== channelId));
        setThreadsByChannel((current) => {
          const next = { ...current };
          delete next[channelId];
          return next;
        });
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
                    message,
                  },
                  ...current,
                ],
          );
        }
        if (update.scope && activeScopeRef.current && sameScope(update.scope, activeScopeRef.current)) {
          setMessages((current) => sortMessages(upsert(current, message)));
        }
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
      case "delivery.updated": {
        const actorId = actorIdRef.current;
        if (actorId) void refreshInbox(actorId).catch(() => {});
        return;
      }
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
      applyConfig(await ipc.accountLogout());
      setWorkspace(null);
      setConnection("idle");
      setChannels([]);
      setMessages([]);
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
      pushNotice(`Workspace ${workspaceForm.name.trim()} added`);
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
      pushNotice("Daemon status refreshed");
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
      setMachineForm({ name: "Local Daemon", dataRoot: "" });
      pushNotice(`Daemon ${name} added`);
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
      pushNotice("Daemon removed");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function createAgent() {
    const name = agentForm.name.trim();
    if (!agentForm.machineId || !agentForm.providerId || !name) return;
    setBusy("agent:create");
    setError(null);
    try {
      const result = await ipc.machineAgentCreate({
        machineId: agentForm.machineId,
        providerId: agentForm.providerId,
        actorId: agentForm.actorId.trim() || undefined,
        name,
        description: agentForm.description.trim(),
        model: agentForm.model.trim(),
        autostart: agentForm.autostart,
      });
      applyMachines(result.machines);
      setAgentForm((current) =>
        normalizeAgentForm({ ...current, actorId: "", name: "Echo" }, result.machines),
      );
      if (workspace && connection === "open") {
        const actorId = agentForm.actorId.trim();
        if (activeChannel && actorId) {
          await ensureChannelMembers(activeChannel.id, [actorId]).catch(() => {});
        }
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

  async function openLocalPath(path: string) {
    setError(null);
    try {
      await ipc.openLocalPath(path);
    } catch (err) {
      setError(errorText(err));
    }
  }

  async function createChannel() {
    const title = newChannelTitle.trim();
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
      setNewChannelTitle("");
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
      const mentionedAgents = replyTo
        ? []
        : mentionedAgentAudience(body, actors, workspace?.actorId);
      const directedTo =
        repliedActor && repliedActor.id !== workspace?.actorId
          ? [{ kind: "actor" as const, id: repliedActor.id }]
          : mentionedAgents;
      const wakesAgent =
        repliedActor?.kind === "agent" || directedTo.some((audience) => {
          const actor = actors[audience.id];
          return actor?.kind === "agent";
        });
      if (activeChannel && directedTo.length > 0) {
        await ensureChannelMembers(
          activeChannel.id,
          directedTo.map((audience) => audience.id),
        );
      }
      const result = await ipc.messageSend({
        target,
        body,
        parentMessageId,
        audience: directedTo,
        deliveryPolicy: wakesAgent ? "wake_agent" : "notify_only",
        intent: wakesAgent ? "request_action" : "chat",
      });
      setMessages((current) => sortMessages(upsert(current, result.message)));
      setDraft("");
      setReplyTo(null);
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
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function assignTask(message: Message, agentActorId: string): Promise<boolean> {
    if (!workspace || !agentActorId) return false;
    if (!canUseAsTaskSource(message)) {
      setError("Tasks can only be assigned from top-level channel messages.");
      return false;
    }
    const title = threadTitle(message);
    const instruction = message.body.trim() || title;
    setBusy(`task:create:${message.id}`);
    setError(null);
    try {
      await ensureChannelMembers(channelFromMessage(message), [agentActorId]);
      const existingTasks = await ipc.taskList();
      let task = existingTasks.tasks.find(
        (candidate) => candidate.sourceMessageId === message.id,
      );
      if (!task) {
        const created = await ipc.taskCreate({
          sourceMessageId: message.id,
          title,
          description: instruction,
          requesterActorId: workspace.actorId,
          ownerActorId: agentActorId,
          status: "claimed",
        });
        task = created.task;
      }
      await ipc.taskAssignmentCreate({
        taskId: task.id,
        fromActorId: workspace.actorId,
        toActorId: agentActorId,
        assignmentType: "fix",
        instruction,
        contract: {
          kind: "gui.message_assignment",
          sourceMessageId: message.id,
          channelId: channelFromMessage(message),
        },
        idempotencyKey: `gui:${message.id}:${agentActorId}`,
      });
      const taskResult = await ipc.taskList();
      setTasks(sortTasks(taskResult.tasks));
      pushNotice(`Task #${task.number} assigned`);
      return true;
    } catch (err) {
      setError(errorText(err));
      return false;
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

  const chatEmpty =
    connection === "open"
      ? activeChannel
        ? activeThread
          ? "No messages in this thread."
          : "No messages in this channel."
        : "No channels."
      : "No workspace connection.";

  return (
    <div className="grid h-screen w-screen grid-cols-[64px_minmax(280px,320px)_minmax(0,1fr)] overflow-hidden bg-background text-foreground xl:grid-cols-[64px_320px_minmax(0,1fr)_320px]">
      <Rail view={view} setView={setView} inboxCount={inbox.length} connection={connection} />
      <Sidebar
        account={account}
        busy={busy}
        channels={channels}
        connection={connection}
        newChannelTitle={newChannelTitle}
        setNewChannelTitle={setNewChannelTitle}
        activeChannelId={activeChannelId}
        activeThreadId={activeThreadId}
        threadsByChannel={threadsByChannel}
        workspace={workspace}
        workspaces={workspaces}
        onAddChannel={createChannel}
        onConnect={connectWorkspace}
        onDisconnect={disconnect}
        onLogin={login}
        onLogout={logout}
        onSelectChannel={(id) => {
          setView("chat");
          setActiveChannelId(id);
          setActiveThreadId(null);
        }}
        onSelectThread={(thread) => {
          setView("chat");
          setActiveChannelId(thread.channelId);
          setActiveThreadId(thread.id);
        }}
      />
      <main className="flex min-h-0 min-w-0 flex-col border-r border-border bg-background">
        {view === "chat" ? (
          <>
            <ChatHeader
              channel={activeChannel}
              thread={activeThread}
              target={target}
              connection={connection}
              onClearThread={() => setActiveThreadId(null)}
            />
            {error && (
              <div className="border-b border-destructive/40 bg-destructive/10 px-4 py-2 text-sm text-destructive-foreground">
                {error}
              </div>
            )}
            <MessageFeed
              actors={actors}
              agents={agentActors}
              messages={messages}
              emptyText={chatEmpty}
              onReply={setReplyTo}
              onStartThread={startThread}
              onOpenTaskAssign={setTaskDraft}
              onAnswerAction={answerAction}
              activeThread={activeThread}
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
              busy={busy === "message:send"}
            />
          </>
        ) : view === "inbox" ? (
          <InboxView
            actors={actors}
            inbox={inbox}
            onOpen={(message) => {
              if (!message) return;
              setView("chat");
              setActiveChannelId(channelFromMessage(message));
              setActiveThreadId(threadIdForMessage(threadsByChannel, message));
            }}
            onAnswer={answerAction}
            busy={busy}
          />
        ) : view === "tasks" ? (
          <TasksView tasks={tasks} channels={channels} />
        ) : (
          <SettingsView
            account={account}
            busy={busy}
            workspaceForm={workspaceForm}
            setWorkspaceForm={setWorkspaceForm}
            machineForm={machineForm}
            setMachineForm={setMachineForm}
            agentForm={agentForm}
            setAgentForm={setAgentForm}
            machines={machines}
            workspaces={workspaces}
            onAddWorkspace={addWorkspace}
            onRemoveWorkspace={removeWorkspace}
            onCheckMachines={checkMachines}
            onAddMachine={createMachine}
            onRemoveMachine={removeMachine}
            onAddAgent={createAgent}
            onRemoveAgent={removeAgent}
            onOpenLocalPath={openLocalPath}
            onLogin={login}
            onLogout={logout}
          />
        )}
      </main>
      <StatePanel
        actors={actorList}
        channel={activeChannel}
        channelTasks={channelTasks}
        inboxCount={inbox.length}
        openRuns={openRuns}
        thread={activeThread}
        onCancelRun={(runId) => {
          setBusy(`run:${runId}:cancel`);
          void ipc
            .runCancel({ runId, reason: "cancelled from Loom Desktop" })
            .catch((err) => setError(errorText(err)))
            .finally(() => setBusy(null));
        }}
      />
      {notice && (
        <div className="fixed bottom-4 left-1/2 z-50 -translate-x-1/2 rounded-md border border-border bg-popover px-4 py-2 text-sm shadow-soft">
          {notice}
        </div>
      )}
      {taskDraft && (
        <TaskAssignDialog
          agents={agentActors}
          busy={busy === `task:create:${taskDraft.id}`}
          message={taskDraft}
          onCancel={() => setTaskDraft(null)}
          onAssign={async (message, agentActorId) => {
            const assigned = await assignTask(message, agentActorId);
            if (assigned) setTaskDraft(null);
          }}
        />
      )}
    </div>
  );
}

function Rail({
  view,
  setView,
  inboxCount,
  connection,
}: {
  view: View;
  setView: (view: View) => void;
  inboxCount: number;
  connection: ConnectionState;
}) {
  const items = [
    { id: "chat" as const, icon: MessageSquare, label: "Chat" },
    { id: "inbox" as const, icon: Inbox, label: "Inbox" },
    { id: "tasks" as const, icon: Check, label: "Tasks" },
    { id: "settings" as const, icon: Settings, label: "Settings" },
  ];
  return (
    <nav className="flex min-h-0 flex-col items-center border-r border-border bg-card py-3">
      <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-md bg-primary text-primary-foreground">
        <Sparkles size={20} />
      </div>
      <div className="flex flex-1 flex-col gap-2">
        {items.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              title={item.label}
              className={cn(
                "relative flex h-10 w-10 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                view === item.id && "bg-accent text-foreground",
              )}
              onClick={() => setView(item.id)}
            >
              <Icon size={18} />
              {item.id === "inbox" && inboxCount > 0 && (
                <span className="absolute -right-1 -top-1 min-w-5 rounded-full bg-amber-400 px-1 text-center text-[10px] font-bold text-black">
                  {inboxCount}
                </span>
              )}
            </button>
          );
        })}
      </div>
      <Circle
        size={12}
        className={cn(
          connection === "open" && "fill-emerald-400 text-emerald-400",
          connection === "connecting" && "fill-amber-400 text-amber-400",
          connection === "error" && "fill-red-400 text-red-400",
          (connection === "idle" || connection === "closed") &&
            "fill-muted-foreground text-muted-foreground",
        )}
      />
    </nav>
  );
}

function Sidebar({
  account,
  busy,
  channels,
  connection,
  newChannelTitle,
  setNewChannelTitle,
  activeChannelId,
  activeThreadId,
  threadsByChannel,
  workspace,
  workspaces,
  onAddChannel,
  onConnect,
  onDisconnect,
  onLogin,
  onLogout,
  onSelectChannel,
  onSelectThread,
}: {
  account: HumanAccount | null;
  busy: string | null;
  channels: Channel[];
  connection: ConnectionState;
  newChannelTitle: string;
  setNewChannelTitle: (value: string) => void;
  activeChannelId: string | null;
  activeThreadId: string | null;
  threadsByChannel: Record<string, Thread[]>;
  workspace: Workspace | null;
  workspaces: Workspace[];
  onAddChannel: () => void;
  onConnect: (workspaceId: string) => void;
  onDisconnect: () => void;
  onLogin: (provider: ipc.LoginProvider) => void;
  onLogout: () => void;
  onSelectChannel: (channelId: string) => void;
  onSelectThread: (thread: Thread) => void;
}) {
  return (
    <aside className="flex min-h-0 min-w-0 flex-col border-r border-border bg-card">
      <div className="border-b border-border p-4">
        {account ? (
          <div className="flex items-center gap-3">
            <Avatar account={account} />
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium">{accountName(account)}</div>
              <div className="truncate text-xs text-muted-foreground">
                {account.provider}
              </div>
            </div>
            <Button variant="ghost" size="icon" title="Sign out" onClick={onLogout}>
              <LogOut size={16} />
            </Button>
          </div>
        ) : (
          <div className="space-y-3">
            <div>
              <div className="text-sm font-medium">Loom Desktop</div>
              <div className="text-xs text-muted-foreground">Third-party identity</div>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <Button
                variant="outline"
                onClick={() => onLogin("github")}
                disabled={busy === "login:github"}
              >
                <Github size={15} />
                GitHub
              </Button>
              <Button
                variant="outline"
                onClick={() => onLogin("google")}
                disabled={busy === "login:google"}
              >
                <span className="text-sm font-semibold">G</span>
                Google
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className="border-b border-border p-3">
        <div className="mb-2 flex items-center justify-between">
          <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
            Workspaces
          </span>
          {connection === "open" ? (
            <Button variant="ghost" size="sm" onClick={onDisconnect}>
              Disconnect
            </Button>
          ) : null}
        </div>
        <div className="space-y-1">
          {workspaces.map((item) => (
            <button
              key={item.id}
              className={cn(
                "flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-accent",
                workspace?.id === item.id && "bg-accent",
              )}
              onClick={() => onConnect(item.id)}
              disabled={busy === `connect:${item.id}`}
            >
              {busy === `connect:${item.id}` ? (
                <Loader2 className="animate-spin" size={14} />
              ) : (
                <Circle
                  size={10}
                  className={cn(
                    workspace?.id === item.id && connection === "open"
                      ? "fill-emerald-400 text-emerald-400"
                      : "text-muted-foreground",
                  )}
                />
              )}
              <span className="min-w-0 flex-1 truncate">{item.name}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex items-center gap-2 border-b border-border p-3">
          <Input
            value={newChannelTitle}
            onChange={(event) => setNewChannelTitle(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") onAddChannel();
            }}
            placeholder="New channel"
            disabled={connection !== "open"}
          />
          <Button
            variant="secondary"
            size="icon"
            onClick={onAddChannel}
            disabled={connection !== "open" || !newChannelTitle.trim()}
          >
            <Plus size={16} />
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-2 scrollbar-thin">
          {channels.map((channel) => {
            const selected = channel.id === activeChannelId && !activeThreadId;
            const threads = threadsByChannel[channel.id] ?? [];
            return (
              <div key={channel.id} className="mb-1">
                <button
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-accent",
                    selected && "bg-accent text-accent-foreground",
                  )}
                  onClick={() => onSelectChannel(channel.id)}
                >
                  <Hash size={15} />
                  <span className="min-w-0 flex-1 truncate">{channel.title}</span>
                  <Badge variant="outline" className="text-[10px]">
                    {threads.length}
                  </Badge>
                </button>
                {channel.id === activeChannelId && threads.length > 0 && (
                  <div className="ml-4 mt-1 space-y-1 border-l border-border pl-2">
                    {threads.map((thread) => (
                      <button
                        key={thread.id}
                        className={cn(
                          "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-muted-foreground hover:bg-accent hover:text-foreground",
                          activeThreadId === thread.id && "bg-accent text-foreground",
                        )}
                        onClick={() => onSelectThread(thread)}
                      >
                        <Split size={13} />
                        <span className="min-w-0 flex-1 truncate">{thread.title}</span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </aside>
  );
}

function ChatHeader({
  channel,
  thread,
  target,
  connection,
  onClearThread,
}: {
  channel: Channel | null;
  thread: Thread | null;
  target: string | null;
  connection: ConnectionState;
  onClearThread: () => void;
}) {
  return (
    <header className="flex h-16 shrink-0 items-center gap-3 border-b border-border px-5">
      <div className="flex h-10 w-10 items-center justify-center rounded-md bg-secondary">
        {thread ? <Split size={18} /> : <Hash size={18} />}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <h1 className="truncate text-base font-semibold">
            {channel ? channel.title : "Workspace"}
          </h1>
          {thread && (
            <Badge variant="secondary" className="max-w-[45%] truncate">
              Thread: {thread.title}
            </Badge>
          )}
        </div>
        <div className="truncate font-mono text-xs text-muted-foreground">
          {target ?? connectionLabel(connection)}
        </div>
      </div>
      {thread && (
        <Button variant="outline" size="sm" onClick={onClearThread}>
          <PanelRight size={15} />
          Channel
        </Button>
      )}
    </header>
  );
}

function MessageFeed({
  actors,
  agents,
  messages,
  emptyText,
  onReply,
  onStartThread,
  onOpenTaskAssign,
  onAnswerAction,
  activeThread,
  busy,
}: {
  actors: Record<string, Actor>;
  agents: Actor[];
  messages: Message[];
  emptyText: string;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onOpenTaskAssign: (message: Message) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  activeThread: Thread | null;
  busy: string | null;
}) {
  if (messages.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">
        {emptyText}
      </div>
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4 scrollbar-thin">
      <div className="mx-auto flex max-w-4xl flex-col gap-3">
        {messages.map((message) => (
          <MessageRow
            key={message.id}
            actor={actors[message.authorActorId]}
            agents={agents}
            message={message}
            onReply={onReply}
            onStartThread={onStartThread}
            onOpenTaskAssign={onOpenTaskAssign}
            onAnswerAction={onAnswerAction}
            canStartThread={!activeThread && canUseAsThreadRoot(message)}
            canAssignTask={canUseAsTaskSource(message)}
            busy={busy}
          />
        ))}
      </div>
    </div>
  );
}

function MessageRow({
  actor,
  agents,
  message,
  onReply,
  onStartThread,
  onOpenTaskAssign,
  onAnswerAction,
  canStartThread,
  canAssignTask,
  busy,
}: {
  actor?: Actor;
  agents: Actor[];
  message: Message;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onOpenTaskAssign: (message: Message) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  canStartThread: boolean;
  canAssignTask: boolean;
  busy: string | null;
}) {
  const actionRequest = messageKind(message) === "action.request";
  const choices = actionChoices(message);
  return (
    <article
      className={cn(
        "group rounded-md px-3 py-2 transition-colors hover:bg-accent/40",
        actionRequest && "border border-amber-400/40 bg-amber-400/10",
      )}
    >
      <div className="flex items-start gap-3">
        <ActorAvatar actor={actor} fallback={message.authorActorId} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{actor ? displayName(actor) : message.authorActorId}</span>
            <span className="text-xs text-muted-foreground">{formatTime(message.createdAt)}</span>
            <Badge variant={actor?.kind === "agent" ? "success" : "outline"}>
              {actor?.kind ?? message.kind}
            </Badge>
            {message.parentMessageId && (
              <span className="font-mono text-xs text-muted-foreground">
                reply {shortId(message.parentMessageId)}
              </span>
            )}
          </div>
          <div className="prose prose-invert mt-1 max-w-none break-words text-sm leading-6">
            <ReactMarkdown>{message.body || metadataText(message)}</ReactMarkdown>
          </div>
          {actionRequest && (
            <div className="mt-3 flex flex-wrap gap-2">
              {choices.map((choice) => (
                <Button
                  key={choice.id}
                  size="sm"
                  variant={choice.accepted ? "default" : "outline"}
                  disabled={busy?.startsWith(`action:${message.id}:`)}
                  onClick={() => onAnswerAction(message, choice.id, choice.accepted)}
                >
                  {choice.accepted ? <Check size={14} /> : <X size={14} />}
                  {choice.label}
                </Button>
              ))}
            </div>
          )}
          <div className="mt-2 flex flex-wrap gap-2 opacity-0 transition-opacity group-hover:opacity-100">
            <Button variant="ghost" size="sm" onClick={() => onReply(message)}>
              Reply
            </Button>
            {canStartThread && (
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
            {agents.length > 0 && canAssignTask && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onOpenTaskAssign(message)}
                disabled={busy === `task:create:${message.id}`}
              >
                <Check size={14} />
                Assign task
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

function TaskAssignDialog({
  agents,
  busy,
  message,
  onAssign,
  onCancel,
}: {
  agents: Actor[];
  busy: boolean;
  message: Message;
  onAssign: (message: Message, agentActorId: string) => Promise<void> | void;
  onCancel: () => void;
}) {
  const [agentActorId, setAgentActorId] = useState(agents[0]?.id ?? "");
  useEffect(() => {
    if (!agentActorId || !agents.some((agent) => agent.id === agentActorId)) {
      setAgentActorId(agents[0]?.id ?? "");
    }
  }, [agentActorId, agents]);
  const source = threadTitle(message);
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-background/70 px-4 backdrop-blur-sm">
      <section className="w-full max-w-md rounded-md border border-border bg-popover p-4 shadow-soft">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold">Assign Task</h2>
            <div className="mt-1 truncate text-xs text-muted-foreground">
              {shortId(message.id, 12)}
            </div>
          </div>
          <Button variant="ghost" size="icon" onClick={onCancel} disabled={busy}>
            <X size={15} />
          </Button>
        </div>
        <div className="mt-4 space-y-3">
          <label className="block">
            <span className="mb-1 block text-xs font-medium text-muted-foreground">
              Source Message
            </span>
            <div className="rounded-md border border-border bg-background px-3 py-2 text-sm">
              {source}
            </div>
          </label>
          <label className="block">
            <span className="mb-1 block text-xs font-medium text-muted-foreground">
              Agent
            </span>
            <select
              value={agentActorId}
              onChange={(event) => setAgentActorId(event.target.value)}
              className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground"
            >
              {agents.map((agent) => (
                <option key={agent.id} value={agent.id}>
                  {displayName(agent)}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="outline" size="sm" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button
            size="sm"
            onClick={() => onAssign(message, agentActorId)}
            disabled={busy || !agentActorId}
          >
            Assign
          </Button>
        </div>
      </section>
    </div>
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
  busy,
}: {
  draft: string;
  setDraft: (value: string) => void;
  disabled: boolean;
  replyTo: Message | null;
  actorName: string;
  onClearReply: () => void;
  onSend: () => void;
  busy: boolean;
}) {
  return (
    <footer className="border-t border-border p-4">
      <div className="mx-auto max-w-4xl">
        {replyTo && (
          <div className="mb-2 flex items-center gap-2 rounded-md border border-border bg-muted px-3 py-2 text-xs text-muted-foreground">
            <span className="min-w-0 flex-1 truncate">Replying to {actorName}</span>
            <button onClick={onClearReply}>
              <X size={14} />
            </button>
          </div>
        )}
        <div className="flex items-end gap-2">
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
            placeholder={disabled ? "Connect and select a channel" : "Message"}
            className="max-h-48 min-h-16"
          />
          <Button
            size="icon"
            onClick={onSend}
            disabled={disabled || !draft.trim() || busy}
          >
            {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
          </Button>
        </div>
      </div>
    </footer>
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
                  <Badge variant="outline">{task.status}</Badge>
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

function SettingsView({
  account,
  busy,
  workspaceForm,
  setWorkspaceForm,
  machineForm,
  setMachineForm,
  agentForm,
  setAgentForm,
  machines,
  workspaces,
  onAddWorkspace,
  onRemoveWorkspace,
  onCheckMachines,
  onAddMachine,
  onRemoveMachine,
  onAddAgent,
  onRemoveAgent,
  onOpenLocalPath,
  onLogin,
  onLogout,
}: {
  account: HumanAccount | null;
  busy: string | null;
  workspaceForm: { name: string; serverUrl: string };
  setWorkspaceForm: (form: { name: string; serverUrl: string }) => void;
  machineForm: { name: string; dataRoot: string };
  setMachineForm: (form: { name: string; dataRoot: string }) => void;
  agentForm: AgentFormState;
  setAgentForm: (form: AgentFormState) => void;
  machines: MachineInfo[];
  workspaces: Workspace[];
  onAddWorkspace: () => void;
  onRemoveWorkspace: (id: string) => void;
  onCheckMachines: () => void;
  onAddMachine: () => void;
  onRemoveMachine: (machineId: string) => void;
  onAddAgent: () => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
  onOpenLocalPath: (path: string) => void;
  onLogin: (provider: ipc.LoginProvider) => void;
  onLogout: () => void;
}) {
  const selectedMachine =
    machines.find((machine) => machine.id === agentForm.machineId) ?? machines[0];
  const selectedProvider =
    selectedMachine?.providers.find((provider) => provider.id === agentForm.providerId) ??
    selectedMachine?.providers[0];
  const modelChoices = selectedProvider?.modelChoices ?? [];
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Settings" detail="Identity, workspaces, daemons, agents" />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto grid max-w-4xl gap-5">
          <div className="rounded-md border border-border bg-card p-4">
            <div className="mb-3 text-sm font-medium">Account</div>
            {account ? (
              <div className="flex items-center gap-3">
                <Avatar account={account} />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium">{accountName(account)}</div>
                  <div className="truncate text-sm text-muted-foreground">
                    {account.email || account.staffId}
                  </div>
                </div>
                <Button variant="outline" onClick={onLogout} disabled={busy === "logout"}>
                  <LogOut size={15} />
                  Sign out
                </Button>
              </div>
            ) : (
              <div className="flex flex-wrap gap-2">
                <Button onClick={() => onLogin("github")} disabled={busy === "login:github"}>
                  <Github size={15} />
                  GitHub
                </Button>
                <Button variant="outline" onClick={() => onLogin("google")} disabled={busy === "login:google"}>
                  Google
                </Button>
              </div>
            )}
          </div>

          <div className="rounded-md border border-border bg-card p-4">
            <div className="mb-3 text-sm font-medium">Add Workspace</div>
            <div className="grid gap-3 sm:grid-cols-[180px_1fr_auto]">
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
              <Button onClick={onAddWorkspace} disabled={busy === "workspace:add"}>
                <Plus size={15} />
                Add
              </Button>
            </div>
          </div>

          <div className="rounded-md border border-border bg-card p-4">
            <div className="mb-3 text-sm font-medium">Workspaces</div>
            <div className="space-y-2">
              {workspaces.map((workspace) => (
                <div
                  key={workspace.id}
                  className="flex items-center gap-3 rounded-md border border-border px-3 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{workspace.name}</div>
                    <div className="truncate font-mono text-xs text-muted-foreground">
                      {workspace.serverUrl}
                    </div>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => onRemoveWorkspace(workspace.id)}
                    disabled={busy === `workspace:remove:${workspace.id}`}
                  >
                    <Trash2 size={16} />
                  </Button>
                </div>
              ))}
            </div>
          </div>

          <div className="rounded-md border border-border bg-card p-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <div className="text-sm font-medium">Daemons</div>
              <Button
                variant="outline"
                size="sm"
                onClick={onCheckMachines}
                disabled={busy === "machine:check"}
              >
                {busy === "machine:check" ? (
                  <Loader2 className="animate-spin" size={15} />
                ) : (
                  <RefreshCw size={15} />
                )}
                Check
              </Button>
            </div>
            <div className="grid gap-3 sm:grid-cols-[180px_1fr_auto]">
              <Input
                value={machineForm.name}
                onChange={(event) =>
                  setMachineForm({ ...machineForm, name: event.target.value })
                }
                placeholder="Daemon name"
              />
              <Input
                value={machineForm.dataRoot}
                onChange={(event) =>
                  setMachineForm({ ...machineForm, dataRoot: event.target.value })
                }
                placeholder="Data root"
              />
              <Button onClick={onAddMachine} disabled={busy === "machine:create"}>
                <Plus size={15} />
                Add
              </Button>
            </div>
            <div className="mt-4 space-y-3">
              {machines.length === 0 ? (
                <MutedLine>No daemons configured.</MutedLine>
              ) : (
                machines.map((machine) => (
                  <MachineCard
                    key={machine.id}
                    machine={machine}
                    busy={busy}
                    onRemove={onRemoveMachine}
                    onOpenLocalPath={onOpenLocalPath}
                    onRemoveAgent={onRemoveAgent}
                  />
                ))
              )}
            </div>
          </div>

          <div className="rounded-md border border-border bg-card p-4">
            <div className="mb-3 text-sm font-medium">Add Agent</div>
            <div className="grid gap-3 sm:grid-cols-2">
              <select
                value={selectedMachine?.id ?? ""}
                onChange={(event) =>
                  setAgentForm(
                    normalizeAgentForm(
                      { ...agentForm, machineId: event.target.value, model: "" },
                      machines,
                    ),
                  )
                }
                className="h-10 rounded-md border border-input bg-background px-3 text-sm"
              >
                {machines.map((machine) => (
                  <option key={machine.id} value={machine.id}>
                    {machine.name}
                  </option>
                ))}
              </select>
              <select
                value={selectedProvider?.id ?? ""}
                onChange={(event) => {
                  const provider = selectedMachine?.providers.find(
                    (item) => item.id === event.target.value,
                  );
                  setAgentForm({
                    ...agentForm,
                    machineId: selectedMachine?.id ?? agentForm.machineId,
                    providerId: event.target.value,
                    model: provider?.defaultModel ?? "",
                  });
                }}
                className="h-10 rounded-md border border-input bg-background px-3 text-sm"
                disabled={!selectedMachine || selectedMachine.providers.length === 0}
              >
                {(selectedMachine?.providers ?? []).map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.name}
                  </option>
                ))}
              </select>
              <Input
                value={agentForm.name}
                onChange={(event) =>
                  setAgentForm({ ...agentForm, name: event.target.value })
                }
                placeholder="Agent name"
              />
              <Input
                value={agentForm.actorId}
                onChange={(event) =>
                  setAgentForm({ ...agentForm, actorId: event.target.value })
                }
                placeholder="Actor id"
              />
              {modelChoices.length > 0 ? (
                <select
                  value={agentForm.model || selectedProvider?.defaultModel || ""}
                  onChange={(event) =>
                    setAgentForm({ ...agentForm, model: event.target.value })
                  }
                  className="h-10 rounded-md border border-input bg-background px-3 text-sm"
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
                  onChange={(event) =>
                    setAgentForm({ ...agentForm, model: event.target.value })
                  }
                  placeholder="Model"
                />
              )}
              <label className="flex h-10 items-center gap-2 rounded-md border border-border px-3 text-sm">
                <input
                  type="checkbox"
                  checked={agentForm.autostart}
                  onChange={(event) =>
                    setAgentForm({ ...agentForm, autostart: event.target.checked })
                  }
                />
                Autostart
              </label>
              <Textarea
                value={agentForm.description}
                onChange={(event) =>
                  setAgentForm({ ...agentForm, description: event.target.value })
                }
                placeholder="Agent instructions"
                className="sm:col-span-2"
              />
            </div>
            <div className="mt-3 flex justify-end">
              <Button
                onClick={onAddAgent}
                disabled={
                  busy === "agent:create" ||
                  !selectedMachine ||
                  !selectedProvider ||
                  !agentForm.name.trim()
                }
              >
                <Bot size={15} />
                Add Agent
              </Button>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function MachineCard({
  machine,
  busy,
  onRemove,
  onOpenLocalPath,
  onRemoveAgent,
}: {
  machine: MachineInfo;
  busy: string | null;
  onRemove: (machineId: string) => void;
  onOpenLocalPath: (path: string) => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  return (
    <div className="rounded-md border border-border p-3">
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-md bg-secondary">
          <HardDrive size={17} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{machine.name}</span>
            <Badge variant={machine.connectionStatus === "online" ? "success" : "outline"}>
              {machine.connectionStatus}
            </Badge>
            <Badge variant="secondary">{machine.setupStatus}</Badge>
            {machine.readOnly && <Badge variant="warning">read only</Badge>}
          </div>
          <div className="mt-1 truncate font-mono text-xs text-muted-foreground">
            {machine.id}
          </div>
        </div>
        <div className="flex gap-1">
          {machine.canOpenLocalPath && (
            <Button
              variant="ghost"
              size="icon"
              title="Open data root"
              onClick={() => onOpenLocalPath(machine.dataRoot)}
            >
              <HardDrive size={15} />
            </Button>
          )}
          {!machine.readOnly && (
            <Button
              variant="ghost"
              size="icon"
              title="Remove daemon"
              onClick={() => onRemove(machine.id)}
              disabled={busy === `machine:remove:${machine.id}`}
            >
              <Trash2 size={15} />
            </Button>
          )}
        </div>
      </div>
      <div className="mt-3 grid gap-3 md:grid-cols-2">
        <InfoBlock label="Data" value={machine.dataRoot} />
        <InfoBlock label="Command" value={machine.serveCommand} />
      </div>
      <div className="mt-3 flex flex-wrap gap-2">
        {machine.providers.length === 0 ? (
          <Badge variant="warning">no CLI providers</Badge>
        ) : (
          machine.providers.map((provider) => (
            <ProviderBadge key={provider.id} provider={provider} />
          ))
        )}
      </div>
      <div className="mt-3 space-y-2">
        {machine.agents.length === 0 ? (
          <MutedLine>No agents on this daemon.</MutedLine>
        ) : (
          machine.agents.map((agent) => {
            const actor = agent.spec.actor;
            return (
              <div
                key={actor.id}
                className="flex items-center gap-3 rounded-md border border-border px-3 py-2"
              >
                <ActorAvatar actor={actor} fallback={actor.id} small />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium">
                    {actor.displayName || actor.id}
                  </div>
                  <div className="truncate font-mono text-xs text-muted-foreground">
                    {actor.id}
                  </div>
                </div>
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
                  >
                    <Trash2 size={15} />
                  </Button>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

function ProviderBadge({ provider }: { provider: MachineAgentProviderInfo }) {
  return (
    <Badge variant="outline">
      {provider.name}
      {provider.actorCount > 0 ? ` (${provider.actorCount})` : ""}
    </Badge>
  );
}

function InfoBlock({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-md border border-border bg-background p-2">
      <div className="mb-1 text-[11px] font-medium uppercase text-muted-foreground">
        {label}
      </div>
      <div className="break-all font-mono text-xs text-muted-foreground">{value}</div>
    </div>
  );
}

function StatePanel({
  actors,
  channel,
  channelTasks,
  inboxCount,
  openRuns,
  thread,
  onCancelRun,
}: {
  actors: Actor[];
  channel: Channel | null;
  channelTasks: Task[];
  inboxCount: number;
  openRuns: Run[];
  thread: Thread | null;
  onCancelRun: (runId: string) => void;
}) {
  return (
    <aside className="hidden min-h-0 min-w-0 flex-col bg-card xl:flex">
      <div className="border-b border-border p-4">
        <div className="text-sm font-medium">Protocol State</div>
        <div className="mt-1 truncate font-mono text-xs text-muted-foreground">
          {thread ? `thread:${thread.id}` : channel ? `channel:${channel.id}` : "idle"}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-4 scrollbar-thin">
        <PanelBlock title="Runs" count={openRuns.length}>
          {openRuns.length === 0 ? (
            <MutedLine>No active runs.</MutedLine>
          ) : (
            openRuns.map((run) => (
              <div key={run.id} className="rounded-md border border-border p-3">
                <div className="flex items-center gap-2">
                  <Bot size={15} />
                  <span className="min-w-0 flex-1 truncate text-sm">{run.actorId}</span>
                  <Badge variant="success">{run.status}</Badge>
                </div>
                <Button
                  className="mt-3 w-full"
                  variant="outline"
                  size="sm"
                  onClick={() => onCancelRun(run.id)}
                >
                  Cancel
                </Button>
              </div>
            ))
          )}
        </PanelBlock>
        <PanelBlock title="Inbox" count={inboxCount}>
          <MutedLine>{inboxCount} pending deliveries.</MutedLine>
        </PanelBlock>
        <PanelBlock title="Members" count={channel?.members.length ?? actors.length}>
          <div className="space-y-2">
            {actors.slice(0, 12).map((actor) => (
              <div key={actor.id} className="flex items-center gap-2">
                <ActorAvatar actor={actor} fallback={actor.id} small />
                <div className="min-w-0 flex-1 truncate text-sm">{displayName(actor)}</div>
                <Badge variant="outline">{actor.kind}</Badge>
              </div>
            ))}
          </div>
        </PanelBlock>
        <PanelBlock title="Tasks" count={channelTasks.length}>
          <div className="space-y-2">
            {channelTasks.slice(0, 8).map((task) => (
              <div key={task.id} className="rounded-md border border-border p-2">
                <div className="truncate text-sm font-medium">{task.title}</div>
                <div className="mt-1 text-xs text-muted-foreground">{task.status}</div>
              </div>
            ))}
            {channelTasks.length === 0 && <MutedLine>No tasks in this channel.</MutedLine>}
          </div>
        </PanelBlock>
      </div>
    </aside>
  );
}

function PageHeader({ title, detail }: { title: string; detail: string }) {
  return (
    <header className="flex h-16 shrink-0 items-center justify-between border-b border-border px-5">
      <h1 className="text-base font-semibold">{title}</h1>
      <span className="text-sm text-muted-foreground">{detail}</span>
    </header>
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
    <div className="flex min-h-80 flex-col items-center justify-center gap-3 rounded-md border border-dashed border-border text-muted-foreground">
      <Icon size={28} />
      <div className="text-sm">{text}</div>
    </div>
  );
}

function PanelBlock({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: ReactNode;
}) {
  return (
    <section className="mb-5">
      <div className="mb-2 flex items-center justify-between">
        <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          {title}
        </div>
        <Badge variant="outline">{count}</Badge>
      </div>
      {children}
    </section>
  );
}

function MutedLine({ children }: { children: ReactNode }) {
  return <div className="text-sm text-muted-foreground">{children}</div>;
}

function Avatar({ account }: { account: HumanAccount }) {
  if (account.avatarUrl) {
    return (
      <img
        alt=""
        src={account.avatarUrl}
        className="h-10 w-10 rounded-md object-cover"
      />
    );
  }
  return (
    <div className="flex h-10 w-10 items-center justify-center rounded-md bg-secondary">
      <User size={18} />
    </div>
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
  return (
    <div
      className={cn(
        "flex shrink-0 items-center justify-center rounded-md bg-secondary text-secondary-foreground",
        small ? "h-7 w-7" : "h-9 w-9",
      )}
      title={actor?.id ?? fallback}
    >
      {actor?.kind === "agent" ? <Bot size={small ? 14 : 17} /> : <User size={small ? 14 : 17} />}
    </div>
  );
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

function displayName(actor: Actor) {
  return actor.displayName || actor.id;
}

function actorName(actors: Record<string, Actor>, actorId: string) {
  return actors[actorId] ? displayName(actors[actorId]) : actorId;
}

function connectionLabel(connection: ConnectionState) {
  if (connection === "open") return "Connected";
  if (connection === "connecting") return "Connecting";
  if (connection === "error") return "Connection error";
  if (connection === "closed") return "Disconnected";
  return "Idle";
}

function normalizeAgentForm(form: AgentFormState, machines: MachineInfo[]): AgentFormState {
  const machine = machines.find((item) => item.id === form.machineId) ?? machines[0];
  const provider =
    machine?.providers.find((item) => item.id === form.providerId) ??
    machine?.providers[0];
  return {
    ...form,
    machineId: machine?.id ?? "",
    providerId: provider?.id ?? "",
    model: form.model || provider?.defaultModel || "",
  };
}

function mentionedAgentAudience(
  body: string,
  actors: Record<string, Actor>,
  selfActorId?: string,
) {
  return Object.values(actors)
    .filter((actor) => actor.kind === "agent" && actor.id !== selfActorId)
    .filter((actor) => body.includes(`@${actor.id}`))
    .map((actor) => ({ kind: "actor" as const, id: actor.id }));
}

function canUseAsThreadRoot(message: Message) {
  return canUseAsTaskSource(message);
}

function canUseAsTaskSource(message: Message) {
  return (
    message.kind !== "system" &&
    message.scope.kind === "channel" &&
    !message.parentMessageId &&
    !message.threadRootMessageId
  );
}

async function ensureChannelMembers(channelId: string, actorIds: string[]) {
  const uniqueActorIds = [...new Set(actorIds.map((id) => id.trim()).filter(Boolean))];
  await Promise.allSettled(
    uniqueActorIds.map((actorId) => ipc.channelInvite({ channelId, actorId })),
  );
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

function actionChoices(message: Message) {
  const raw = message.metadata?.choices;
  if (Array.isArray(raw)) {
    const parsed = raw
      .map((choice) => {
        if (!choice || typeof choice !== "object") return null;
        const record = choice as Record<string, unknown>;
        const id = String(record.id ?? record.label ?? "");
        if (!id) return null;
        const label = String(record.label ?? id);
        return { id, label, accepted: !/reject|decline|cancel|no/i.test(label) };
      })
      .filter((choice): choice is { id: string; label: string; accepted: boolean } =>
        Boolean(choice),
      );
    if (parsed.length > 0) return parsed;
  }
  return [
    { id: "accepted", label: "Approve", accepted: true },
    { id: "declined", label: "Decline", accepted: false },
  ];
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
