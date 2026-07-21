import { useCallback, useRef } from "react";
import * as ipc from "@/ipc/bridge";
import type {
  Channel,
  ChannelMemberConfig,
  MachineInfo,
  Message,
  Workspace,
} from "@/ipc/types";
import type { AgentFormState, AgentUpdatePatch } from "@/lib/types";
import {
  accountName,
  accountToActor,
  actorName,
  audienceWakesAgent,
  displayName,
  filterEmptyEnvKeys,
  machineCanCreateAgent,
  machineCanRunCommands,
  mentionAudience,
  normalizeAgentForm,
  resolveAgentMachine,
  resolveAgentProvider,
  uniqueAudience,
} from "@/lib/format-utils";
import {
  channelMentionActors,
  directMessageTarget,
  directPeerForMessage,
  isChannelMentionActor,
  sameScope,
  sortChannels,
} from "@/lib/channel-utils";
import {
  emptyThreadStats,
  normalizeMessage,
  sortMessages,
  threadTitle,
  upsertMessage,
  upsertThreadStatsMessage,
} from "@/lib/message-utils";
import { defaultAgentPromptAssembly } from "@/lib/constants";
import {
  defaultWorkspaceForm,
  normalizeWorkspaceFormServerUrl,
} from "@/lib/server-url";
import { sortThreads, upsert } from "@/lib/format-utils";
import type { ScopeRef } from "@/ipc/types";
import {
  shouldConvertLongText,
  type PendingAttachment,
} from "@/lib/attachment-utils";

export interface ActionDeps {
  // State setters
  setBusy: (busy: string | null) => void;
  setError: (err: string | null) => void;
  setConnection: (conn: import("@/lib/types").ConnectionState) => void;
  setChannels: (updater: import("@/ipc/types").Channel[] | ((prev: import("@/ipc/types").Channel[]) => import("@/ipc/types").Channel[])) => void;
  setActors: (updater: Record<string, import("@/ipc/types").Actor> | ((current: Record<string, import("@/ipc/types").Actor>) => Record<string, import("@/ipc/types").Actor>)) => void;
  setRuns: (updater: Record<string, import("@/ipc/types").Run> | ((current: Record<string, import("@/ipc/types").Run>) => Record<string, import("@/ipc/types").Run>)) => void;
  setInbox: (updater: import("@/ipc/types").InboxListEntry[] | ((prev: import("@/ipc/types").InboxListEntry[]) => import("@/ipc/types").InboxListEntry[])) => void;
  setTasks: (updater: import("@/ipc/types").Task[] | ((prev: import("@/ipc/types").Task[]) => import("@/ipc/types").Task[])) => void;
  setMessages: (updater: Message[] | ((prev: Message[]) => Message[])) => void;
  setThreadMessages: (updater: Message[] | ((prev: Message[]) => Message[])) => void;
  setDirectMessages: (updater: Message[] | ((prev: Message[]) => Message[])) => void;
  setThreadStatsById: (updater: (current: Record<string, import("@/lib/types").ThreadActivityStats>) => Record<string, import("@/lib/types").ThreadActivityStats>) => void;
  setDraft: (draft: string) => void;
  setThreadDraft: (draft: string) => void;
  setDirectDraft: (draft: string) => void;
  setReplyTo: (replyTo: Message | null) => void;
  setActiveChannelId: (id: string) => void;
  setActiveThreadId: (id: string | null) => void;
  setActiveDirectActorId: (updater: string | null | ((prev: string | null) => string | null)) => void;
  setDirectScopesByActorId: (updater: Record<string, ScopeRef> | ((current: Record<string, ScopeRef>) => Record<string, ScopeRef>)) => void;
  setWorkspace: (ws: Workspace | null) => void;
  setWorkspaceForm: (form: import("@/lib/types").WorkspaceFormState) => void;
  setChannelPanelTab: (tab: import("@/lib/types").ChannelPanelTab | null) => void;
  setOnboardingActive: (active: boolean) => void;
  setAgentForm: (updater: (current: AgentFormState) => AgentFormState) => void;
  setChannelMemberConfigsByChannel: React.Dispatch<React.SetStateAction<Record<string, Record<string, ChannelMemberConfig>>>>;
  setThreadsByChannel: (updater: (current: Record<string, import("@/ipc/types").Thread[]>) => Record<string, import("@/ipc/types").Thread[]>) => void;
  setView: (view: import("@/lib/types").View) => void;

  // State values
  draft: string;
  threadDraft: string;
  directDraft: string;
  replyTo: Message | null;
  target: string | null;
  threadMessageTarget: string | null;
  activeDirectTarget: string | null;
  activeDirectActor: { id: string } | null;
  activeChannel: Channel | null;
  channelThreads: import("@/ipc/types").Thread[];
  actors: Record<string, import("@/ipc/types").Actor>;
  machines: MachineInfo[];
  workspace: Workspace | null;
  connection: import("@/lib/types").ConnectionState;
  workspaceForm: import("@/lib/types").WorkspaceFormState;
  agentForm: AgentFormState;

  // Refs
  workspaceRef: React.MutableRefObject<Workspace | null>;
  autoReconnectRef: React.MutableRefObject<boolean>;
  hasOpenedConnectionRef: React.MutableRefObject<boolean>;
  reconnectAttemptRef: React.MutableRefObject<number>;
  activeScopeRef: React.MutableRefObject<ScopeRef | null>;
  activeThreadScopeRef: React.MutableRefObject<ScopeRef | null>;
  activeDirectScopeRef: React.MutableRefObject<ScopeRef | null>;
  actorIdRef: React.MutableRefObject<string | null>;

  // Callbacks
  applyConfig: (config: import("@/ipc/types").DesktopConfig) => void;
  applyMachines: (machines: MachineInfo[]) => void;
  loadMachines: (force?: boolean) => Promise<unknown>;
  loadWorkspaceData: (workspace: Workspace) => Promise<void>;
  connectWorkspace: (workspaceId: string, opts?: {
    automatic?: boolean;
    quiet?: boolean;
    reconnect?: boolean;
  }) => Promise<Workspace | null>;
  clearReconnectTimer: () => void;
  pushNotice: (msg: string) => void;
  applyChannelDeleted: (channelId: string) => void;
}

export function useActions(deps: ActionDeps) {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  const setLocalIdentity = useCallback(async (args: {
    userId: string;
    nickname: string;
    actorId: string;
  }): Promise<boolean> => {
    const d = depsRef.current;
    d.setBusy("account:set-local");
    d.setError(null);
    try {
      const result = await ipc.accountSetLocal(args);
      d.autoReconnectRef.current = false;
      d.hasOpenedConnectionRef.current = false;
      d.reconnectAttemptRef.current = 0;
      d.clearReconnectTimer();
      d.applyConfig(result.config);
      d.workspaceRef.current = null;
      d.actorIdRef.current = null;
      d.setConnection("idle");
      d.setChannels([]);
      d.setActors({});
      d.setRuns({});
      d.setInbox([]);
      d.setTasks([]);
      d.setMessages([]);
      d.setThreadMessages([]);
      d.setDirectMessages([]);
      d.setDirectDraft("");
      d.setActiveDirectActorId(null);
      d.setDirectScopesByActorId(() => ({}));
      d.pushNotice(`Signed in as ${accountName(result.account)}`);
      return true;
    } catch (err) {
      d.setError(errorText(err));
      return false;
    } finally {
      d.setBusy(null);
    }
  }, []);

  const logout = useCallback(async () => {
    const d = depsRef.current;
    d.setBusy("logout");
    try {
      d.autoReconnectRef.current = false;
      d.hasOpenedConnectionRef.current = false;
      d.reconnectAttemptRef.current = 0;
      d.clearReconnectTimer();
      d.applyConfig(await ipc.accountLogout());
      d.workspaceRef.current = null;
      d.setWorkspace(null);
      d.setConnection("idle");
      d.setChannels([]);
      d.setActors({});
      d.setRuns({});
      d.setInbox([]);
      d.setTasks([]);
      d.setMessages([]);
      d.setThreadMessages([]);
      d.setDirectMessages([]);
      d.setDirectDraft("");
      d.setActiveDirectActorId(null);
      d.setDirectScopesByActorId(() => ({}));
      d.setOnboardingActive(true);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const updateAccountAvatar = useCallback(async (avatarUrl: string) => {
    const d = depsRef.current;
    d.setBusy("account:avatar");
    d.setError(null);
    try {
      const next = await ipc.accountUpdateAvatar(avatarUrl);
      d.applyConfig(next);
      const updatedAccount = next.account ?? null;
      if (updatedAccount) {
        d.setActors((current) => ({
          ...current,
          [updatedAccount.actorId]: accountToActor(updatedAccount),
        }));
      }
      d.pushNotice("Account avatar updated");
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const addWorkspace = useCallback(async (): Promise<Workspace | null> => {
    const d = depsRef.current;
    const hasTarget = d.workspaceForm.advanced
      ? d.workspaceForm.serverUrl.trim()
      : d.workspaceForm.host.trim();
    if (!d.workspaceForm.name.trim() || !hasTarget) return null;
    d.setBusy("workspace:add");
    d.setError(null);
    try {
      const serverUrl = normalizeWorkspaceFormServerUrl(d.workspaceForm);
      const next = await ipc.workspaceAdd({
        name: d.workspaceForm.name.trim(),
        serverUrl,
        activate: true,
      });
      d.applyConfig(next);
      await d.loadMachines();
      d.setWorkspaceForm(defaultWorkspaceForm());
      const saved =
        next.workspaces.find((item) => item.serverUrl === serverUrl) ??
        next.workspaces.find((item) => item.id === next.active) ??
        null;
      if (saved) {
        await d.connectWorkspace(saved.id, { quiet: true });
        d.pushNotice(`Connected to ${saved.name}`);
      } else {
        d.pushNotice(`Server ${d.workspaceForm.name.trim()} added`);
      }
      return saved;
    } catch (err) {
      d.setError(errorText(err));
      return null;
    } finally {
      d.setBusy(null);
    }
  }, []);

  const removeWorkspace = useCallback(async (id: string) => {
    const d = depsRef.current;
    d.setBusy(`workspace:remove:${id}`);
    d.setError(null);
    try {
      const next = await ipc.workspaceRemove(id);
      d.applyConfig(next);
      await d.loadMachines();
      if (d.workspace?.id === id) {
        try {
          await ipc.disconnect();
        } catch {
          /* local state still closes */
        }
        d.autoReconnectRef.current = false;
        d.hasOpenedConnectionRef.current = false;
        d.reconnectAttemptRef.current = 0;
        d.clearReconnectTimer();
        d.workspaceRef.current = null;
        d.setWorkspace(null);
        d.setConnection("idle");
      }
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const checkMachines = useCallback(async () => {
    const d = depsRef.current;
    d.setBusy("machine:check");
    d.setError(null);
    try {
      await d.loadMachines(true);
      d.pushNotice("Registered hosts refreshed");
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const createMachine = useCallback(async (args: {
    name: string;
    dataRoot?: string;
  }): Promise<MachineInfo | null> => {
    const d = depsRef.current;
    const name = args.name.trim();
    if (!name) {
      d.setError("Host name is required.");
      return null;
    }
    d.setBusy("machine:create");
    d.setError(null);
    try {
      const result = await ipc.machineCreate({
        name,
        dataRoot: args.dataRoot?.trim() || undefined,
      });
      d.applyMachines(result.machines);
      d.pushNotice(`Host ${name} registration prepared`);
      return (
        result.machines.find(
          (machine) => machine.source === "local_registration" && machine.name === name,
        ) ??
        result.machines.find((machine) => machine.name === name) ??
        null
      );
    } catch (err) {
      d.setError(errorText(err));
      return null;
    } finally {
      d.setBusy(null);
    }
  }, []);

  const startLocalHost = useCallback(async (): Promise<boolean> => {
    const d = depsRef.current;
    if (d.connection !== "open" || !d.workspaceRef.current) {
      d.setError("Connect to a server before starting a local host.");
      return false;
    }
    const liveHost = d.machines.find(
      (machine) =>
        machine.source === "server_inventory" && machine.connectionStatus === "online",
    );
    if (liveHost) {
      d.pushNotice(`${liveHost.name} is already online`);
      return true;
    }

    let machine =
      d.machines.find((item) => item.source === "local_registration") ?? null;
    if (!machine) {
      machine = await createMachine({ name: "Local Host" });
      if (!machine) return false;
    }

    d.setBusy("machine:start");
    d.setError(null);
    try {
      const result = await ipc.machineStart(machine.id);
      d.applyMachines(result.machines);
      d.pushNotice(`Local host started (pid ${result.pid})`);
      window.setTimeout(() => {
        void d.loadMachines(true).catch(() => {});
      }, 1200);
      return true;
    } catch (err) {
      d.setError(errorText(err));
      return false;
    } finally {
      d.setBusy(null);
    }
  }, [createMachine]);

  const removeMachine = useCallback(async (machineId: string) => {
    const d = depsRef.current;
    d.setBusy(`machine:remove:${machineId}`);
    d.setError(null);
    try {
      const result = await ipc.machineRemove(machineId);
      d.applyMachines(result.machines);
      d.pushNotice("Registered host removed");
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const createAgent = useCallback(async (form: AgentFormState = depsRef.current.agentForm): Promise<boolean> => {
    const d = depsRef.current;
    const name = form.name.trim();
    const machine = resolveAgentMachine(form, d.machines);
    const provider = resolveAgentProvider(form, machine);
    if (!machine) {
      d.setError("Register a host before creating an agent.");
      return false;
    }
    if (!machineCanCreateAgent(machine)) {
      d.setError(
        machine.connectionStatus !== "online"
          ? `Start the daemon on ${machine.name} before creating an agent.`
          : !machineCanRunCommands(machine)
            ? `Host ${machine.name} cannot run agent commands for the current account.`
            : `Host ${machine.name} does not support agent creation.`,
      );
      return false;
    }
    if (!provider) {
      d.setError(`No agent runtime is available for ${machine.name}.`);
      return false;
    }
    if (!name) {
      d.setError("Agent name is required.");
      return false;
    }
    d.setBusy("agent:create");
    d.setError(null);
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
        wake: form.wake,
      });
      d.applyMachines(result.machines);
      d.setAgentForm((current) =>
        normalizeAgentForm({ ...current, actorId: "", name: "Echo" }, result.machines),
      );
      if (d.workspace && d.connection === "open") {
        await d.loadWorkspaceData(d.workspace);
      }
      d.pushNotice(`Agent ${name} added`);
      return true;
    } catch (err) {
      d.setError(errorText(err));
      return false;
    } finally {
      d.setBusy(null);
    }
  }, []);

  const removeAgent = useCallback(async (machineId: string, actorId: string) => {
    const d = depsRef.current;
    d.setBusy(`agent:remove:${actorId}`);
    d.setError(null);
    try {
      const result = await ipc.machineAgentRemove({ machineId, actorId });
      d.applyMachines(result.machines);
      if (d.workspace && d.connection === "open") {
        await d.loadWorkspaceData(d.workspace);
      }
      d.pushNotice("Agent removed");
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const updateAgent = useCallback(async (patch: AgentUpdatePatch) => {
    const d = depsRef.current;
    d.setBusy(`agent:update:${patch.actorId}`);
    d.setError(null);
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
        bundleSkills: patch.bundleSkills,
        wake: patch.wake,
      });
      await d.loadMachines();
      if (d.workspace && d.connection === "open") {
        await d.loadWorkspaceData(d.workspace);
      }
      d.pushNotice("Agent settings saved");
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const addAgentSkill = useCallback(async (machineId: string, actorId: string, source: string) => {
    const d = depsRef.current;
    d.setBusy(`agent:skill:add:${actorId}`);
    d.setError(null);
    try {
      await ipc.agentSkillAdd({
        machineId,
        actorId,
        source: source.trim(),
      });
      await d.loadMachines();
      if (d.workspace && d.connection === "open") {
        await d.loadWorkspaceData(d.workspace);
      }
      d.pushNotice("Skill added");
      return true;
    } catch (err) {
      d.setError(errorText(err));
      return false;
    } finally {
      d.setBusy(null);
    }
  }, []);

  const inviteMemberToChannel = useCallback(async (channelId: string, actorId: string) => {
    const d = depsRef.current;
    const actor = d.actors[actorId];
    d.setBusy(`channel:invite:${channelId}:${actorId}`);
    d.setError(null);
    try {
      const result = await ipc.channelInvite({ channelId, actorId });
      d.setChannels((current) => sortChannels(upsert(current, result.channel)));
      d.pushNotice(`${actor ? displayName(actor) : actorId} added to channel`);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const removeMemberFromChannel = useCallback(async (channelId: string, actorId: string) => {
    const d = depsRef.current;
    const actor = d.actors[actorId];
    d.setBusy(`channel:revoke:${channelId}:${actorId}`);
    d.setError(null);
    try {
      const result = await ipc.channelRevoke({ channelId, actorId });
      d.setChannels((current) => sortChannels(upsert(current, result.channel)));
      d.setChannelMemberConfigsByChannel((current) => {
        const channelConfigs = current[channelId];
        if (!channelConfigs) return current;
        const nextChannelConfigs = { ...channelConfigs };
        delete nextChannelConfigs[actorId];
        return { ...current, [channelId]: nextChannelConfigs };
      });
      d.pushNotice(`${actor ? displayName(actor) : actorId} removed from channel`);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const saveMemberWorkspace = useCallback(async (
    channelId: string,
    actorId: string,
    workspaceDir: string,
  ): Promise<boolean> => {
    const d = depsRef.current;
    const value = workspaceDir.trim();
    if (!value) {
      d.setError("Workspace path is required.");
      return false;
    }
    d.setBusy(`channel:member-workspace:${channelId}:${actorId}`);
    d.setError(null);
    try {
      const result = await ipc.channelMemberConfigSet({
        channelId,
        actorId,
        workspaceDir: value,
      });
      d.setChannelMemberConfigsByChannel((current) => ({
        ...current,
        [channelId]: {
          ...(current[channelId] ?? {}),
          [actorId]: result.config,
        },
      }));
      d.pushNotice("Workspace saved");
      return true;
    } catch (err) {
      d.setError(errorText(err));
      return false;
    } finally {
      d.setBusy(null);
    }
  }, []);

  const clearMemberWorkspace = useCallback(async (channelId: string, actorId: string): Promise<boolean> => {
    const d = depsRef.current;
    d.setBusy(`channel:member-workspace:${channelId}:${actorId}`);
    d.setError(null);
    try {
      await ipc.channelMemberConfigClear({ channelId, actorId });
      d.setChannelMemberConfigsByChannel((current) => {
        const channelConfigs = current[channelId];
        if (!channelConfigs) return current;
        const nextChannelConfigs = { ...channelConfigs };
        delete nextChannelConfigs[actorId];
        return { ...current, [channelId]: nextChannelConfigs };
      });
      d.pushNotice("Workspace reset");
      return true;
    } catch (err) {
      d.setError(errorText(err));
      return false;
    } finally {
      d.setBusy(null);
    }
  }, []);

  const openLocalPath = useCallback(async (path: string) => {
    const d = depsRef.current;
    d.setError(null);
    try {
      await ipc.openLocalPath(path);
    } catch (err) {
      d.setError(errorText(err));
    }
  }, []);

  const createChannelWithTitle = useCallback(async (rawTitle: string) => {
    const d = depsRef.current;
    const title = rawTitle.trim();
    const currentWorkspace = d.workspaceRef.current ?? d.workspace;
    if (!title) return;
    if (!currentWorkspace) {
      d.setError("Add or select a space before creating a channel.");
      return;
    }
    d.setBusy("channel:create");
    try {
      let channelWorkspace = currentWorkspace;
      if (d.connection !== "open") {
        const connected = await d.connectWorkspace(currentWorkspace.id, { quiet: true });
        if (!connected) return;
        channelWorkspace = connected;
      }
      const result = await ipc.channelCreate({
        title,
        actorId: channelWorkspace.actorId,
      });
      d.setChannels((current) => sortChannels(upsert(current, result.channel)));
      d.setActiveChannelId(result.channel.id);
      d.setActiveThreadId(null);
      d.setChannelPanelTab(null);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const renameChannel = useCallback(async (channel: Channel, rawTitle: string) => {
    const d = depsRef.current;
    const title = rawTitle.trim();
    if (!title || title === channel.title) return;
    d.setBusy(`channel:rename:${channel.id}`);
    d.setError(null);
    try {
      const result = await ipc.channelUpdate({
        channelId: channel.id,
        title,
        topic: channel.topic,
      });
      d.setChannels((current) => sortChannels(upsert(current, result.channel)));
      d.pushNotice(`Renamed #${channel.title} to #${result.channel.title}`);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const deleteChannel = useCallback(async (channel: Channel) => {
    const d = depsRef.current;
    d.setBusy(`channel:delete:${channel.id}`);
    d.setError(null);
    try {
      const result = await ipc.channelDelete({
        channelId: channel.id,
        cascade: true,
      });
      if (result.deleted) {
        d.applyChannelDeleted(channel.id);
        const deletedThreads = result.deletedThreads ?? 0;
        d.pushNotice(
          deletedThreads > 0
            ? `Deleted #${channel.title} and ${deletedThreads} threads`
            : `Deleted #${channel.title}`,
        );
      }
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const uploadAttachments = useCallback(
    async (
      pending: PendingAttachment[],
      scope: ScopeRef | null,
      actorId: string,
    ): Promise<string[]> => {
      if (pending.length === 0) return [];
      const ids: string[] = [];
      for (const attachment of pending) {
        const result = await ipc.artifactPublish({
          ingress: {
            kind: "fileBytes",
            name: attachment.name,
            mediaType: attachment.mediaType,
            bytes: Array.from(attachment.bytes),
          },
          createdBy: actorId,
          ...(scope ? { scope } : {}),
        });
        ids.push(result.artifact.id);
      }
      return ids;
    },
    [],
  );

  const convertLongTextToAttachment = useCallback(
    async (
      text: string,
      scope: ScopeRef | null,
      actorId: string,
    ): Promise<string> => {
      const result = await ipc.artifactPublish({
        ingress: {
          kind: "inlineText",
          name: `message-${Date.now()}.txt`,
          mediaType: "text/plain",
          text,
        },
        createdBy: actorId,
        ...(scope ? { scope } : {}),
      });
      return result.artifact.id;
    },
    [],
  );

  const sendMessage = useCallback(async (attachments?: PendingAttachment[]) => {
    const d = depsRef.current;
    const body = d.draft.trim();
    if (!body || !d.target) return;
    d.setBusy("message:send");
    try {
      const parentMessageId = d.replyTo?.id;
      const repliedActor = d.replyTo ? d.actors[d.replyTo.authorActorId] : undefined;
      const mentionableActors = d.activeChannel
        ? channelMentionActors(d.activeChannel, d.actors)
        : [];
      const mentionedAudience = mentionAudience(
        body,
        mentionableActors,
        d.workspace?.actorId,
      );
      const replyAudience =
        repliedActor && repliedActor.id !== d.workspace?.actorId
          ? [{ kind: "actor" as const, id: repliedActor.id }]
          : [];
      const directedTo = uniqueAudience([...replyAudience, ...mentionedAudience]);
      const unavailableAgents = d.activeChannel
        ? directedTo.filter(
            (audience) =>
              audience.kind === "actor" &&
              !isChannelMentionActor(d.activeChannel!, audience.id),
          )
        : [];
      if (unavailableAgents.length > 0) {
        d.setError(
          `Add ${unavailableAgents
            .map((audience) => actorName(d.actors, audience.id))
            .join(", ")} to this channel before mentioning them.`,
        );
        return;
      }
      const wakesAgent = directedTo.some((audience) =>
        audienceWakesAgent(audience, d.actors),
      );

      // Upload pending attachments + handle long text conversion
      const actorId = d.workspace?.actorId ?? d.actorIdRef.current ?? "";
      const scope = d.activeScopeRef.current;
      let attachmentIds: string[] = [];
      let messageBody = body;

      if (shouldConvertLongText(body.length)) {
        const textArtifactId = await convertLongTextToAttachment(body, scope, actorId);
        attachmentIds = [textArtifactId, ...await uploadAttachments(attachments ?? [], scope, actorId)];
        messageBody = body.slice(0, 500) + "\n\n[... full text attached as .txt ...]";
      } else {
        attachmentIds = await uploadAttachments(attachments ?? [], scope, actorId);
      }

      const result = await ipc.messageSend({
        target: d.target,
        body: messageBody,
        parentMessageId,
        audience: directedTo,
        deliveryPolicy: wakesAgent ? "wake_agent" : "notify_only",
        intent: wakesAgent ? "request_action" : "chat",
        ...(attachmentIds.length > 0 ? { attachments: attachmentIds } : {}),
      });
      d.setMessages((current) => sortMessages(upsertMessage(current, result.message)));
      d.setDraft("");
      d.setReplyTo(null);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const sendThreadMessage = useCallback(async (attachments?: PendingAttachment[]) => {
    const d = depsRef.current;
    const body = d.threadDraft.trim();
    if (!body || !d.threadMessageTarget) return;
    d.setBusy("thread:message:send");
    try {
      const mentionableActors = d.activeChannel
        ? channelMentionActors(d.activeChannel, d.actors)
        : [];
      const mentionedAudience = mentionAudience(
        body,
        mentionableActors,
        d.workspace?.actorId,
      );
      const unavailableAgents = d.activeChannel
        ? mentionedAudience.filter(
            (audience) =>
              audience.kind === "actor" &&
              !isChannelMentionActor(d.activeChannel!, audience.id),
          )
        : [];
      if (unavailableAgents.length > 0) {
        d.setError(
          `Add ${unavailableAgents
            .map((audience) => actorName(d.actors, audience.id))
            .join(", ")} to this channel before mentioning them.`,
        );
        return;
      }
      const wakesAgent = mentionedAudience.some((audience) =>
        audienceWakesAgent(audience, d.actors),
      );

      // Upload pending attachments + handle long text conversion
      const actorId = d.workspace?.actorId ?? d.actorIdRef.current ?? "";
      const scope = d.activeThreadScopeRef.current;
      let attachmentIds: string[] = [];
      let messageBody = body;

      if (shouldConvertLongText(body.length)) {
        const textArtifactId = await convertLongTextToAttachment(body, scope, actorId);
        attachmentIds = [textArtifactId, ...await uploadAttachments(attachments ?? [], scope, actorId)];
        messageBody = body.slice(0, 500) + "\n\n[... full text attached as .txt ...]";
      } else {
        attachmentIds = await uploadAttachments(attachments ?? [], scope, actorId);
      }

      const result = await ipc.messageSend({
        target: d.threadMessageTarget,
        body: messageBody,
        audience: mentionedAudience,
        deliveryPolicy: wakesAgent ? "wake_agent" : "notify_only",
        intent: wakesAgent ? "request_action" : "chat",
        ...(attachmentIds.length > 0 ? { attachments: attachmentIds } : {}),
      });
      d.setThreadMessages((current) => sortMessages(upsertMessage(current, result.message)));
      d.setThreadStatsById((current) => upsertThreadStatsMessage(current, result.message));
      d.setThreadDraft("");
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const sendDirectMessage = useCallback(async () => {
    const d = depsRef.current;
    const body = d.directDraft.trim();
    if (!body || !d.activeDirectActor || !d.activeDirectTarget) return;
    const directMentions = mentionAudience(
      body,
      Object.values(d.actors),
      d.workspace?.actorId,
    );
    if (directMentions.length > 0) {
      d.setError("Direct messages do not support @ mentions.");
      return;
    }
    d.setBusy(`direct:message:send:${d.activeDirectActor.id}`);
    d.setError(null);
    try {
      const result = await ipc.messageSend({
        target: d.activeDirectTarget,
        body,
        deliveryPolicy: "wake_agent",
        intent: "request_action",
      });
      const message = normalizeMessage(result.message);
      d.activeDirectScopeRef.current = message.scope;
      d.setDirectScopesByActorId((current) => ({
        ...current,
        [d.activeDirectActor!.id]: message.scope,
      }));
      d.setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
      d.setDirectDraft("");
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const startThread = useCallback(async (message: Message) => {
    const d = depsRef.current;
    if (!d.activeChannel) return;
    const existing = d.channelThreads.find(
      (thread) => thread.rootMessageId === message.id,
    );
    if (existing) {
      d.setActiveThreadId(existing.id);
      d.setChannelPanelTab(null);
      return;
    }
    d.setBusy(`thread:create:${message.id}`);
    try {
      const result = await ipc.threadCreate({
        channelId: d.activeChannel.id,
        rootMessageId: message.id,
        title: threadTitle(message),
      });
      d.setThreadsByChannel((current) => ({
        ...current,
        [d.activeChannel!.id]: sortThreads(
          upsert(current[d.activeChannel!.id] ?? [], result.thread),
        ),
      }));
      d.setActiveThreadId(result.thread.id);
      d.setChannelPanelTab(null);
      d.setThreadStatsById((current) => ({
        ...current,
        [result.thread.id]: emptyThreadStats(),
      }));
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const toggleMessageReaction = useCallback(async (message: Message, emoji: string) => {
    const d = depsRef.current;
    d.setBusy(`message:reaction:${message.id}:${emoji}`);
    d.setError(null);
    try {
      const result = await ipc.messageReactionToggle({
        messageId: message.id,
        emoji,
      });
      const updatedMessage = normalizeMessage(result.message);
      if (
        d.activeScopeRef.current &&
        sameScope(updatedMessage.scope, d.activeScopeRef.current)
      ) {
        d.setMessages((current) => sortMessages(upsertMessage(current, updatedMessage)));
      }
      if (
        d.activeThreadScopeRef.current &&
        sameScope(updatedMessage.scope, d.activeThreadScopeRef.current)
      ) {
        d.setThreadMessages((current) => sortMessages(upsertMessage(current, updatedMessage)));
      }
      if (
        d.activeDirectScopeRef.current &&
        sameScope(updatedMessage.scope, d.activeDirectScopeRef.current)
      ) {
        d.setDirectMessages((current) => sortMessages(upsertMessage(current, updatedMessage)));
      }
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const answerAction = useCallback(async (message: Message, optionId: string, accepted: boolean) => {
    const d = depsRef.current;
    const responseTarget = message.target || d.target;
    if (!responseTarget || !d.workspace) return;
    const responseKind = accepted ? "accepted" : "declined";
    d.setBusy(`action:${message.id}:${optionId}`);
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
        actorId: d.workspace.actorId,
        sourceId: message.id,
      });
      d.setInbox((current) =>
        current.filter((item) => item.delivery.sourceId !== message.id),
      );
      d.pushNotice(`Action ${responseKind}`);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const answerDirectAction = useCallback(async (message: Message, optionId: string, accepted: boolean) => {
    const d = depsRef.current;
    if (!d.workspace) return;
    const peerActorId =
      directPeerForMessage(message, d.workspace.actorId) ?? d.activeDirectActor?.id ?? null;
    const responseTarget = peerActorId
      ? directMessageTarget(peerActorId)
      : message.target;
    const responseKind = accepted ? "accepted" : "declined";
    d.setBusy(`action:${message.id}:${optionId}`);
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
        actorId: d.workspace.actorId,
        sourceId: message.id,
      });
      d.setInbox((current) =>
        current.filter((item) => item.delivery.sourceId !== message.id),
      );
      d.pushNotice(`Action ${responseKind}`);
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setBusy(null);
    }
  }, []);

  const selectWorkspace = useCallback(async (workspaceId: string): Promise<Workspace | null> => {
    const d = depsRef.current;
    d.setView("chat");
    d.setChannelPanelTab(null);
    if (d.workspace?.id !== workspaceId || d.connection !== "open") {
      return d.connectWorkspace(workspaceId);
    }
    return d.workspaceRef.current ?? d.workspace ?? null;
  }, []);

  const finishOnboarding = useCallback(() => {
    const d = depsRef.current;
    d.setOnboardingActive(false);
    d.setView("chat");
    if (d.connection === "open") {
      void d.loadMachines(true).catch(() => {});
    }
  }, []);

  return {
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
  };
}

// Helper needed locally
function errorText(err: unknown): string {
  if (err instanceof Error) return err.message;
  return String(err);
}
