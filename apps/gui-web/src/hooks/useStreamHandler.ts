import { useCallback, useRef } from "react";
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
import { applyMessageToUsageStore, applyRunToUsageStore } from "@/store/usageStore";
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
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const applyChannelDeleted = useCallback(
    (channelId: string) => {
      const d = depsRef.current;
      const deletedThreads = d.threadsByChannel[channelId] ?? [];
      const deletedChannel =
        d.channels.find((channel) => channel.id === channelId) ?? null;
      const fallbackChannelId =
        d.channels.find(
          (channel) => channel.id !== channelId && !isDirectChannel(channel),
        )?.id ?? null;
      d.setChannels((current) =>
        current.filter((channel) => channel.id !== channelId),
      );
      const deletedDirectPeerId = directChannelPeerId(deletedChannel, d.actorIdRef.current);
      if (deletedDirectPeerId) {
        d.setDirectScopesByActorId((current) => {
          const next = { ...current };
          delete next[deletedDirectPeerId];
          return next;
        });
      }
      d.setThreadsByChannel((current) => {
        const next = { ...current };
        delete next[channelId];
        return next;
      });
      d.setTasks((current) => current.filter((task) => task.channelId !== channelId));
      d.setThreadStatsById((current) => {
        const next = { ...current };
        for (const thread of deletedThreads) delete next[thread.id];
        return next;
      });
      d.updateChannelGroups((current) =>
        current.map((group) => ({
          ...group,
          channelIds: group.channelIds.filter((id) => id !== channelId),
        })),
      );
      d.setActiveChannelId((current) =>
        current === channelId ? fallbackChannelId : current,
      );
      d.setActiveThreadId((current) =>
        current && deletedThreads.some((thread) => thread.id === current)
          ? null
          : current,
      );
      if (d.activeChannelId === channelId) {
        d.setChannelPanelTab(null);
        d.setReplyTo(null);
        d.setMessages(() => []);
        d.setThreadMessages(() => []);
      }
    },
    [],
  );

  const handleStream = useCallback(
    (update: StreamUpdate) => {
      const d = depsRef.current;
      switch (update.kind) {
        case "channel.created":
        case "channel.updated":
        case "channel.invited": {
          const channel = update.data.channel as Channel | undefined;
          if (channel) {
            d.setChannels((current) => sortChannels(upsert(current, channel)));
            const peerActorId = directChannelPeerId(channel, d.actorIdRef.current);
            if (peerActorId) {
              d.setDirectScopesByActorId((current) => ({
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
          d.setThreadsByChannel((current) => ({
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
          if (messageIsActionRequestFor(message, d.actorIdRef.current)) {
            d.setInbox((current) =>
              current.some((item) => item.delivery.sourceId === message.id)
                ? current
                : [
                    {
                      delivery: {
                        sourceId: message.id,
                        actorId: d.actorIdRef.current ?? "",
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
            d.activeScopeRef.current &&
            sameScope(update.scope, d.activeScopeRef.current)
          ) {
            d.setMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (message.scope.kind === "thread") {
            d.setThreadStatsById((current) => upsertThreadStatsMessage(current, message));
          }
          if (
            update.scope &&
            d.activeThreadScopeRef.current &&
            sameScope(update.scope, d.activeThreadScopeRef.current)
          ) {
            d.setThreadMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (
            (update.scope &&
              d.activeDirectScopeRef.current &&
              sameScope(update.scope, d.activeDirectScopeRef.current)) ||
            messageBelongsToDirectActor(
              message,
              d.activeDirectActorIdRef.current,
              d.actorIdRef.current,
            )
          ) {
            d.setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          return;
        }
        case "message.updated": {
          const message = update.data.message as Message | undefined;
          if (!message) return;
          applyMessageToUsageStore(message);
          if (
            update.scope &&
            d.activeScopeRef.current &&
            sameScope(update.scope, d.activeScopeRef.current)
          ) {
            d.setMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (message.scope.kind === "thread") {
            d.setThreadStatsById((current) => upsertThreadStatsMessage(current, message));
          }
          if (
            update.scope &&
            d.activeThreadScopeRef.current &&
            sameScope(update.scope, d.activeThreadScopeRef.current)
          ) {
            d.setThreadMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          if (
            (update.scope &&
              d.activeDirectScopeRef.current &&
              sameScope(update.scope, d.activeDirectScopeRef.current)) ||
            messageBelongsToDirectActor(
              message,
              d.activeDirectActorIdRef.current,
              d.actorIdRef.current,
            )
          ) {
            d.setDirectMessages((current) => sortMessages(upsertMessage(current, message)));
          }
          d.setInbox((current) =>
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
          if (run) {
            applyRunToUsageStore(run);
            d.setRuns((current) => ({ ...current, [run.id]: run }));
          }
          return;
        }
        case "task.changed": {
          const task = update.data.task as Task | undefined;
          if (task) d.setTasks((current) => sortTasks(upsert(current, task)));
          return;
        }
        case "task_assignment.changed": {
          const task = update.data.task as Task | undefined;
          if (task) d.setTasks((current) => sortTasks(upsert(current, task)));
          return;
        }
        case "delivery.updated": {
          const actorId = d.actorIdRef.current;
          if (actorId) void d.refreshInbox(actorId).catch(() => {});
          return;
        }
      }
    },
    [applyChannelDeleted],
  );

  return { handleStream, applyChannelDeleted };
}
