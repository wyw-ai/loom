// Shared "connect this workspace" orchestration. Used by both the ServerRail
// click handler and the Landing's big button. Keeping this out of the
// components means the two entry points stay in sync on error handling and
// state transitions — the failure path especially is easy to get wrong.

import * as ipc from "@/ipc/bridge";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useInbox } from "@/store/inbox";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";

export async function connectWorkspace(workspaceId: string) {
  const state = useWorkspaces.getState();
  const ws = state.workspaces.find((w) => w.id === workspaceId);
  if (!ws) {
    useUI.getState().pushToast("error", "unknown workspace");
    return;
  }

  const previousWorkspaceId = useSession.getState().workspace?.id;
  const switchingWorkspace =
    state.activeId !== workspaceId || previousWorkspaceId !== workspaceId;

  if (switchingWorkspace) {
    if (useSession.getState().connection === "open") {
      await ipc.disconnect().catch(() => undefined);
    }
    useSession.getState().setWorkspace(null);
    useSession.getState().setConnection("idle");
    resetServerScopedStores();
  }

  try {
    const cfg = await ipc.setActiveWorkspace(workspaceId);
    useWorkspaces.getState().setConfig(cfg);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    useUI.getState().pushToast("error", `switch server failed: ${msg}`);
    return;
  }

  if (!useWorkspaces.getState().account) {
    useUI.getState().setView("settings");
    useUI.getState().pushToast("warn", "sign in to Account before connecting");
    return;
  }
  useWorkspaces.getState().setConnecting(workspaceId);
  useSession.getState().setConnection("connecting");
  try {
    const result = await ipc.connect(workspaceId);
    const cfg = await ipc.workspacesList();
    useWorkspaces.getState().setConfig(cfg);
    const connectedWorkspace =
      result.workspace ?? cfg.workspaces.find((w) => w.id === workspaceId) ?? ws;
    useSession.getState().setWorkspace(connectedWorkspace);
    useSession.getState().setConnection("open");
    resetServerScopedStores();
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    useSession.getState().setConnection("error", msg);
    useUI.getState().pushToast("error", `connect failed: ${msg}`);
  } finally {
    useWorkspaces.getState().setConnecting(null);
  }
}

export async function disconnectWorkspace() {
  try {
    await ipc.disconnect();
  } finally {
    useSession.getState().setWorkspace(null);
    useSession.getState().setConnection("idle");
    resetServerScopedStores();
  }
}

export function resetServerScopedStores() {
  useChannels.setState({
    channels: [],
    threadsByChannel: {},
    membersByChannel: {},
    currentScope: null,
    selected: {},
  });
  useMessages.setState({ byScope: {} });
  useInbox.setState({ items: [] });
  useActors.getState().clear();
}
