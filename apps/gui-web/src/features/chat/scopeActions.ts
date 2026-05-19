import * as ipc from "@/ipc/bridge";
import type { ScopeRef } from "@/ipc/types";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useUI } from "@/store/ui";

export async function openScope(scope: ScopeRef) {
  useChannels.getState().setCurrentScope(scope);
  useMessages.getState().ensureScope(scope);
  try {
    const parentChannelId =
      scope.kind === "thread" ? findKnownThreadChannelId(scope.id) : undefined;
    if (parentChannelId) {
      await ipc.scopeSubscribe({ kind: "channel", id: parentChannelId });
    }
    await ipc.scopeSubscribe(scope);
    const res = await ipc.scopeRead(scope, 100);
    useMessages.getState().ingestBackfill(scope, res.events);
  } catch (e) {
    useUI
      .getState()
      .pushToast(
        "error",
        `open scope: ${e instanceof Error ? e.message : String(e)}`,
      );
  }
}

function findKnownThreadChannelId(threadId: string): string | undefined {
  const { threadsByChannel, archivedThreadsByChannel } = useChannels.getState();
  for (const [channelId, threads] of Object.entries(threadsByChannel)) {
    if (threads.some((thread) => thread.id === threadId)) return channelId;
  }
  for (const [channelId, threads] of Object.entries(archivedThreadsByChannel)) {
    if (threads.some((thread) => thread.id === threadId)) return channelId;
  }
  return undefined;
}
