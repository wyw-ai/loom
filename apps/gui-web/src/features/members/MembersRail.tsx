import { useEffect } from "react";
import { UserPlus, X } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Actor, Channel } from "@/ipc/types";
import { useChannels } from "@/store/channels";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { openInviteToChannel } from "@/features/sidebar/channelActions";
import { PixelAvatar } from "@/features/common/PixelAvatar";

export function MembersRail() {
  const scope = useChannels((s) => s.currentScope);
  const channels = useChannels((s) => s.channels);
  const threads = useChannels((s) => s.threadsByChannel);
  const members = useChannels((s) => s.membersByChannel);
  const replaceMembers = useChannels((s) => s.replaceMembers);
  const toggleMembers = useUI((s) => s.toggleMembers);

  const channelId =
    scope?.kind === "channel"
      ? scope.id
      : scope
      ? Object.entries(threads).find(([, ts]) =>
          ts.some((t) => t.id === scope.id),
        )?.[0]
      : undefined;

  useEffect(() => {
    if (!channelId) return;
    if (members[channelId]) return;
    (async () => {
      try {
        const r = await ipc.channelMembers(channelId);
        replaceMembers(channelId, r.members);
      } catch {
        /* ignore */
      }
    })();
  }, [channelId, members, replaceMembers]);

  if (!channelId) return null;

  const channel = channels.find((c) => c.id === channelId);
  const rows = members[channelId] ?? [];

  return (
    <aside className="absolute inset-y-0 right-0 z-20 flex w-72 max-w-[min(18rem,calc(100vw-6rem))] flex-col border-l-2 border-black bg-brutal-cream shadow-[-8px_0_0_#111]">
      <header className="flex h-panel-header shrink-0 items-center gap-2 border-b-2 border-black px-4">
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold text-primary">
            Participants
          </div>
          <div className="truncate text-xs text-muted">
            {channel?.title ?? channelId}
          </div>
        </div>
        {channel && (
          <button
            aria-label="Invite actor"
            title="Invite actor"
            className="btn-brutal-sm bg-white p-1.5"
            onClick={() => openInviteToChannel(channel)}
          >
            <UserPlus size={16} />
          </button>
        )}
        <button
          aria-label="Close members"
          title="Close members"
          className="btn-brutal-sm bg-white p-1.5"
          onClick={toggleMembers}
        >
          <X size={16} />
        </button>
      </header>
      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-scroll py-2">
        {rows.length === 0 ? (
          <div className="px-4 py-2 text-xs text-muted">no members</div>
        ) : (
          rows.map((m) => (
            <MemberRow key={m.id} actor={m} channel={channel} />
          ))
        )}
      </div>
    </aside>
  );
}

function MemberRow({
  actor,
  channel,
}: {
  actor: Actor;
  channel?: Channel;
}) {
  const selfId = useSession((s) => s.workspace?.actorId);
  const openContextMenu = useUI((s) => s.openContextMenu);
  const pushToast = useUI((s) => s.pushToast);
  const replaceMembers = useChannels((s) => s.replaceMembers);
  const isSelf = actor.id === selfId;

  const revoke = () => {
    if (!channel) return;
    useUI.getState().openModal({
      type: "confirm",
      title: `Remove ${actor.displayName || actor.id}?`,
      body: `They will lose access to #${channel.title}.`,
      confirmLabel: "Remove",
      danger: true,
      onConfirm: async () => {
        try {
          await ipc.channelRevoke({
            channelId: channel.id,
            actorId: actor.id,
          });
          const r = await ipc.channelMembers(channel.id);
          replaceMembers(channel.id, r.members);
        } catch (e) {
          pushToast(
            "error",
            `revoke: ${e instanceof Error ? e.message : String(e)}`,
          );
        }
      },
    });
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    if (!channel) return;
    openContextMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          kind: "item",
          label: "Remove from channel…",
          danger: true,
          disabled: isSelf,
          onClick: revoke,
        },
      ],
    });
  };

  return (
    <MemberRowInner actor={actor} onContextMenu={onContextMenu} />
  );
}

function MemberRowInner({
  actor,
  onContextMenu,
}: {
  actor: Actor;
  onContextMenu: (e: React.MouseEvent) => void;
}) {
  return (
    <div
      onContextMenu={onContextMenu}
      className="flex min-w-0 items-center gap-2 border-2 border-transparent px-3 py-1.5 text-sm font-bold text-black hover:border-black hover:bg-white"
    >
      <PixelAvatar id={actor.id} label={actor.displayName} size={28} />
      <span className="min-w-0 flex-1 truncate">{actor.displayName || actor.id}</span>
      <span className="shrink-0 font-mono text-[11px] text-black/45">{actor.kind}</span>
    </div>
  );
}
