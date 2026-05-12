// Slash-command dispatcher. Mirrors the TUI's slash verbs
// (crates/cli/src/cmd/chat/app.rs::slash_command_items) so muscle memory
// transfers. Returns `true` if the input was consumed as a command; the
// caller (Prompt) skips the usual "send as message" path in that case.

import * as ipc from "@/ipc/bridge";
import type { ScopeRef } from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import {
  openInviteToChannel,
} from "@/features/sidebar/channelActions";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";

type DispatchCtx = { scope: ScopeRef; actorId: string };

export async function tryHandleSlash(
  input: string,
  ctx: DispatchCtx,
): Promise<"consumed" | "passthrough"> {
  const trimmed = input.trim();
  if (!trimmed.startsWith("/")) return "passthrough";

  const [rawCmd, ...rest] = trimmed.split(/\s+/);
  const args = trimmed.slice(rawCmd.length).trim();

  switch (rawCmd) {
    case "/handoff":
      // Handled inline in Prompt.send so the flow stays identical to
      // "@agent msg" and becomes one content.add with a handoff relation.
      return "passthrough";

    case "/reply":
      openReplyPicker(ctx);
      return "consumed";

    case "/action":
      openActionPicker(ctx);
      return "consumed";

    case "/cancel":
      void doCancel(ctx, rest);
      return "consumed";

    case "/invite":
      openInviteForCurrentChannel(ctx);
      return "consumed";

    case "/members":
      useUI.getState().toggleMembers();
      return "consumed";

    case "/announce":
      void doAnnounce(ctx, args);
      return "consumed";

    case "/agents":
      void listAgents();
      return "consumed";

    case "/quit":
      void closeWindow();
      return "consumed";

    default:
      useUI
        .getState()
        .pushToast("warn", `unknown command: ${rawCmd}`);
      return "consumed";
  }
}

function openReplyPicker({ scope }: DispatchCtx) {
  const scopeStore = useMessages.getState().byScope[scopeKey(scope)];
  const bubbles = scopeStore?.bubbles ?? [];
  const items = bubbles
    .filter((b) => b.kind !== "system")
    .slice(-25)
    .reverse()
    .map((b) => ({
      id: b.id,
      label: preview(b.text),
      hint: `@${b.actorId}`,
    }));
  if (items.length === 0) {
    useUI.getState().pushToast("info", "no replyable events in this scope");
    return;
  }
  useUI.getState().openModal({
    type: "picker",
    title: "Reply to…",
    items,
    onPick: (eventId) => {
      const b = bubbles.find((x) => x.id === eventId);
      if (!b) return;
      useUI.getState().setReplyTarget(scope, {
        eventId,
        actorId: b.actorId,
        preview: preview(b.text),
        handoffTarget: b.handoffTarget,
      });
    },
  });
}

function openActionPicker({ scope }: DispatchCtx) {
  const scopeStore = useMessages.getState().byScope[scopeKey(scope)];
  if (!scopeStore || scopeStore.pendingActionIds.size === 0) {
    useUI.getState().pushToast("info", "no pending action.request here");
    return;
  }
  const items = [...scopeStore.pendingActionIds]
    .map((id) => scopeStore.bubbles.find((b) => b.id === id))
    .filter(Boolean)
    .map((b) => ({
      id: b!.id,
      label: preview(b!.text),
      hint: `@${b!.actorId}`,
    }));
  useUI.getState().openModal({
    type: "picker",
    title: "Respond to action.request",
    items,
    onPick: (eventId) => {
      const b = scopeStore.bubbles.find((x) => x.id === eventId);
      if (!b) return;
      // Sub-modal: pick the option. We only emit accepted/declined here —
      // the full option set lives on b.choices. If the bubble carries a
      // non-default choice list, show those; otherwise fall back to yes/no.
      const choices =
        (b.choices?.length ? b.choices : null) ?? [
          { id: "approve", label: "Approve" },
          { id: "reject", label: "Reject" },
        ];
      useUI.getState().openModal({
        type: "picker",
        title: `${preview(b.text)} —`,
        items: choices.map((c) => ({ id: c.id, label: c.label })),
        onPick: async (optionId) => {
          const label = choices.find((c) => c.id === optionId)?.label ?? "";
          const declined = /reject|decline|cancel|abort|no/i.test(label);
          const kind =
            b.requestType === "question" || b.requestType === "human_decision"
              ? "answered"
              : declined
                ? "declined"
                : "accepted";
          try {
            await ipc.eventAppend({
              type: "action.response",
              actorId: useSession.getState().workspace!.actorId,
              scope,
              payload: {
                optionId,
                kind,
                ...(b.actionRequestId ? { requestId: b.actionRequestId } : {}),
              },
              relations: [
                { kind: "responds_to", target: { kind: "event", id: eventId } },
              ],
            });
          } catch (e) {
            useUI
              .getState()
              .pushToast(
                "error",
                `action.response: ${e instanceof Error ? e.message : String(e)}`,
              );
          }
        },
      });
    },
  });
}

async function doCancel({ scope }: DispatchCtx, args: string[]) {
  const store = useMessages.getState().byScope[scopeKey(scope)];
  const open = Object.entries(store?.openTurns ?? {});
  if (open.length === 0) {
    useUI.getState().pushToast("info", "no open turn here");
    return;
  }

  // `/cancel @agent` — match by actor suffix.
  const target = args[0]?.startsWith("@") ? args[0].slice(1) : undefined;
  const picked = target
    ? open.find(([, info]) => info.actorId.endsWith(target))
    : open.length === 1
    ? open[0]
    : null;

  const run = async (turnId: string) => {
    try {
      await ipc.turnClose(turnId, "cancelled");
    } catch (e) {
      useUI
        .getState()
        .pushToast(
          "error",
          `cancel: ${e instanceof Error ? e.message : String(e)}`,
        );
    }
  };

  if (picked) {
    await run(picked[0]);
    return;
  }

  useUI.getState().openModal({
    type: "picker",
    title: "Cancel which turn?",
    items: open.map(([turnId, info]) => ({
      id: turnId,
      label: `@${info.actorId}`,
      hint: new Date(info.openedAt).toLocaleTimeString(),
    })),
    onPick: (turnId) => void run(turnId),
  });
}

function openInviteForCurrentChannel({ scope }: DispatchCtx) {
  const channels = useChannels.getState().channels;
  const threadsByChannel = useChannels.getState().threadsByChannel;
  let channelId: string | undefined;
  if (scope.kind === "channel") {
    channelId = scope.id;
  } else {
    for (const [chId, ts] of Object.entries(threadsByChannel)) {
      if (ts.some((t) => t.id === scope.id)) {
        channelId = chId;
        break;
      }
    }
  }
  const channel = channels.find((c) => c.id === channelId);
  if (!channel) {
    useUI.getState().pushToast("warn", "can't resolve current channel");
    return;
  }
  openInviteToChannel(channel);
}

async function doAnnounce({ scope, actorId }: DispatchCtx, args: string) {
  const body = args.trim();
  if (!body) {
    useUI
      .getState()
      .pushToast("warn", "usage: /announce <text>  |  /announce clear");
    return;
  }
  try {
    if (body === "clear") {
      await ipc.eventAppend({
        type: "announcement.clear",
        actorId,
        scope,
        payload: {},
      });
    } else {
      await ipc.eventAppend({
        type: "announcement.set",
        actorId,
        scope,
        payload: { text: body },
      });
    }
  } catch (e) {
    useUI
      .getState()
      .pushToast(
        "error",
        `announce: ${e instanceof Error ? e.message : String(e)}`,
      );
  }
}

async function listAgents() {
  try {
    const r = await ipc.agentList();
    if (r.agents.length === 0) {
      useUI.getState().pushToast("info", "no agents registered");
      return;
    }
    const lines = r.agents
      .map((a) => `@${a.spec.actor.id} — ${a.status}`)
      .join(" · ");
    useUI.getState().pushToast("info", lines);
  } catch (e) {
    useUI
      .getState()
      .pushToast(
        "error",
        `agent/list: ${e instanceof Error ? e.message : String(e)}`,
      );
  }
}

async function closeWindow() {
  try {
    const mod = await import("@tauri-apps/api/window");
    const win =
      (mod as unknown as { getCurrentWindow?: () => { close: () => Promise<void> } })
        .getCurrentWindow?.() ??
      (mod as unknown as { getCurrent?: () => { close: () => Promise<void> } })
        .getCurrent?.();
    await win?.close();
  } catch {
    window.close();
  }
}

function preview(text: string, max = 72): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? flat.slice(0, max - 1) + "…" : flat;
}
