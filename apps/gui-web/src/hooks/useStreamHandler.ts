import { useCallback } from "react";
import type {
  Channel,
  Message,
  Run,
  StreamUpdate,
  Task,
  Thread,
  ScopeRef,
  InboxListEntry,
} from "@/ipc/types";
import { applyMessageToUsageStore } from "@/store/usageStore";
import {
  directChannelPeerId,
  isDirectChannel,
  messageBelongsToDirectActor,
  sameScope,
  sortChannels,
} from "@/lib/channel-utils";
import {
  messageIsActionRequestFor,
  normalizeMessage,
  sortMessages,
  upsertMessage,
  upsertThreadStatsMessage,
} from "@/lib/message-utils";
import { sortTasks, sortThreads, upsert } from "@/lib/format-utils";
import type { ChannelGroup, ChannelPanelTab, ThreadActivityStats } from "@/lib/types";

export interface StreamHandlerDeps {
  setChannels: (updater: (current: Channel[]) => Channel[]) => void;
  setDirectScopesByActorId: (
    updater: (current: Record<string, ScopeRef>) => Record<string, ScopeRef>,
  ) => void;
  setThreadsByChannel: (
    updater: (current: Record<string, Thread[]>) => Record<string, Thread[]>,
  ) => void;
  setMessages: (updater: (current: Message[]) => Message[]) => void;
  setThreadMessages: (updater: (current: Message[]) => Message[]) => void;
  setDirectMessages: (updater: (current: Message[]) => Message[]) => void;
  setThreadStatsById: (
    updater: (current: Record<string, ThreadActivityStats>) => Record<string, ThreadActivityStats>,
  ) => void;
  setRuns: (updater: (current: Record<string, Run>) => Record<string, Run>) => void;
  setTasks: (updater: (current: Task[]) => Task[]) => void;
  setInbox: (
    updater: (current: InboxListEntry[]) => InboxListEntry[],
  ) => void;
  setError: (error: string | null) => void;
  activeScopeRef: React.MutableRefObject<ScopeRef | null>;
  activeThreadScopeRef: React.MutableRefObject<ScopeRef | null>;
  activeDirectScopeRef: React.MutableRefObject<ScopeRef | null>;
  activeDirectActorIdRef: React.MutableRefObject<string | null>;
  actorIdRef: React.MutableRefObject<string | null>;
  threadsByChannel: Record<string, Thread[]>;
  channels: Channel[];
  activeChannelId: string | null;
  setChannelPanelTab: (tab: ChannelPanelTab | null) => void;
  setReplyTo: (reply: import("@/ipc/types").Message | null) => void;
  setActiveChannelId: (updater: (current: string | null) => string | null) => void;
  setActiveThreadId: (updater: (current: string | null) => string | null) => void;
  updateChannelGroups: (updater: (current: ChannelGroup[]) => ChannelGroup[]) => void;
  refreshInbox: (actorId: string) => Promise<void>;
}

export function useStreamHandler(deps: StreamHandlerDeps) {
  const applyChannelDeleted = useCallback(
    (channelId: string) => {
      const deletedThreads = deps.threadsByChannel[channelId] ?? [];
      const deletedChannel =
        deps.channels.find((channel) => channel.id === channelId) ?? null;
      const fallbackChannelId =
        deps.channels.find(
          (channel) => channel.id !== channelId && !isDirectChannel(channel),
        )?.id ?? null;
      deps.setChannels((current) =>
        current.filter((channel) => channel.id !== channelId),
      );
      const deletedDirectPeerId = directChannelPeerId(deletedChannel, deps.actorIdRef.current);
      if (deletedDirectPeerId) {
        deps.setDirectScopesByActorId((current) => {
          const next = { ...current };
          delete next[deletedDirectPeerId];
          return next;
        });
      }
      deps.setThreadsByChannel((current) => {
        const next = { ...current };
        delete next[channelId];
        return next;
      });
      deps.setTasks((current) => current.filter((task) => task.channelId !== channelId));
      deps.setThreadStatsById((current) => {
        const next = { ...current };
        for (const thread of deletedThreads) delete next[thread.id];
        return next;
      });
      deps.updateChannelGroups((current) =>
        current.map((group) => ({
          ...group,
          channelIds: group.channelIds.filter((id) => id !== channelId),
        })),
      );
      deps.setActiveChannelId((current) =>
        current === channelId ? fallbackChannelId : current,
      );
      deps.setActiveThreadId((current) =>
        current && deletedThreads.some((thread) => thread.id === current)
          ? null
          : current,
      );
      if (deps.activeChannelId === channelId) {
        deps.setChannelPanelTab(null);
        deps.setReplyTo(null);
        deps.setMessages(() => []);
        deps.setThreadMessages(() => []);
      }
    },
    [deps],
  );

  const handleStream = useCallback(
    (update: StreamUpdate) => {
      switch (update.kind) {
        case "channel.created":
        case "channel.updated":
        case "channel.invited": {
          const channel = update.data.channel as Channel | undefined;
          if (channel) {
            deps.setChannels((current) => sortChannels(upsert(current, channel)));
            const peerActorId = directChannelPeerId(channel, deps.actorIdRef.current);
            if (peerActorId) {
              deps.setDirectScopesByActorId((current) => ({
                ...current,
                [peerActorId]: { kind: "channel", id: channel.id },
              }));
            }
          }
          return;
        }
        case "channel.deleted": {
          const channelId = update.data.channelId as string | undefined;
          if (!channelId) return;
          applyChannelDeleted(channelId);
          return;
        }
        case "channel.revoked": {
          const channelId = update.data.channelId as string | undefined;
          if (!channelId) return;
          applyChannelDeleted(channelId);
          return;
        }
        case "thread.created":
        case "thread.updated": {
          const thread = update.data.thread as Thread | undefined;
          if (!thread) return;
          deps.setThreadsByChannel((current) => ({
            ...current,
            [thread.channelId]: sortThreads(
              upsert(current[thread.channelId] ?? [], thread).filter(
                (item) => !item.archivedAt,
              ),
            ),
          }));
          return;
        }
        case "message.created": {
          const message = update.data.message as Message | undefined;
          if (!message) return;
          applyMessageToUsageStore(message);
          if (messageIsActionRequestFor(message, deps.actorIdRef.current)) {
            deps.setInbox((current) =>
              current.some((item) => item.delivery.sourceId === message.id)
                ? current
                : [
                    {
                      delivery: {
                        sourceId: message.id,
                        actorId: deps.actorIdRef.current ?? "",
                        state: "pending",
                        updatedAt: message.createdAt,
                      },
                      message: normalizeMessage(message),
                    },
                    ...current,
                  ],
            );
          }
          if (
            update.scope &&
            deps.activeScopeRef.current &&
            sameScope(update.scope, deps.activeScopeRef.current)
          ) {
            deps.setMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (message.scope.kind === "thread") {
            deps.setThreadStatsById((current) => upsertThreadStatsMessage(current, message));
          }
          if (
            update.scope &&
            deps.activeThreadScopeRef.current &&
            sameScope(update.scope, deps.activeThreadScopeRef.current)
          ) {
            deps.setThreadMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (
            (update.scope &&
              deps.activeDirectScopeRef.current &&
              sameScope(update.scope, deps.activeDirectScopeRef.current)) ||
            messageBelongsToDirectActor(
              message,
              deps.activeDirectActorIdRef.current,
              deps.actorIdRef.current,
            )
          ) {
            deps.setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          return;
        }
        case "message.updated": {
          const message = update.data.message as Message | undefined;
          if (!message) return;
          applyMessageToUsageStore(message);
          if (
            update.scope &&
            deps.activeScopeRef.current &&
            sameScope(update.scope, deps.activeScopeRef.current)
          ) {
            deps.setMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (message.scope.kind === "thread") {
            deps.setThreadStatsById((current) => upsertThreadStatsMessage(current, message));
          }
          if (
            update.scope &&
            deps.activeThreadScopeRef.current &&
            sameScope(update.scope, deps.activeThreadScopeRef.current)
          ) {
            deps.setThreadMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (
            (update.scope &&
              deps.activeDirectScopeRef.current &&
              sameScope(update.scope, deps.activeDirectScopeRef.current)) ||
            messageBelongsToDirectActor(
              message,
              deps.activeDirectActorIdRef.current,
              deps.actorIdRef.current,
            )
          ) {
            deps.setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          deps.setInbox((current) =>
            current.map((item) =>
              item.delivery.sourceId === message.id
                ? { ...item, message: normalizeMessage(message) }
                : item,
            ),
          );
          return;
        }
        case "run.updated": {
          const run = update.data.run as Run | undefined;
          if (run) deps.setRuns((current) => ({ ...current, [run.id]: run }));
          return;
        }
        case "task.changed": {
          const task = update.data.task as Task | undefined;
          if (task) deps.setTasks((current) => sortTasks(upsert(current, task)));
          return;
        }
        case "task_assignment.changed": {
          const task = update.data.task as Task | undefined;
          if (task) deps.setTasks((current) => sortTasks(upsert(current, task)));
          return;
        }
        case "delivery.updated": {
          const actorId = deps.actorIdRef.current;
          if (actorId) void deps.refreshInbox(actorId).catch(() => {});
          return;
        }
      }
    },
    [deps, applyChannelDeleted],
  );

  return { handleStream, applyChannelDeleted };
}
