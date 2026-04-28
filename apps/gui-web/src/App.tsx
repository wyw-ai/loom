import { useEffect } from "react";

import * as ipc from "@/ipc/bridge";
import { scopeKey, type JoiEvent, type ScopeRef } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useInbox } from "@/store/inbox";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";
import { ServerRail } from "@/features/sidebar/ServerRail";
import { ChannelsPane } from "@/features/sidebar/ChannelsPane";
import { MembersRail } from "@/features/members/MembersRail";
import { ChatView } from "@/features/chat/ChatView";
import { InboxPage } from "@/features/inbox/InboxPage";
import { Landing } from "@/features/landing/Landing";
import { Toast } from "@/features/common/Toast";
import { ModalHost } from "@/features/common/Modal";
import { ContextMenuHost } from "@/features/common/ContextMenu";
import { DisconnectedOverlay } from "@/features/common/DisconnectedOverlay";
import { AddWorkspaceHost } from "@/features/workspaces/AddWorkspaceModal";
import { summarizeActionRequest } from "@/features/chat/actionRequestSummary";
import { notifyDesktop } from "@/features/notifications/desktop";

export function App() {
  const view = useUI((s) => s.view);
  const sidebarVisible = useUI((s) => s.sidebarVisible);
  const membersVisible = useUI((s) => s.membersVisible);
  const workspace = useSession((s) => s.workspace);
  const connection = useSession((s) => s.connection);

  // Bootstrap once: load workspaces into the store and bind notification
  // listeners. We do NOT auto-connect — the user picks a workspace from the
  // ServerRail, Discord-style.
  useEffect(() => {
    let offStream: (() => void) | null = null;
    let offDelta: (() => void) | null = null;
    let offConn: (() => void) | null = null;

    (async () => {
      try {
        const cfg = await ipc.workspacesList();
        useWorkspaces.getState().setConfig(cfg);
      } catch (e) {
        useUI
          .getState()
          .pushToast(
            "error",
            `load workspaces failed: ${e instanceof Error ? e.message : String(e)}`,
          );
      }

      offStream = await ipc.onStream((u) => handleStream(u));
      offDelta = await ipc.onStreamDelta((d) =>
        useMessages.getState().mergeDelta(d),
      );
      offConn = await ipc.onConnection((c) => {
        if (c.state === "closed") {
          useSession.getState().setConnection(
            "closed",
            (c as { reason?: string }).reason,
          );
          useUI.getState().pushToast("warn", "disconnected from joi-server");
        } else {
          useSession.getState().setConnection("open");
        }
      });
    })();

    return () => {
      offStream?.();
      offDelta?.();
      offConn?.();
    };
  }, []);

  // When a workspace becomes active, run the "after connect" bootstrap:
  // pull channel list. The connect() call itself happens in ServerRail /
  // Landing (user-initiated) so we never auto-contact a server the user
  // hasn't asked us to touch.
  useEffect(() => {
    if (connection !== "open" || !workspace) return;
    (async () => {
      try {
        const r = await ipc.channelList();
        useChannels.getState().replaceChannels(r.channels);
      } catch (e) {
        useUI
          .getState()
          .pushToast(
            "error",
            `channel/list failed: ${e instanceof Error ? e.message : String(e)}`,
          );
      }
      // Prime the global actor directory so Bubble headers resolve
      // displayName instead of the raw id. We also include ourselves so
      // own-message bubbles render the right name.
      try {
        const r = await ipc.actorList();
        useActors.getState().upsertMany(r.actors);
        if (workspace.displayName) {
          useActors.getState().upsert({
            id: workspace.actorId,
            kind: "human",
            displayName: workspace.displayName,
          });
        }
      } catch {
        /* best-effort — bubbles fall back to actorId */
      }
    })();
  }, [connection, workspace]);

  function handleStream(u: {
    kind: string;
    scope: ScopeRef;
    data: Record<string, unknown>;
  }) {
    const channels = useChannels.getState();
    const messages = useMessages.getState();
    const inbox = useInbox.getState();
    const me = useSession.getState().workspace?.actorId;

    switch (u.kind) {
      case "event.created": {
        const ev = u.data.event as JoiEvent | undefined;
        if (!ev) return;
        const forMe = ev.relations.some(
          (r) =>
            r.kind === "hands_off_to" &&
            r.target.kind === "actor" &&
            r.target.id === me,
        );
        const currentKey = channels.currentScope
          ? scopeKey(channels.currentScope)
          : null;
        const evScopeKey = scopeKey(ev.scope);

        if (ev.type === "action.request" && forMe && evScopeKey !== currentKey) {
          const p = summarizeActionRequest(
            (ev.payload ?? {}) as Record<string, unknown>,
          );
          inbox.add({
            requestEventId: ev.id,
            scope: ev.scope,
            title: p.title,
            description: p.description,
            reason: p.reason,
            command: p.command,
            rawInput: p.rawInput,
            actionRequestId: p.requestId,
            choices: p.choices,
            arrivedAt: ev.occurredAt,
            seen: false,
          });
          useUI
            .getState()
            .pushToast("warn", `action.request waiting in #${ev.scope.id}`);
          void notifyDesktop(
            "Action request waiting",
            `${p.title} in #${ev.scope.id}`,
          );
          return;
        }

        messages.ingestEvent(ev.scope, ev);
        if (ev.type === "action.response") {
          const reqId = ev.relations.find(
            (r) => r.kind === "responds_to" && r.target.kind === "event",
          )?.target.id;
          if (reqId) inbox.remove(reqId);
        }
        return;
      }
      case "turn.opened": {
        const turn = u.data.turn as
          | { id: string; actorId: string; scope: ScopeRef; openedAt: string }
          | undefined;
        if (!turn) return;
        if (turn.actorId === me) return;
        messages.openTurn(turn.scope, turn.id, turn.actorId, turn.openedAt);
        return;
      }
      case "turn.closed": {
        const turn = u.data.turn as
          | { id: string; scope: ScopeRef }
          | undefined;
        if (!turn) return;
        messages.closeTurn(turn.scope, turn.id);
        return;
      }
      case "channel.invited": {
        const channel = u.data.channel as
          | {
              id: string;
              title: string;
              visibility: "public" | "private";
              members: string[];
            }
          | undefined;
        if (!channel) return;
        channels.upsertChannel(channel);
        if (u.data.actorId === me) {
          useUI
            .getState()
            .pushToast("info", `you were added to #${channel.title}`);
          void notifyDesktop(
            "Added to channel",
            `You can now read and send messages in #${channel.title}.`,
          );
        }
        // If the MembersRail has already rendered this channel once, its
        // cached Actor[] no longer matches reality — refetch so the new
        // member shows up without requiring the user to reopen the rail.
        if (channels.membersByChannel[channel.id]) {
          (async () => {
            try {
              const m = await ipc.channelMembers(channel.id);
              useChannels.getState().replaceMembers(channel.id, m.members);
            } catch {
              /* stale — next open will refetch */
            }
          })();
        }
        return;
      }
      case "channel.revoked": {
        const channelId = u.data.channelId as string | undefined;
        if (!channelId) return;
        // `channel.revoked` fires both when THIS actor was revoked (server
        // drops us from the channel) and when another member was revoked
        // from a channel we're still in. We can't distinguish by payload
        // alone, so: if we still see the channel after revoke propagation,
        // refresh; otherwise drop it from state.
        if (channels.membersByChannel[channelId]) {
          (async () => {
            try {
              const m = await ipc.channelMembers(channelId);
              useChannels.getState().replaceMembers(channelId, m.members);
            } catch {
              // channel is gone for us — drop it.
              useChannels.getState().removeChannel(channelId);
            }
          })();
        } else {
          channels.removeChannel(channelId);
        }
        return;
      }
      case "thread.created": {
        const thread = u.data.thread as
          | { id: string; channelId: string; title: string }
          | undefined;
        if (thread) channels.upsertThread(thread);
        return;
      }
    }
  }

  // When no workspace is bound yet, render the Landing page in place of the
  // chat view. ServerRail still renders so the user can pick/add.
  const showLanding = !workspace || connection === "idle";

  return (
    <div
      className="grid h-screen w-screen overflow-hidden text-primary"
      style={{
        gridTemplateColumns: showLanding || !sidebarVisible
          ? "72px minmax(0, 1fr)"
          : "72px 240px minmax(0, 1fr)",
      }}
    >
      <ServerRail />
      {!showLanding && sidebarVisible && <ChannelsPane />}
      <main className="relative min-h-0 min-w-0 overflow-hidden bg-main">
        {showLanding ? (
          <Landing />
        ) : (
          <>
            <ChatView />
            {view === "inbox" && (
              <div className="absolute inset-0 z-10 bg-main">
                <InboxPage />
              </div>
            )}
          </>
        )}
        {!showLanding && view === "chat" && membersVisible && <MembersRail />}
        {connection === "closed" && workspace && <DisconnectedOverlay />}
      </main>
      <Toast />
      <ModalHost />
      <ContextMenuHost />
      <AddWorkspaceHost />
    </div>
  );
}
