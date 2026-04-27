// Shared "connect this workspace" orchestration. Used by both the ServerRail
// click handler and the Landing's big button. Keeping this out of the
// components means the two entry points stay in sync on error handling and
// state transitions — the failure path especially is easy to get wrong.

import * as ipc from "@/ipc/bridge";
import { useChannels } from "@/store/channels";
import { useInbox } from "@/store/inbox";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";

export async function connectWorkspace(workspaceId: string) {
  const ws = useWorkspaces
    .getState()
    .workspaces.find((w) => w.id === workspaceId);
  if (!ws) {
    useUI.getState().pushToast("error", "unknown workspace");
    return;
  }
  useWorkspaces.getState().setConnecting(workspaceId);
  useSession.getState().setConnection("connecting");
  try {
    await ipc.connect(workspaceId);
    useWorkspaces.getState().setActive(workspaceId);
    useSession.getState().setWorkspace(ws);
    useSession.getState().setConnection("open");
    // Connecting to a new workspace invalidates per-scope stores from the
    // previous one. Reset ambient state so the user doesn't see stale
    // bubbles/channel badges bleed across servers.
    useChannels.getState().replaceChannels([]);
    useChannels.getState().setCurrentScope(null);
    useInbox.setState({ items: [] });
    useMessages.setState({ byScope: {} });
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
    useChannels.getState().replaceChannels([]);
    useChannels.getState().setCurrentScope(null);
    useMessages.setState({ byScope: {} });
    useInbox.setState({ items: [] });
  }
}
