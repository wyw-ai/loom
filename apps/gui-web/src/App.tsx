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
import { TasksPage } from "@/features/tasks/TasksPage";
import { MembersPage } from "@/features/members/MembersPage";
import { MachinesPage } from "@/features/members/MachinesPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { Toast } from "@/features/common/Toast";
import { ModalHost } from "@/features/common/Modal";
import { ContextMenuHost } from "@/features/common/ContextMenu";
import { DisconnectedOverlay } from "@/features/common/DisconnectedOverlay";
import { AddWorkspaceHost } from "@/features/workspaces/AddWorkspaceModal";
import { WorkspaceSwitcherHost } from "@/features/workspaces/WorkspaceSwitcher";
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

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "k") return;
      e.preventDefault();

      const ui = useUI.getState();
      if (ui.modal) return;
      ui.openModal({ type: "quickSwitch" });
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // When a workspace becomes active, run the "after connect" bootstrap:
  // pull channel list. The connect() call itself happens in ServerRail /
  // Landing (user-initiated) so we never auto-contact a server the user
  // hasn't asked us to touch.
  useEffect(() => {
    if (connection !== "open" || !workspace) return;
    let alive = true;
    const workspaceId = workspace.id;
    const stillCurrent = () =>
      alive &&
      useSession.getState().connection === "open" &&
      useSession.getState().workspace?.id === workspaceId;

    (async () => {
      try {
        const r = await ipc.channelList();
        if (!stillCurrent()) return;
        useChannels.getState().replaceChannels(r.channels);
      } catch (e) {
        if (!stillCurrent()) return;
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
        if (!stillCurrent()) return;
        useActors.getState().clear();
        useActors.getState().upsertMany(r.actors);
        const account = useWorkspaces.getState().account;
        if (account) {
          useActors.getState().upsert({
            id: account.actorId,
            kind: "human",
            displayName: account.nickname || account.realName || account.staffId,
            _meta: {
              account: {
                provider: account.provider,
                staffId: account.staffId,
                nickname: account.nickname,
                realName: account.realName,
                email: account.email,
              },
              avatarUrl: account.avatarUrl,
            },
          });
        } else if (workspace.displayName) {
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
    return () => {
      alive = false;
    };
  }, [connection, workspace?.id]);

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
      case "channel.created": {
        const channel = u.data.channel as
          | {
              id: string;
              title: string;
              visibility: "public" | "private";
              members: string[];
            }
          | undefined;
        if (channel) {
          const existing = channels.channels.find((c) => c.id === channel.id);
          channels.upsertChannel(
            existing
              ? {
                  ...channel,
                  members: mergeMemberIds(existing.members, channel.members),
                }
              : channel,
          );
        }
        return;
      }
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
          const payload = (ev.payload ?? {}) as Record<string, unknown>;
          inbox.add({
            requestEventId: ev.id,
            scope: ev.scope,
            title: p.title,
            description: p.description,
            requestType:
              typeof payload.requestType === "string"
                ? payload.requestType
                : undefined,
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

  // When no workspace is bound yet, render the Landing page only in the chat
  // slot. Local admin pages such as Machines should remain reachable before a
  // server connection exists.
  const showLanding = view === "chat" && (!workspace || connection === "idle");
  const fullWidthView =
    view === "members" || view === "machines" || view === "settings";

  return (
    <div
      className="grid h-screen w-screen overflow-hidden text-primary"
      style={{
        gridTemplateColumns: showLanding || fullWidthView || !sidebarVisible
          ? "72px minmax(0, 1fr)"
          : "72px 240px minmax(0, 1fr)",
      }}
    >
      <ServerRail />
      {!showLanding && !fullWidthView && sidebarVisible && <ChannelsPane />}
      <main className="relative min-h-0 min-w-0 overflow-hidden bg-main">
        {showLanding ? (
          <Landing />
        ) : view === "tasks" ? (
          <TasksPage />
        ) : view === "members" ? (
          <MembersPage />
        ) : view === "machines" ? (
          <MachinesPage />
        ) : view === "inbox" ? (
          <InboxPage />
        ) : view === "settings" ? (
          <SettingsPage />
        ) : (
          <ChatView />
        )}
        {!showLanding && view === "chat" && membersVisible && <MembersRail />}
        {connection === "closed" && workspace && <DisconnectedOverlay />}
      </main>
      <Toast />
      <ModalHost />
      <ContextMenuHost />
      <WorkspaceSwitcherHost />
      <AddWorkspaceHost />
    </div>
  );
}

function mergeMemberIds(...memberLists: string[][]): string[] {
  const seen = new Set<string>();
  const merged: string[] = [];
  for (const members of memberLists) {
    for (const member of members) {
      if (seen.has(member)) continue;
      seen.add(member);
      merged.push(member);
    }
  }
  return merged;
}
