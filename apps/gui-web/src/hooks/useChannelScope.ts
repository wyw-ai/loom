import { useEffect } from "react";
import * as ipc from "@/ipc/bridge";
import { scopeKey, type Actor, type Channel, type ChannelMemberConfig, type Message, type MessageContextResult, type ScopeRef, type Thread } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";
import type { ThreadActivityStats } from "@/lib/types";
import { errorText, sortThreads } from "@/lib/format-utils";
import { normalizeMessage, sortMessages, threadStatsFromMessages } from "@/lib/message-utils";
import { sameScope } from "@/lib/channel-utils";

function contextMessagesForScope(
  context: MessageContextResult | null,
  scope: ScopeRef | null,
): Message[] {
  if (!context || !scope || !sameScope(context.anchor.scope, scope)) return [];
  return sortMessages(
    [...context.before, context.anchor, ...context.after].map(normalizeMessage),
  );
}

function mergeMessages(context: Message[], loaded: Message[]): Message[] {
  const byId = new Map<string, Message>();
  for (const message of context) byId.set(message.id, message);
  for (const message of loaded) byId.set(message.id, message);
  return sortMessages([...byId.values()]);
}

export interface ChannelScopeDeps {
  connection: ConnectionState;
  activeChannel: Channel | null;
  visibleChannels: Channel[];
  channelIdsKey: string;
  activeScope: ScopeRef | null;
  target: string | null;
  activeThreadScope: ScopeRef | null;
  threadMessageTarget: string | null;
  activeDirectActor: { id: string } | null;
  activeDirectScope: ScopeRef | null;
  activeDirectTarget: string | null;
  activeScopeRef: React.MutableRefObject<ScopeRef | null>;
  activeThreadScopeRef: React.MutableRefObject<ScopeRef | null>;
  activeDirectScopeRef: React.MutableRefObject<ScopeRef | null>;
  pendingMessageContextRef: React.MutableRefObject<MessageContextResult | null>;
  setActors: (
    updater: (current: Record<string, Actor>) => Record<string, Actor>,
  ) => void;
  setChannelMemberConfigsByChannel: React.Dispatch<React.SetStateAction<Record<string, Record<string, ChannelMemberConfig>>>>;
  setThreadsByChannel: (
    updater: (current: Record<string, Thread[]>) => Record<string, Thread[]>,
  ) => void;
  setMessages: (updater: Message[] | ((prev: Message[]) => Message[])) => void;
  setThreadMessages: (updater: Message[] | ((prev: Message[]) => Message[])) => void;
  setDirectMessages: (updater: Message[] | ((prev: Message[]) => Message[])) => void;
  setThreadStatsById: (
    updater: (current: Record<string, ThreadActivityStats>) => Record<string, ThreadActivityStats>,
  ) => void;
  setError: (err: string | null) => void;
}

export function useChannelScope(deps: ChannelScopeDeps) {
  const {
    connection,
    activeChannel,
    visibleChannels,
    channelIdsKey,
    activeScope,
    target,
    activeThreadScope,
    threadMessageTarget,
    activeDirectActor,
    activeDirectScope,
    activeDirectTarget,
    activeScopeRef,
    activeThreadScopeRef,
    activeDirectScopeRef,
    pendingMessageContextRef,
    setActors,
    setChannelMemberConfigsByChannel,
    setThreadsByChannel,
    setMessages,
    setThreadMessages,
    setDirectMessages,
    setThreadStatsById,
    setError,
  } = deps;

  // Load channel members, member configs, and threads when active channel changes
  useEffect(() => {
    if (!activeChannel || connection !== "open") return;
    let alive = true;
    void ipc
      .channelMembers(activeChannel.id)
      .then((result) => {
        if (!alive) return;
        setActors((current) => {
          const next = { ...current };
          for (const actor of result.members) next[actor.id] = actor;
          return next;
        });
      })
      .catch(() => {});
    void ipc
      .channelMemberConfigList(activeChannel.id)
      .then((result) => {
        if (!alive) return;
        setChannelMemberConfigsByChannel((current) => ({
          ...current,
          [activeChannel.id]: Object.fromEntries(
            result.configs.map((config) => [config.actorId, config]),
          ),
        }));
      })
      .catch(() => {});
    void ipc
      .threadList(activeChannel.id)
      .then((result) => {
        if (!alive) return;
        setThreadsByChannel((current) => ({
          ...current,
          [activeChannel.id]: sortThreads(result.threads),
        }));
      })
      .catch((err) => {
        if (alive) setError(errorText(err));
      });
    return () => {
      alive = false;
    };
  }, [activeChannel?.id, activeChannel?.members.join("|"), connection]);

  // Load threads for all visible channels
  useEffect(() => {
    if (connection !== "open" || visibleChannels.length === 0) return;
    let alive = true;
    void Promise.allSettled(
      visibleChannels.map((channel) =>
        ipc.threadList(channel.id).then((result) => ({
          channelId: channel.id,
          threads: result.threads,
        })),
      ),
    ).then((results) => {
      if (!alive) return;
      setThreadsByChannel((current) => {
        const next = { ...current };
        for (const result of results) {
          if (result.status === "fulfilled") {
            next[result.value.channelId] = sortThreads(result.value.threads);
          }
        }
        return next;
      });
    });
    return () => {
      alive = false;
    };
  }, [channelIdsKey, connection]);

  // Subscribe to active scope and load messages
  useEffect(() => {
    activeScopeRef.current = activeScope;
    if (!activeScope || !target || connection !== "open") {
      setMessages([]);
      return;
    }

    let alive = true;
    const contextMessages = contextMessagesForScope(
      pendingMessageContextRef.current,
      activeScope,
    );
    setMessages(contextMessages);
    void ipc.scopeSubscribe(activeScope).catch(() => {});
    void ipc
      .messageList({ target, limit: 150 })
      .then((result) => {
        if (!alive) return;
        setError(null);
        setMessages(
          mergeMessages(
            contextMessages,
            result.messages.map(normalizeMessage),
          ),
        );
      })
      .catch((err) => {
        if (alive) setError(errorText(err));
      });

    return () => {
      alive = false;
      void ipc.scopeUnsubscribe(activeScope).catch(() => {});
    };
  }, [activeScope ? scopeKey(activeScope) : null, connection, target]);

  // Subscribe to active thread scope and load thread messages
  useEffect(() => {
    activeThreadScopeRef.current = activeThreadScope;
    if (!activeThreadScope || !threadMessageTarget || connection !== "open") {
      setThreadMessages([]);
      return;
    }

    let alive = true;
    const contextMessages = contextMessagesForScope(
      pendingMessageContextRef.current,
      activeThreadScope,
    );
    setThreadMessages(contextMessages);
    void ipc.scopeSubscribe(activeThreadScope).catch(() => {});
    void ipc
      .messageList({ target: threadMessageTarget, limit: 100 })
      .then((result) => {
        if (!alive) return;
        setError(null);
        const sorted = mergeMessages(
          contextMessages,
          result.messages.map(normalizeMessage),
        );
        setThreadMessages(sorted);
        setThreadStatsById((current) => ({
          ...current,
          [activeThreadScope.id]: threadStatsFromMessages(
            sorted,
            result.pageInfo?.hasMore ?? false,
          ),
        }));
      })
      .catch((err) => {
        if (alive) setError(errorText(err));
      });

    return () => {
      alive = false;
      void ipc.scopeUnsubscribe(activeThreadScope).catch(() => {});
    };
  }, [
    activeThreadScope ? scopeKey(activeThreadScope) : null,
    connection,
    threadMessageTarget,
  ]);

  // Subscribe to active direct scope and load direct messages
  useEffect(() => {
    activeDirectScopeRef.current = activeDirectScope;
    if (!activeDirectActor || !activeDirectTarget || connection !== "open") {
      setDirectMessages([]);
      return;
    }
    if (!activeDirectScope) {
      setDirectMessages([]);
      return;
    }

    let alive = true;
    const contextMessages = contextMessagesForScope(
      pendingMessageContextRef.current,
      activeDirectScope,
    );
    setDirectMessages(contextMessages);
    void ipc.scopeSubscribe(activeDirectScope).catch(() => {});
    void ipc
      .messageList({ target: activeDirectTarget, limit: 150 })
      .then((result) => {
        if (!alive) return;
        setError(null);
        setDirectMessages(
          mergeMessages(
            contextMessages,
            result.messages.map(normalizeMessage),
          ),
        );
      })
      .catch((err) => {
        if (alive) setError(errorText(err));
      });

    return () => {
      alive = false;
      void ipc.scopeUnsubscribe(activeDirectScope).catch(() => {});
    };
  }, [
    activeDirectActor?.id,
    activeDirectScope ? scopeKey(activeDirectScope) : null,
    activeDirectTarget,
    connection,
  ]);
}
