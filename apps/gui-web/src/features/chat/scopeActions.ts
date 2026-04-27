import * as ipc from "@/ipc/bridge";
import type { ScopeRef } from "@/ipc/types";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useUI } from "@/store/ui";

export async function openScope(scope: ScopeRef) {
  useChannels.getState().setCurrentScope(scope);
  useMessages.getState().ensureScope(scope);
  try {
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
