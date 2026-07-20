import type {
  Channel,
  MachineInfo,
  ScopeRef,
  Workspace,
  Actor,
  Task,
  Thread,
  ChannelMemberConfig,
} from "@/ipc/types";
import { channelTarget, threadTarget } from "@/ipc/types";
import {
  channelGroupStorageKey,
  directMessageTarget,
  directScopeForActor,
  flattenThreads,
  isDirectChannel,
} from "@/lib/channel-utils";
import { displayName, findAgentMemberEntry, uniqueActorsById } from "@/lib/format-utils";
import type { ThreadWithChannel } from "@/lib/types";

export interface DerivedStateParams {
  channels: Channel[];
  activeChannelId: string | null;
  activeThreadId: string | null;
  activeDirectActorId: string | null;
  threadsByChannel: Record<string, import("@/ipc/types").Thread[]>;
  tasks: Task[];
  actors: Record<string, Actor>;
  machines: MachineInfo[];
  workspace: Workspace | null;
  directScopesByActorId: Record<string, ScopeRef>;
  channelMemberConfigsByChannel: Record<string, Record<string, ChannelMemberConfig>>;
}

export function deriveVisibleChannels(channels: Channel[]): Channel[] {
  return channels.filter((channel) => !isDirectChannel(channel));
}

export function deriveActiveChannel(
  visibleChannels: Channel[],
  activeChannelId: string | null,
): Channel | null {
  return visibleChannels.find((channel) => channel.id === activeChannelId) ?? null;
}

export function deriveChannelThreads(
  activeChannel: Channel | null,
  threadsByChannel: Record<string, Thread[]>,
): Thread[] {
  return activeChannel ? threadsByChannel[activeChannel.id] ?? [] : [];
}

export function deriveActiveThread(
  channelThreads: Thread[],
  activeThreadId: string | null,
): Thread | null {
  return channelThreads.find((thread) => thread.id === activeThreadId) ?? null;
}

export function deriveActiveScope(activeChannel: Channel | null): ScopeRef | null {
  return activeChannel ? { kind: "channel", id: activeChannel.id } : null;
}

export function deriveActiveThreadScope(
  activeThread: Thread | null,
): ScopeRef | null {
  return activeThread ? { kind: "thread", id: activeThread.id } : null;
}

export function deriveAllThreads(
  threadsByChannel: Record<string, Thread[]>,
  visibleChannels: Channel[],
): ThreadWithChannel[] {
  return flattenThreads(threadsByChannel, visibleChannels);
}

export function deriveActorList(actors: Record<string, Actor>): Actor[] {
  return Object.values(actors).sort((a, b) =>
    displayName(a).localeCompare(displayName(b)),
  );
}

export function deriveAgentActors(actorList: Actor[]): Actor[] {
  return actorList.filter((actor) => actor.kind === "agent");
}

export function deriveActiveDirectActor(
  agentActors: Actor[],
  activeDirectActorId: string | null,
  machines: MachineInfo[],
): Actor | null {
  return (
    agentActors.find((actor) => actor.id === activeDirectActorId) ??
    (activeDirectActorId
      ? findAgentMemberEntry(machines, activeDirectActorId)?.agent.spec.actor ?? null
      : null)
  );
}

export function deriveActiveDirectScope(
  activeDirectActor: Actor | null,
  workspace: Workspace | null,
  channels: Channel[],
  directScopesByActorId: Record<string, ScopeRef>,
): ScopeRef | null {
  return activeDirectActor && workspace
    ? directScopeForActor(channels, workspace.actorId, activeDirectActor.id) ??
        directScopesByActorId[activeDirectActor.id] ??
        null
    : null;
}

export function deriveMemberCandidates(actorList: Actor[]): Actor[] {
  return uniqueActorsById(actorList.filter((actor) => actor.kind !== "service"));
}

export function deriveTasksBySourceMessageId(tasks: Task[]): Record<string, Task> {
  return Object.fromEntries(tasks.map((task) => [task.sourceMessageId, task]));
}

export function deriveChannelTasks(
  activeChannel: Channel | null,
  tasks: Task[],
): Task[] {
  return activeChannel
    ? tasks.filter((task) => task.channelId === activeChannel.id)
    : tasks;
}

export function deriveChannelGroupsKey(workspace: Workspace | null): string {
  return channelGroupStorageKey(workspace);
}

export function deriveActiveDirectTarget(activeDirectActor: Actor | null): string | null {
  return activeDirectActor ? directMessageTarget(activeDirectActor.id) : null;
}

export function deriveTarget(activeChannel: Channel | null): string | null {
  return activeChannel ? channelTarget(activeChannel.id) : null;
}

export function deriveThreadMessageTarget(
  activeThread: Thread | null,
): string | null {
  return activeThread ? threadTarget(activeThread) : null;
}
