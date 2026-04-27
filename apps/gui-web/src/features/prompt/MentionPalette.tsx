import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";

import * as ipc from "@/ipc/bridge";
import type { ActorKind, Channel, ScopeRef, Thread } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useSession } from "@/store/session";

interface Entry {
  id: string;
  display: string;
  kind: ActorKind;
  status?: string;
}

export interface MentionPaletteHandle {
  pickFirst: () => boolean;
}

interface Props {
  filter: string;
  onPick: (actorId: string) => void;
}

// Returns the set of actor ids the @-menu is allowed to surface for the
// current scope, or `null` when there's no restriction (public channel, or
// thread whose parent is public — "公区"-like behavior). Mirrors
// crates/cli/src/cmd/chat/app.rs::current_channel_membership_ctx +
// publicly_addressable_actors.
function resolveAllowedAgents(
  currentScope: ScopeRef | null,
  channels: Channel[],
  threadsByChannel: Record<string, Thread[]>,
): Set<string> | null {
  if (!currentScope) return null;

  // Resolve the owning channel for the current scope.
  let channelId: string | undefined;
  if (currentScope.kind === "channel") {
    channelId = currentScope.id;
  } else {
    for (const [chId, ts] of Object.entries(threadsByChannel)) {
      if (ts.some((t) => t.id === currentScope.id)) {
        channelId = chId;
        break;
      }
    }
  }
  if (!channelId) return null;
  const ch = channels.find((c) => c.id === channelId);
  if (!ch || ch.visibility !== "private") return null;

  // Private — allow this channel's members + every member of any public
  // channel we can see (the "公区" fallback).
  const allow = new Set<string>(ch.members);
  for (const c of channels) {
    if (c.visibility === "public") {
      for (const id of c.members) allow.add(id);
    }
  }
  return allow;
}

export const MentionPalette = forwardRef<MentionPaletteHandle, Props>(
  function MentionPalette({ filter, onPick }, ref) {
    const actorsById = useActors((s) => s.byId);
    const selfId = useSession((s) => s.workspace?.actorId);
    const currentScope = useChannels((s) => s.currentScope);
    const channels = useChannels((s) => s.channels);
    const threadsByChannel = useChannels((s) => s.threadsByChannel);
    const [statuses, setStatuses] = useState<Record<string, string>>({});

    // actor/list carries every known actor, including humans that were just
    // invited to a private channel. agent/list only contributes live status.
    useEffect(() => {
      let alive = true;
      (async () => {
        try {
          const [al, ag] = await Promise.all([
            ipc.actorList().catch(() => ({ actors: [] as never[] })),
            ipc.agentList().catch(() => ({ agents: [] as never[] })),
          ]);
          if (!alive) return;
          if (al.actors.length > 0) {
            useActors.getState().upsertMany(al.actors);
          }
          if (ag.agents.length > 0) {
            useActors
              .getState()
              .upsertMany(ag.agents.map((a) => a.spec.actor));
            const next: Record<string, string> = {};
            for (const a of ag.agents) next[a.spec.actor.id] = a.status;
            setStatuses(next);
          }
        } catch {
          /* ignore — palette falls back to whatever's cached */
        }
      })();
      return () => {
        alive = false;
      };
    }, []);

    const f = filter.toLowerCase();
    const items = useMemo(() => {
      // In a private channel the pool is restricted to the channel's members
      // plus actors reachable via public channels. Public channels/threads
      // don't restrict — every known actor except self/system is addressable.
      const allow = resolveAllowedAgents(currentScope, channels, threadsByChannel);

      const list: Entry[] = [];
      for (const a of Object.values(actorsById)) {
        if (a.id === selfId) continue;
        if (a.id === "system") continue;
        if (allow && !allow.has(a.id)) continue;
        const display = a.displayName || a.id;
        if (
          !a.id.toLowerCase().includes(f) &&
          !display.toLowerCase().includes(f)
        ) {
          continue;
        }
        list.push({
          id: a.id,
          display,
          kind: a.kind,
          status: statuses[a.id],
        });
      }
      list.sort((x, y) => {
        const rank = actorKindRank(x.kind) - actorKindRank(y.kind);
        return rank || x.display.localeCompare(y.display);
      });
      return list;
    }, [actorsById, statuses, f, selfId, currentScope, channels, threadsByChannel]);

    useImperativeHandle(
      ref,
      () => ({
        pickFirst: () => {
          if (items.length === 0) return false;
          onPick(items[0].id);
          return true;
        },
      }),
      [items, onPick],
    );

    if (items.length === 0) return null;

    return (
      <div className="absolute bottom-full left-0 right-0 mb-2 max-h-72 overflow-y-auto rounded-md border border-border bg-elevated px-1 py-1 shadow-lg">
        <div className="px-2 pb-1 pt-1 text-[11px] font-semibold uppercase tracking-wide text-muted">
          Mention an actor · Enter to pick first
        </div>
        {items.map((e, i) => (
          <button
            key={e.id}
            className={
              "flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-hover " +
              (i === 0 ? "bg-hover/60" : "")
            }
            onClick={() => onPick(e.id)}
          >
            <span className={actorRoleClass(e.kind)}>@{e.display}</span>
            <span className="text-[11px] text-muted">{e.id}</span>
            {e.status && (
              <span className="ml-auto text-[11px] text-muted">{e.status}</span>
            )}
          </button>
        ))}
      </div>
    );
  },
);

function actorKindRank(kind: ActorKind) {
  if (kind === "agent") return 0;
  if (kind === "human") return 1;
  return 2;
}

function actorRoleClass(kind: ActorKind) {
  if (kind === "agent") return "text-role-agent";
  if (kind === "service") return "text-role-service";
  return "text-role-human";
}
