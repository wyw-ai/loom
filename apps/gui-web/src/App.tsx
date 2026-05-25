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
  Reply,
  Send,
  Settings,
  Sparkles,
  Split,
  Trash2,
  User,
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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { cn, formatTime, shortId } from "@/lib/utils";

type ConnectionState = "idle" | "connecting" | "open" | "closed" | "error";
type View = "chat" | "inbox" | "tasks" | "settings";
type SettingsStep = "workspace" | "daemon" | "agent";
type AgentFormState = {
  machineId: string;
  providerId: string;
  actorId: string;
  name: string;
  description: string;
  model: string;
  autostart: boolean;
};

const quickReactionEmojis = ["👍", "✅", "👀"];

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
  const [, setRuns] = useState<Record<string, Run>>({});
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

  const activeScopeRef = useRef<ScopeRef | null>(null);
  const actorIdRef = useRef<string | null>(null);
  const targetRef = useRef<string | null>(null);
  const workspaceRef = useRef<Workspace | null>(null);
  const autoReconnectRef = useRef(false);
  const reconnectTimerRef = useRef<number | null>(null);
  const reconnectAttemptRef = useRef(0);

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
      case "channel.updated":
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
      case "message.updated": {
        const message = update.data.message as Message | undefined;
        if (!message) return;
        if (update.scope && activeScopeRef.current && sameScope(update.scope, activeScopeRef.current)) {
          setMessages((current) => sortMessages(upsert(current, message)));
        }
        setInbox((current) =>
          current.map((item) =>
            item.delivery.sourceId === message.id ? { ...item, message } : item,
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
    const machine = resolveAgentMachine(agentForm, machines);
    const provider = resolveAgentProvider(agentForm, machine);
    if (!machine) {
      setError("Add a daemon before creating an agent.");
      return;
    }
    if (!machineCanCreateAgent(machine)) {
      setError(`Daemon ${machine.name} is read-only or does not support agent creation.`);
      return;
    }
    if (!provider) {
      setError(`No agent CLI provider is available for ${machine.name}.`);
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

  async function updateChannelTopic(channelId: string, topic: string) {
    const channel = channels.find((item) => item.id === channelId);
    if (!channel) return;
    setBusy(`channel:topic:${channelId}`);
    setError(null);
    try {
      const result = await ipc.channelUpdate({
        channelId,
        title: channel.title,
        topic,
      });
      setChannels((current) => sortChannels(upsert(current, result.channel)));
      pushNotice("Channel topic updated");
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

  async function toggleMessageReaction(message: Message, emoji: string) {
    setBusy(`message:reaction:${message.id}:${emoji}`);
    setError(null);
    try {
      const result = await ipc.messageReactionToggle({
        messageId: message.id,
        emoji,
      });
      setMessages((current) => sortMessages(upsert(current, result.message)));
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

  const chatEmpty =
    connection === "open"
      ? activeChannel
        ? activeThread
          ? "No messages in this thread."
          : "No messages in this channel."
        : "No channels."
      : "No workspace connection.";

  const showChatChrome = view === "chat";

  return (
    <div
      className={cn(
        "grid h-screen w-screen overflow-hidden bg-background text-foreground",
        showChatChrome
          ? "grid-cols-[64px_minmax(280px,320px)_minmax(0,1fr)] xl:grid-cols-[64px_320px_minmax(0,1fr)_320px]"
          : "grid-cols-[64px_minmax(0,1fr)]",
      )}
    >
      <Rail view={view} setView={setView} inboxCount={inbox.length} connection={connection} />
      {showChatChrome && (
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
      )}
      <main
        className={cn(
          "flex min-h-0 min-w-0 flex-col bg-background",
          showChatChrome && "border-r border-border",
        )}
      >
        {view === "chat" ? (
          <>
            <ChatHeader
              channel={activeChannel}
              thread={activeThread}
              task={activeThreadTask}
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
              messages={messages}
              tasksBySourceMessageId={tasksBySourceMessageId}
              channelThreads={channelThreads}
              emptyText={chatEmpty}
              onReply={setReplyTo}
              onStartThread={startThread}
              onToggleReaction={toggleMessageReaction}
              onAnswerAction={answerAction}
              activeThread={activeThread}
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
        ) : (
          <>
            <ErrorBanner error={error} />
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
          </>
        )}
      </main>
      {showChatChrome && (
        <ChannelPanel
          actors={actors}
          memberCandidates={memberCandidates}
          channel={activeChannel}
          channelTasks={channelTasks}
          thread={activeThread}
          busy={busy}
          onInviteMember={inviteMemberToChannel}
          onRemoveMember={removeMemberFromChannel}
          onUpdateTopic={updateChannelTopic}
        />
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
  task,
  target,
  connection,
  onClearThread,
}: {
  channel: Channel | null;
  thread: Thread | null;
  task: Task | null;
  target: string | null;
  connection: ConnectionState;
  onClearThread: () => void;
}) {
  const topic = channelTopic(channel);
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
              {thread.title}
            </Badge>
          )}
          {thread && (task ? <TaskStateBadge task={task} /> : <NoTaskBadge />)}
        </div>
        <div className="truncate text-xs text-muted-foreground">
          {thread
            ? `#${channel?.title ?? "channel"} / ${thread.title}`
            : topic || target || connectionLabel(connection)}
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
  messages,
  tasksBySourceMessageId,
  channelThreads,
  emptyText,
  onReply,
  onStartThread,
  onToggleReaction,
  onAnswerAction,
  activeThread,
  currentActorId,
  busy,
}: {
  actors: Record<string, Actor>;
  messages: Message[];
  tasksBySourceMessageId: Record<string, Task>;
  channelThreads: Thread[];
  emptyText: string;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  activeThread: Thread | null;
  currentActorId: string | null;
  busy: string | null;
}) {
  const workflowSourceIds = new Set(
    messages.filter(isWorkflowMessage).map((message) => message.id),
  );
  const visibleMessages = messages.filter((message) => !isHiddenProtocolMessage(message));
  if (visibleMessages.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">
        {emptyText}
      </div>
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4 scrollbar-thin">
      <div className="mx-auto flex max-w-4xl flex-col gap-3">
        {visibleMessages.map((message) => {
          const threadSummary = activeThread
            ? null
            : channelThreads.find((thread) => thread.rootMessageId === message.id) ?? null;
          const sourceTask =
            !activeThread && message.scope.kind === "channel"
              ? tasksBySourceMessageId[message.id] ?? null
              : null;
          return (
            <MessageRow
              key={message.id}
              actor={actors[message.authorActorId]}
              actors={actors}
              message={message}
              workflowSourceIds={workflowSourceIds}
              onReply={onReply}
              onStartThread={onStartThread}
              onToggleReaction={onToggleReaction}
              onAnswerAction={onAnswerAction}
              canStartThread={!activeThread && canUseAsThreadRoot(message)}
              threadSummary={threadSummary}
              sourceTask={sourceTask}
              currentActorId={currentActorId}
              busy={busy}
            />
          );
        })}
      </div>
    </div>
  );
}

function MessageRow({
  actor,
  actors,
  message,
  workflowSourceIds,
  onReply,
  onStartThread,
  onToggleReaction,
  onAnswerAction,
  canStartThread,
  threadSummary,
  sourceTask,
  currentActorId,
  busy,
}: {
  actor?: Actor;
  actors: Record<string, Actor>;
  message: Message;
  workflowSourceIds: Set<string>;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  canStartThread: boolean;
  threadSummary: Thread | null;
  sourceTask: Task | null;
  currentActorId: string | null;
  busy: string | null;
}) {
  const actionRequest = messageKind(message) === "action.request";
  const choices = actionChoices(message);
  const reactions = message.reactions ?? [];
  if (isWorkflowMessage(message)) {
    return <WorkflowEventRow actor={actor} actors={actors} message={message} />;
  }
  if (isWorkflowResultMessage(message, workflowSourceIds)) {
    return <WorkflowResultRow actor={actor} message={message} />;
  }
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
            {sourceTask && <TaskStateBadge task={sourceTask} />}
            {message.parentMessageId && (
              <span className="font-mono text-xs text-muted-foreground">
                reply {shortId(message.parentMessageId)}
              </span>
            )}
          </div>
          <div className="prose prose-invert mt-1 max-w-none break-words text-sm leading-6">
            <ReactMarkdown>{message.body || metadataText(message)}</ReactMarkdown>
          </div>
          {reactions.length > 0 && (
            <div className="mt-2 flex min-h-7 flex-wrap items-center gap-1.5">
              {reactions.map((reaction) => {
                const selected = Boolean(
                  currentActorId && reaction.actorIds.includes(currentActorId),
                );
                return (
                  <button
                    key={reaction.emoji}
                    type="button"
                    className={cn(
                      "inline-flex h-7 items-center gap-1 rounded-md border px-2 text-xs transition-colors",
                      selected
                        ? "border-primary/60 bg-primary/15 text-primary"
                        : "border-border bg-secondary/60 text-foreground hover:border-primary/40",
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
              <div className="flex gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                {quickReactionEmojis.map((emoji) => (
                  <button
                    key={emoji}
                    type="button"
                    className="inline-flex h-7 w-7 items-center justify-center rounded-md border border-transparent text-sm text-muted-foreground transition-colors hover:border-border hover:bg-secondary hover:text-foreground"
                    title={`React ${emoji}`}
                    disabled={busy === `message:reaction:${message.id}:${emoji}`}
                    onClick={() => onToggleReaction(message, emoji)}
                  >
                    {emoji}
                  </button>
                ))}
              </div>
            </div>
          )}
          {threadSummary && (
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <button
                type="button"
                className="flex w-fit max-w-full items-center gap-2 rounded-md border border-border bg-card px-2.5 py-1.5 text-left text-xs text-muted-foreground transition-colors hover:border-primary/40 hover:text-foreground"
                onClick={() => onStartThread(message)}
              >
                <Split size={14} />
                <span className="font-medium text-foreground">Thread</span>
                <span className="min-w-0 truncate">{threadSummary.title}</span>
              </button>
              {!sourceTask && <NoTaskBadge />}
            </div>
          )}
          {actionRequest && choices.length > 0 && (
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
              <Reply size={14} />
              Reply
            </Button>
            <div className="flex gap-1">
              {quickReactionEmojis.map((emoji) => (
                <button
                  key={emoji}
                  type="button"
                  className="inline-flex h-8 w-8 items-center justify-center rounded-md text-sm text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
                  title={`React ${emoji}`}
                  disabled={busy === `message:reaction:${message.id}:${emoji}`}
                  onClick={() => onToggleReaction(message, emoji)}
                >
                  {emoji}
                </button>
              ))}
            </div>
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

function TaskStateBadge({ task }: { task: Task }) {
  return (
    <Badge variant={taskBadgeVariant(task)} title={task.id}>
      Task #{task.number} · {task.status}
    </Badge>
  );
}

function NoTaskBadge() {
  return (
    <Badge variant="outline" className="text-muted-foreground">
      No task
    </Badge>
  );
}

function taskBadgeVariant(task: Task): "outline" | "success" | "warning" {
  if (task.status === "done") return "success";
  if (task.status === "failed" || task.status === "canceled") return "warning";
  return "outline";
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
    <div className="mx-auto flex max-w-[80%] items-center gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
      <Check size={14} />
      <span className="min-w-0 flex-1 truncate">{summary}</span>
      <span>{formatTime(message.createdAt)}</span>
      {actor && <Badge variant="outline">{displayName(actor)}</Badge>}
    </div>
  );
}

function WorkflowResultRow({
  actor,
  message,
}: {
  actor?: Actor;
  message: Message;
}) {
  return (
    <article className="group rounded-md px-3 py-2 transition-colors hover:bg-accent/40">
      <div className="flex items-start gap-3">
        <ActorAvatar actor={actor} fallback={message.authorActorId} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{actor ? displayName(actor) : message.authorActorId}</span>
            <span className="text-xs text-muted-foreground">{formatTime(message.createdAt)}</span>
            <Badge variant="success">task result</Badge>
          </div>
          <div className="mt-1 text-sm leading-6">{workflowResultSummary(message)}</div>
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
        <div className="relative flex items-end gap-2">
          {showMentions && (
            <div className="absolute bottom-[calc(100%+8px)] left-0 z-20 w-full max-w-xl overflow-hidden rounded-md border border-border bg-popover shadow-soft">
              <div className="border-b border-border px-3 py-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
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
                        ? "bg-accent text-accent-foreground"
                        : "hover:bg-accent/60",
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
  const [step, setStep] = useState<SettingsStep>("workspace");
  const selectedMachine = resolveAgentMachine(agentForm, machines);
  const selectedProvider = resolveAgentProvider(agentForm, selectedMachine);
  const modelChoices = selectedProvider?.modelChoices ?? [];
  const workspaceReady = workspaces.length > 0;
  const daemonReady = machines.length > 0;
  const agentReady = Boolean(
    selectedMachine &&
      selectedProvider &&
      machineCanCreateAgent(selectedMachine) &&
      agentForm.name.trim(),
  );

  useEffect(() => {
    if (!workspaceReady && step !== "workspace") {
      setStep("workspace");
    } else if (step === "agent" && !daemonReady) {
      setStep("daemon");
    }
  }, [daemonReady, step, workspaceReady]);

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Settings" detail="Workspace setup" />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto grid max-w-6xl gap-5 lg:grid-cols-[230px_minmax(0,1fr)]">
          <aside className="space-y-4">
            <div className="rounded-md border border-border bg-card p-4">
              {account ? (
                <div className="flex items-center gap-3">
                  <Avatar account={account} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{accountName(account)}</div>
                    <div className="truncate text-xs text-muted-foreground">
                      {account.email || account.staffId || account.provider}
                    </div>
                  </div>
                  <Button variant="ghost" size="icon" onClick={onLogout} disabled={busy === "logout"}>
                    <LogOut size={15} />
                  </Button>
                </div>
              ) : (
                <div className="space-y-3">
                  <div>
                    <div className="text-sm font-medium">Account</div>
                    <div className="text-xs text-muted-foreground">Optional identity</div>
                  </div>
                  <div className="grid grid-cols-2 gap-2">
                    <Button size="sm" onClick={() => onLogin("github")} disabled={busy === "login:github"}>
                      <Github size={14} />
                      GitHub
                    </Button>
                    <Button size="sm" variant="outline" onClick={() => onLogin("google")} disabled={busy === "login:google"}>
                      Google
                    </Button>
                  </div>
                </div>
              )}
            </div>
            <div className="rounded-md border border-border bg-card p-2">
              <SettingsStepButton
                active={step === "workspace"}
                complete={workspaceReady}
                label="Workspace"
                detail={`${workspaces.length} configured`}
                onClick={() => setStep("workspace")}
              />
              <SettingsStepButton
                active={step === "daemon"}
                complete={daemonReady}
                label="Daemon"
                detail={`${machines.length} available`}
                disabled={!workspaceReady}
                onClick={() => setStep("daemon")}
              />
              <SettingsStepButton
                active={step === "agent"}
                complete={machines.some((machine) => machine.agentCount > 0)}
                label="Agent"
                detail={selectedMachine ? selectedMachine.name : "choose daemon"}
                disabled={!workspaceReady || !daemonReady}
                onClick={() => setStep("agent")}
              />
            </div>
          </aside>

          <div className="min-w-0">
            {step === "workspace" && (
              <div className="space-y-4">
                <SettingsSection
                  title="Workspace"
                  detail="Connect the GUI to a Loom server profile."
                >
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
                </SettingsSection>
                <SettingsSection title="Configured Workspaces" detail="Profiles saved on this Mac.">
                  <div className="space-y-2">
                    {workspaces.length === 0 ? (
                      <MutedLine>No workspaces configured.</MutedLine>
                    ) : (
                      workspaces.map((workspace) => (
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
                          <Badge variant="outline">{workspace.displayName || workspace.actorId}</Badge>
                          <Button
                            variant="ghost"
                            size="icon"
                            title="Remove workspace"
                            onClick={() => onRemoveWorkspace(workspace.id)}
                            disabled={busy === `workspace:remove:${workspace.id}`}
                          >
                            <Trash2 size={16} />
                          </Button>
                        </div>
                      ))
                    )}
                  </div>
                  <div className="mt-4 flex justify-end">
                    <Button onClick={() => setStep("daemon")} disabled={!workspaceReady}>
                      Continue
                    </Button>
                  </div>
                </SettingsSection>
              </div>
            )}

            {step === "daemon" && (
              <div className="space-y-4">
                <SettingsSection
                  title="Daemons"
                  detail="A daemon hosts local or remote agents for the active workspace."
                  action={
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
                  }
                >
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
                </SettingsSection>
                <div className="space-y-3">
                  {machines.length === 0 ? (
                    <EmptyState icon={HardDrive} text="No daemons configured." />
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
                <div className="flex justify-between">
                  <Button variant="outline" onClick={() => setStep("workspace")}>
                    Workspace
                  </Button>
                  <Button onClick={() => setStep("agent")} disabled={!daemonReady}>
                    Add Agent
                  </Button>
                </div>
              </div>
            )}

            {step === "agent" && (
              <div className="space-y-4">
                <SettingsSection
                  title="Add Agent"
                  detail="Pick a daemon first, then choose the runtime provider and identity."
                >
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
                        <option
                          key={machine.id}
                          value={machine.id}
                          disabled={!machineCanCreateAgent(machine)}
                        >
                          {machine.name}
                          {!machineCanCreateAgent(machine) ? " (read-only)" : ""}
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
                      placeholder="Actor id (optional)"
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
                  <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
                    <div className="text-sm text-muted-foreground">
                      {!selectedMachine
                        ? "No daemon selected."
                        : !machineCanCreateAgent(selectedMachine)
                          ? "This daemon is read-only for the current account."
                          : !selectedProvider
                            ? "No CLI provider detected for this daemon."
                            : `${selectedProvider.name} on ${selectedMachine.name}`}
                    </div>
                    <Button
                      onClick={onAddAgent}
                      disabled={busy === "agent:create" || !agentReady}
                    >
                      {busy === "agent:create" ? (
                        <Loader2 className="animate-spin" size={15} />
                      ) : (
                        <Bot size={15} />
                      )}
                      Add Agent
                    </Button>
                  </div>
                </SettingsSection>
                {selectedMachine && (
                  <MachineCard
                    machine={selectedMachine}
                    busy={busy}
                    onRemove={onRemoveMachine}
                    onOpenLocalPath={onOpenLocalPath}
                    onRemoveAgent={onRemoveAgent}
                  />
                )}
                <div className="flex justify-start">
                  <Button variant="outline" onClick={() => setStep("daemon")}>
                    Daemons
                  </Button>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function SettingsStepButton({
  active,
  complete,
  disabled,
  label,
  detail,
  onClick,
}: {
  active: boolean;
  complete: boolean;
  disabled?: boolean;
  label: string;
  detail: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-left transition-colors",
        active ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
        disabled && "cursor-not-allowed opacity-50 hover:bg-transparent hover:text-muted-foreground",
      )}
      onClick={onClick}
      disabled={disabled}
    >
      <span
        className={cn(
          "flex h-6 w-6 shrink-0 items-center justify-center rounded-md border text-xs",
          complete ? "border-emerald-400/60 text-emerald-300" : "border-border",
        )}
      >
        {complete ? <Check size={13} /> : <Circle size={10} />}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium">{label}</span>
        <span className="block truncate text-xs">{detail}</span>
      </span>
    </button>
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
    <section className="rounded-md border border-border bg-card p-4">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-medium">{title}</div>
          <div className="mt-1 text-sm text-muted-foreground">{detail}</div>
        </div>
        {action}
      </div>
      {children}
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

function ChannelPanel({
  actors,
  memberCandidates,
  channel,
  channelTasks,
  thread,
  busy,
  onInviteMember,
  onRemoveMember,
  onUpdateTopic,
}: {
  actors: Record<string, Actor>;
  memberCandidates: Actor[];
  channel: Channel | null;
  channelTasks: Task[];
  thread: Thread | null;
  busy: string | null;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
  onUpdateTopic: (channelId: string, topic: string) => void;
}) {
  const [selectedMemberId, setSelectedMemberId] = useState("");
  const [topicDraft, setTopicDraft] = useState("");
  const members = channel
    ? channel.members.map((actorId) => actors[actorId] ?? fallbackActor(actorId))
    : [];
  const availableMembers = channel
    ? memberCandidates.filter((actor) => canAddChannelMember(channel, actor))
    : [];
  useEffect(() => {
    if (
      !selectedMemberId ||
      !availableMembers.some((actor) => actor.id === selectedMemberId)
    ) {
      setSelectedMemberId(availableMembers[0]?.id ?? "");
    }
  }, [availableMembers, selectedMemberId]);
  useEffect(() => {
    setTopicDraft(channelTopic(channel));
  }, [channel?.id, channel?.topic]);
  const topicChanged = Boolean(channel && topicDraft.trim() !== channelTopic(channel));
  return (
    <aside className="hidden min-h-0 min-w-0 flex-col bg-card xl:flex">
      <div className="border-b border-border p-4">
        <div className="text-sm font-medium">{channel?.title ?? "Channel"}</div>
        <div className="mt-1 truncate font-mono text-xs text-muted-foreground">
          {thread ? `Thread: ${thread.title}` : channel ? channel.visibility : "Not connected"}
        </div>
        {channel && !thread && (
          <div className="mt-4 space-y-2">
            <label className="text-[11px] font-medium uppercase text-muted-foreground">
              Topic
            </label>
            <Textarea
              value={topicDraft}
              onChange={(event) => setTopicDraft(event.target.value)}
              placeholder="Set a channel topic"
              className="min-h-16 resize-none text-sm"
            />
            <div className="flex justify-end">
              <Button
                size="sm"
                variant="outline"
                disabled={!topicChanged || busy === `channel:topic:${channel.id}`}
                onClick={() => onUpdateTopic(channel.id, topicDraft.trim())}
              >
                Save
              </Button>
            </div>
          </div>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-4 scrollbar-thin">
        <PanelBlock title="Members" count={members.length}>
          <div className="space-y-2">
            {members.map((actor) => (
              <div key={actor.id} className="flex items-center gap-2">
                <ActorAvatar actor={actor} fallback={actor.id} small />
                <div className="min-w-0 flex-1 truncate text-sm">{displayName(actor)}</div>
                <Badge variant="outline">{actor.kind}</Badge>
                {channel && canRemoveChannelMember(channel, actor.id) && (
                  <Button
                    size="icon"
                    variant="ghost"
                    title={`Remove ${displayName(actor)}`}
                    disabled={busy === `channel:revoke:${channel.id}:${actor.id}`}
                    onClick={() => onRemoveMember(channel.id, actor.id)}
                    className="h-7 w-7"
                  >
                    {busy === `channel:revoke:${channel.id}:${actor.id}` ? (
                      <Loader2 className="animate-spin" size={13} />
                    ) : (
                      <X size={13} />
                    )}
                  </Button>
                )}
              </div>
            ))}
            {channel && members.length === 0 && (
              <MutedLine>No explicit members.</MutedLine>
            )}
            {channel?.visibility === "public" && (
              <MutedLine>Public channel; explicit members are managed here.</MutedLine>
            )}
            {channel && (
              <div className="rounded-md border border-border p-2">
                <div className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
                  <Users size={13} />
                  Add member
                </div>
                <div className="flex gap-2">
                  <select
                    value={selectedMemberId}
                    onChange={(event) => setSelectedMemberId(event.target.value)}
                    disabled={availableMembers.length === 0}
                    className="h-8 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-xs text-foreground disabled:opacity-60"
                  >
                    {availableMembers.length === 0 ? (
                      <option value="">No candidates</option>
                    ) : (
                      availableMembers.map((actor) => (
                        <option key={actor.id} value={actor.id}>
                          {displayName(actor)} - {actor.kind}
                        </option>
                      ))
                    )}
                  </select>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={
                      !selectedMemberId ||
                      busy === `channel:invite:${channel.id}:${selectedMemberId}`
                    }
                    onClick={() => onInviteMember(channel.id, selectedMemberId)}
                  >
                    {busy === `channel:invite:${channel.id}:${selectedMemberId}` ? (
                      <Loader2 className="animate-spin" size={14} />
                    ) : (
                      <Plus size={14} />
                    )}
                    Add
                  </Button>
                </div>
              </div>
            )}
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

function ErrorBanner({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <div className="border-b border-destructive/40 bg-destructive/10 px-4 py-2 text-sm text-destructive-foreground">
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

function resolveAgentMachine(
  form: AgentFormState,
  machines: MachineInfo[],
): MachineInfo | undefined {
  const current = machines.find((item) => item.id === form.machineId);
  if (current && machineCanCreateAgent(current)) return current;
  return (
    machines.find((machine) => machineCanCreateAgent(machine) && machine.providers.length > 0) ??
    machines.find(machineCanCreateAgent) ??
    current ??
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
  return [];
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
