// Channel / thread CRUD wired through the generic Modal/ContextMenu hosts.
// Kept out of the rendering components so the right-click menus stay terse
// and the flow is easy to unit-test.

import * as ipc from "@/ipc/bridge";
import type { Channel, Thread } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { openScope } from "@/features/chat/scopeActions";

export function openCreateChannel() {
  (async () => {
    const actorItems = await loadActorItems();
    useUI.getState().openModal({
      type: "channelForm",
      title: "Create channel",
      initialTitle: "",
      initialDescription: "",
      confirmLabel: "Create Channel",
      actorItems,
      onSubmit: async ({ title, actorIds }) => {
        const t = title.trim();
        if (!t) return;
        const actorId = useSession.getState().workspace?.actorId;
        try {
          const { channel } = await ipc.channelCreate({ title: t, actorId });
          useChannels.getState().upsertChannel(channel);
          for (const invitee of actorIds) {
            try {
              await ipc.channelInvite({
                channelId: channel.id,
                actorId: invitee,
              });
            } catch {
              /* keep channel creation successful; invite can be retried */
            }
          }
          await openScope({ kind: "channel", id: channel.id });
        } catch (e) {
          useUI
            .getState()
            .pushToast(
              "error",
              `create channel: ${e instanceof Error ? e.message : String(e)}`,
            );
        }
      },
    });
  })();
}

export function openRenameChannel(channel: Channel) {
  useUI.getState().openModal({
    type: "channelForm",
    title: "Edit channel",
    nameLabel: "Name",
    initialTitle: channel.title,
    initialDescription:
      channel.title === "all" ? "General channel for all members" : "",
    titleLocked: channel.title === "all",
    confirmLabel: "Save Changes",
    onSubmit: async ({ title }) => {
      const t = title.trim();
      if (!t || t === channel.title) return;
      try {
        const r = await ipc.channelUpdate({
          channelId: channel.id,
          title: t,
        });
        useChannels.getState().upsertChannel(r.channel);
      } catch (e) {
        useUI
          .getState()
          .pushToast(
            "error",
            `rename: ${e instanceof Error ? e.message : String(e)}`,
          );
      }
    },
  });
}

export function openDeleteChannel(channel: Channel) {
  useUI.getState().openModal({
    type: "confirm",
    title: `Delete #${channel.title}?`,
    body: "Channel and its threads are removed for every member. This cannot be undone.",
    confirmLabel: "Delete",
    danger: true,
    onConfirm: async () => {
      try {
        await ipc.channelDelete({ channelId: channel.id, cascade: true });
        useChannels.getState().removeChannel(channel.id);
        const current = useChannels.getState().currentScope;
        if (
          current &&
          (current.id === channel.id ||
            useChannels
              .getState()
              .threadsByChannel[channel.id]?.some((t) => t.id === current.id))
        ) {
          useChannels.getState().setCurrentScope(null);
        }
      } catch (e) {
        useUI
          .getState()
          .pushToast(
            "error",
            `delete: ${e instanceof Error ? e.message : String(e)}`,
          );
      }
    },
  });
}

export function openInviteToChannel(channel: Channel) {
  // Pre-load actor directory so the picker has names, then show.
  (async () => {
    let actors: Array<{ id: string; label: string; hint: string }> = [];
    try {
      const r = await ipc.actorList();
      useActors.getState().upsertMany(r.actors);
      const already = new Set(channel.members);
      actors = r.actors
        .filter((a) => !already.has(a.id))
        .map((a) => ({
          id: a.id,
          label: a.displayName || a.id,
          hint: a.kind,
        }));
    } catch (e) {
      useUI
        .getState()
        .pushToast(
          "error",
          `actor/list: ${e instanceof Error ? e.message : String(e)}`,
        );
      return;
    }
    if (actors.length === 0) {
      useUI
        .getState()
        .pushToast("info", "every known actor is already a member");
      return;
    }
    useUI.getState().openModal({
      type: "picker",
      title: `Invite to #${channel.title}`,
      items: actors,
      onPick: async (actorId) => {
        try {
          const r = await ipc.channelInvite({
            channelId: channel.id,
            actorId,
          });
          useChannels.getState().upsertChannel(r.channel);
          // The channel payload carries member IDs but the MembersRail needs
          // full Actor rows (displayName, kind, …). Refetch so the panel
          // shows the new invitee immediately without waiting for the user
          // to toggle/reopen the rail.
          try {
            const m = await ipc.channelMembers(channel.id);
            useChannels.getState().replaceMembers(channel.id, m.members);
          } catch {
            /* leave stale — next open will refetch */
          }
          useUI
            .getState()
            .pushToast("info", `invited ${actorId} to #${channel.title}`);
        } catch (e) {
          useUI
            .getState()
            .pushToast(
              "error",
              `invite: ${e instanceof Error ? e.message : String(e)}`,
            );
        }
      },
    });
  })();
}

export function openCreateThread(channelId: string) {
  useUI.getState().openModal({
    type: "input",
    title: "New thread",
    label: "Title",
    placeholder: "e.g. incident-0424",
    confirmLabel: "Create",
    onSubmit: async (title) => {
      const t = title.trim();
      if (!t) return;
      try {
        const r = await ipc.threadCreate({ channelId, title: t });
        useChannels.getState().upsertThread(r.thread);
        await openScope({ kind: "thread", id: r.thread.id });
      } catch (e) {
        useUI
          .getState()
          .pushToast(
            "error",
            `create thread: ${e instanceof Error ? e.message : String(e)}`,
          );
      }
    },
  });
}

async function loadActorItems(): Promise<
  Array<{ id: string; label: string; hint?: string; kind?: string }>
> {
  try {
    const r = await ipc.actorList();
    useActors.getState().upsertMany(r.actors);
    return r.actors
      .filter((a) => a.kind !== "service")
      .map((a) => ({
        id: a.id,
        label: a.displayName || a.id,
        hint: a.kind,
        kind: a.kind,
      }));
  } catch {
    return [];
  }
}

export function openRenameThread(thread: Thread) {
  useUI.getState().openModal({
    type: "input",
    title: `Rename "${thread.title}"`,
    label: "Title",
    initial: thread.title,
    confirmLabel: "Rename",
    onSubmit: async (title) => {
      const t = title.trim();
      if (!t || t === thread.title) return;
      try {
        const r = await ipc.threadUpdate({ threadId: thread.id, title: t });
        useChannels.getState().upsertThread(r.thread);
      } catch (e) {
        useUI
          .getState()
          .pushToast(
            "error",
            `rename thread: ${e instanceof Error ? e.message : String(e)}`,
          );
      }
    },
  });
}

export function openDeleteThread(thread: Thread) {
  useUI.getState().openModal({
    type: "confirm",
    title: `Delete "${thread.title}"?`,
    body: "The thread and its events are hidden from every member. This cannot be undone.",
    confirmLabel: "Delete",
    danger: true,
    onConfirm: async () => {
      try {
        await ipc.threadDelete({ threadId: thread.id });
        useChannels.getState().removeThread(thread.channelId, thread.id);
        const current = useChannels.getState().currentScope;
        if (current?.kind === "thread" && current.id === thread.id) {
          useChannels.getState().setCurrentScope(null);
        }
      } catch (e) {
        useUI
          .getState()
          .pushToast(
            "error",
            `delete thread: ${e instanceof Error ? e.message : String(e)}`,
          );
      }
    },
  });
}
