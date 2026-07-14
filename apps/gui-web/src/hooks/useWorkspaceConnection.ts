import { useCallback } from "react";
import type { MachineInfo, Workspace, DesktopConfig } from "@/ipc/types";
import type { AgentFormState, WorkspaceFormState, ConnectionState } from "@/lib/types";
import * as ipc from "@/ipc/bridge";
import {
  accountToActor,
  errorText,
  normalizeAgentForm,
} from "@/lib/format-utils";
import {
  isDirectChannel,
  sortChannels,
} from "@/lib/channel-utils";
import { sortTasks } from "@/lib/format-utils";
import { localServerCommand } from "@/lib/constants";
import { defaultWorkspaceForm } from "@/lib/server-url";
import type { View } from "@/lib/types";

export interface WorkspaceConnectionDeps {
  config: DesktopConfig;
  setConfig: (config: DesktopConfig) => void;
  workspace: Workspace | null;
  setWorkspace: (workspace: Workspace | null | ((prev: Workspace | null) => Workspace | null)) => void;
  connection: string;
  setConnection: (connection: ConnectionState | ((prev: ConnectionState) => ConnectionState)) => void;
  setError: (error: string | null) => void;
  setNotice: (notice: string | null) => void;
  setBusy: (busy: string | null) => void;
  setWorkspaceForm: (form: WorkspaceFormState) => void;
  setView: (view: View) => void;
  setChannels: (channels: import("@/ipc/types").Channel[]) => void;
  setActiveChannelId: (id: string | null | ((prev: string | null) => string | null)) => void;
  setActors: (actors: Record<string, import("@/ipc/types").Actor> | ((prev: Record<string, import("@/ipc/types").Actor>) => Record<string, import("@/ipc/types").Actor>)) => void;
  setTasks: (tasks: import("@/ipc/types").Task[]) => void;
  setInbox: (inbox: import("@/ipc/types").InboxListEntry[]) => void;
  setMachines: (machines: MachineInfo[]) => void;
  setAgentForm: (form: AgentFormState | ((prev: AgentFormState) => AgentFormState)) => void;
  setOnboardingActive: (active: boolean) => void;
  setConfigLoaded: (loaded: boolean) => void;
  account: import("@/ipc/types").HumanAccount | null;
  workspaceRef: React.MutableRefObject<Workspace | null>;
  autoReconnectRef: React.MutableRefObject<boolean>;
  hasOpenedConnectionRef: React.MutableRefObject<boolean>;
  reconnectTimerRef: React.MutableRefObject<number | null>;
  reconnectAttemptRef: React.MutableRefObject<number>;
  actorIdRef: React.MutableRefObject<string | null>;
}

export function useWorkspaceConnection(deps: WorkspaceConnectionDeps) {
  const d = deps;

  const applyConfig = useCallback((next: DesktopConfig) => {
    d.setConfig(next);
    const active = next.active
      ? next.workspaces.find((candidate) => candidate.id === next.active)
      : next.workspaces[0];
    if (active) {
      d.setWorkspace((current: Workspace | null) => {
        const selected =
          current && next.workspaces.some((candidate) => candidate.id === current.id)
            ? current
            : active;
        d.workspaceRef.current = selected;
        return selected;
      });
    } else {
      d.workspaceRef.current = null;
      d.setWorkspace(null);
    }
  }, []);

  const pushNotice = useCallback((text: string) => {
    d.setNotice(text);
    window.setTimeout(() => d.setNotice(null), 3200);
  }, []);

  const prepareLocalServerSpace = useCallback(() => {
    d.setWorkspaceForm(defaultWorkspaceForm());
    d.setView("spaces");
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
    if (d.reconnectTimerRef.current !== null) {
      window.clearTimeout(d.reconnectTimerRef.current);
      d.reconnectTimerRef.current = null;
    }
  }, []);

  const refreshInbox = useCallback(async (actorId: string) => {
    const result = await ipc.inboxList({
      actorId,
      state: "pending",
      limit: 100,
    });
    d.setInbox(result.deliveries);
  }, []);

  const applyMachines = useCallback((nextMachines: MachineInfo[]) => {
    d.setMachines(nextMachines);
    d.setAgentForm((current: AgentFormState) => normalizeAgentForm(current, nextMachines));
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
        d.setOnboardingActive(true);
      }
      void loadMachines().catch(() => {});
    } catch (err) {
      d.setError(errorText(err));
    } finally {
      d.setConfigLoaded(true);
    }
  }, [applyConfig, loadMachines]);

  const loadWorkspaceData = useCallback(
    async (current: Workspace) => {
      d.actorIdRef.current = current.actorId;
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
        if (d.account) nextActors[d.account.actorId] = accountToActor(d.account);
        d.setActors(nextActors);
      }
      if (channelResult.status === "fulfilled") {
        const nextChannels = sortChannels(channelResult.value.channels);
        const nextVisibleChannels = nextChannels.filter((channel) => !isDirectChannel(channel));
        d.setChannels(nextChannels);
        d.setActiveChannelId((currentChannel: string | null) =>
          currentChannel &&
          nextVisibleChannels.some((channel) => channel.id === currentChannel)
            ? currentChannel
            : nextVisibleChannels[0]?.id ?? null,
        );
      }
      if (taskResult.status === "fulfilled") {
        d.setTasks(sortTasks(taskResult.value.tasks));
      }
      if (inboxResult.status === "fulfilled") {
        d.setInbox(inboxResult.value.deliveries);
      }
      void loadMachines().catch(() => {});
    },
    [d.account, loadMachines],
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
        (automatic && d.hasOpenedConnectionRef.current && d.autoReconnectRef.current);
      if (d.workspaceRef.current?.id === workspaceId && d.connection === "open") {
        return d.workspaceRef.current;
      }
      if (!automatic) {
        d.autoReconnectRef.current = true;
        d.reconnectAttemptRef.current = 0;
      }
      clearReconnectTimer();
      if (!automatic && !quiet) d.setBusy(`connect:${workspaceId}`);
      d.setConnection("connecting");
      d.setError(reconnecting ? "Connection lost. Reconnecting..." : null);
      try {
        const result = await ipc.connect(workspaceId);
        d.workspaceRef.current = result.workspace;
        d.hasOpenedConnectionRef.current = true;
        d.autoReconnectRef.current = true;
        d.setWorkspace(result.workspace);
        d.setConnection("open");
        d.setError(null);
        d.reconnectAttemptRef.current = 0;
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
        d.setConnection("error");
        if (reconnecting) {
          d.setError("Connection lost. Reconnecting...");
        } else {
          d.autoReconnectRef.current = false;
          d.setError(automatic || quiet ? null : errorText(err));
        }
        return null;
      } finally {
        if (!automatic && !quiet) d.setBusy(null);
      }
    },
    [clearReconnectTimer, d.connection, loadWorkspaceData, pushNotice],
  );

  return {
    applyConfig,
    pushNotice,
    prepareLocalServerSpace,
    clearReconnectTimer,
    refreshInbox,
    applyMachines,
    loadMachines,
    loadConfig,
    loadWorkspaceData,
    connectWorkspace,
  };
}
