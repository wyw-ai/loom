import type { ChannelGroup, ChannelGroupSection, ThreadWithChannel } from "@/lib/types";
import type {
  Actor,
  Channel,
  Message,
  ScopeRef,
  Task,
  Thread,
  Workspace,
} from "@/ipc/types";
import { ungroupedChannelGroupId } from "@/lib/constants";

// ---------------------------------------------------------------------------
// Channel group persistence
// ---------------------------------------------------------------------------

export function channelGroupStorageKey(workspace: Workspace | null) {
  return `loom:channel-groups:v1:${workspace?.id ?? "global"}`;
}

export function loadChannelGroups(key: string): ChannelGroup[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? normalizeChannelGroups(JSON.parse(raw)) : [];
  } catch {
    return [];
  }
}

export function saveChannelGroups(key: string, groups: ChannelGroup[]) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(key, JSON.stringify(groups));
  } catch {
    /* local-only preference; ignore quota or privacy-mode failures */
  }
}

export function normalizeChannelGroups(value: unknown): ChannelGroup[] {
  if (!Array.isArray(value)) return [];
  const seenGroupIds = new Set<string>();
  return value.flatMap((item, index) => {
    if (!item || typeof item !== "object") return [];
    const candidate = item as Partial<ChannelGroup>;
    const rawId =
      typeof candidate.id === "string" && candidate.id.trim()
        ? candidate.id.trim()
        : `local-${index}`;
    const id = seenGroupIds.has(rawId) ? `${rawId}-${index}` : rawId;
    seenGroupIds.add(id);
    const title =
      typeof candidate.title === "string" && candidate.title.trim()
        ? candidate.title.trim()
        : "Untitled";
    const channelIds = Array.isArray(candidate.channelIds)
      ? Array.from(
          new Set(
            candidate.channelIds.filter(
              (channelId): channelId is string =>
                typeof channelId === "string" && channelId.length > 0,
            ),
          ),
        )
      : [];
    return [
      {
        id,
        title,
        channelIds,
        collapsed: Boolean(candidate.collapsed),
      },
    ];
  });
}

export function channelGroupSections(
  groups: ChannelGroup[],
  channels: Channel[],
): ChannelGroupSection[] {
  const channelsById = new Map(channels.map((channel) => [channel.id, channel]));
  const assigned = new Set<string>();
  const sections: ChannelGroupSection[] = groups.map((group) => {
    const groupChannels = group.channelIds.flatMap((channelId) => {
      const channel = channelsById.get(channelId);
      if (!channel || assigned.has(channel.id)) return [];
      assigned.add(channel.id);
      return [channel];
    });
    return {
      id: group.id,
      title: group.title,
      channels: groupChannels,
      collapsed: group.collapsed,
      local: true,
    };
  });
  const ungroupedChannels = channels.filter((channel) => !assigned.has(channel.id));
  if (groups.length === 0 || ungroupedChannels.length > 0) {
    sections.push({
      id: ungroupedChannelGroupId,
      title: groups.length === 0 ? "Channels" : "Ungrouped",
      channels: ungroupedChannels,
      collapsed: false,
      local: false,
    });
  }
  return sections;
}

// ---------------------------------------------------------------------------
// Thread helpers
// ---------------------------------------------------------------------------

export function flattenThreads(
  threadsByChannel: Record<string, Thread[]>,
  channels: Channel[],
): ThreadWithChannel[] {
  const channelsById = new Map(channels.map((channel) => [channel.id, channel]));
  return Object.values(threadsByChannel)
    .flatMap((threads) =>
      threads.flatMap((thread) => {
        const channel = channelsById.get(thread.channelId);
        return channel ? [{ ...thread, channel }] : [];
      }),
    )
    .sort((a, b) => a.title.localeCompare(b.title));
}

// ---------------------------------------------------------------------------
// Channel sorting
// ---------------------------------------------------------------------------

export function sortChannels(items: Channel[]) {
  return [...items].sort((a, b) => a.title.localeCompare(b.title));
}

// ---------------------------------------------------------------------------
// Direct-message helpers
// ---------------------------------------------------------------------------

export function directMessageTarget(actorId: string) {
  return `dm:@${actorId}`;
}

export function directTargetActorId(target: string) {
  const raw = target.trim().match(/^dm:@?([^:]+)$/)?.[1];
  return raw?.trim() || null;
}

export function isDirectChannel(channel: Channel | null | undefined) {
  return Boolean(
    channel &&
      channel.title.startsWith("dm:") &&
      channel.members.length === 2 &&
      channel.visibility === "private",
  );
}

export function directChannelPeerId(
  channel: Channel | null | undefined,
  currentActorId: string | null | undefined,
) {
  if (!channel || !isDirectChannel(channel) || !currentActorId) return null;
  if (!channel.members.includes(currentActorId)) return null;
  return channel.members.find((actorId) => actorId !== currentActorId) ?? null;
}

export function directScopeForActor(
  channels: Channel[],
  currentActorId: string,
  peerActorId: string,
): ScopeRef | null {
  const channel = channels.find(
    (candidate) => directChannelPeerId(candidate, currentActorId) === peerActorId,
  );
  return channel ? { kind: "channel", id: channel.id } : null;
}

export function messageBelongsToDirectActor(
  message: Message,
  peerActorId: string | null,
  currentActorId: string | null,
) {
  if (!peerActorId || !directTargetActorId(message.target)) return false;
  if (message.authorActorId === peerActorId) return true;
  if (currentActorId && message.authorActorId !== currentActorId) return false;
  return directTargetActorId(message.target) === peerActorId;
}

export function directPeerForMessage(message: Message, currentActorId: string | null) {
  const targetActorId = directTargetActorId(message.target);
  if (!targetActorId) return null;
  if (currentActorId && message.authorActorId !== currentActorId) {
    return message.authorActorId;
  }
  return targetActorId;
}

// ---------------------------------------------------------------------------
// Channel mention helpers
// ---------------------------------------------------------------------------

export function channelMentionActors(channel: Channel, actors: Record<string, Actor>) {
  return channel.members
    .map((actorId) => actors[actorId])
    .filter((actor): actor is Actor => Boolean(actor) && actor.kind !== "service");
}

export function channelMentionAgentActors(channel: Channel, actors: Record<string, Actor>) {
  return channelMentionActors(channel, actors).filter(
    (actor) => actor.kind === "agent",
  );
}

export function isChannelMentionActor(channel: Channel, actorId: string) {
  return channel.members.includes(actorId);
}

// ---------------------------------------------------------------------------
// Channel membership helpers
// ---------------------------------------------------------------------------

export function isExplicitChannelMember(channel: Channel, actorId: string) {
  return channel.members.includes(actorId);
}

export function canAddChannelMember(channel: Channel, actor: Actor) {
  return actor.kind !== "service" && !isExplicitChannelMember(channel, actor.id);
}

export function canRemoveChannelMember(channel: Channel, actorId: string) {
  if (!isExplicitChannelMember(channel, actorId)) return false;
  if (channel.visibility === "public") return true;
  return channel.members[0] !== actorId;
}

// ---------------------------------------------------------------------------
// Channel task helpers
// ---------------------------------------------------------------------------

export function channelTaskGroups(tasks: Task[]) {
  const groups = [
    {
      id: "todo",
      title: "To Do",
      tasks: tasks.filter((task) => task.status === "todo" || task.status === "claimed"),
    },
    {
      id: "active",
      title: "In Progress",
      tasks: tasks.filter(
        (task) => task.status === "in_progress" || task.status === "waiting_review",
      ),
    },
    {
      id: "done",
      title: "Done",
      tasks: tasks.filter((task) => task.status === "done"),
    },
    {
      id: "other",
      title: "Other",
      tasks: tasks.filter(
        (task) =>
          task.status === "failed" ||
          task.status === "canceled" ||
          ![
            "todo",
            "claimed",
            "in_progress",
            "waiting_review",
            "done",
          ].includes(task.status),
      ),
    },
  ];
  return groups.filter((group) => group.tasks.length > 0);
}

export function taskProgressPercent(task: Task) {
  if (task.status === "in_progress") return 60;
  if (task.status === "waiting_review") return 85;
  if (task.status === "done") return 100;
  return null;
}

// ---------------------------------------------------------------------------
// Scope helpers
// ---------------------------------------------------------------------------

export function sameScope(a: ScopeRef, b: ScopeRef) {
  return a.kind === b.kind && a.id === b.id;
}

export function channelTopic(channel: Channel | null | undefined) {
  return typeof channel?.topic === "string" ? channel.topic.trim() : "";
}
