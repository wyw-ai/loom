import { useCallback, useEffect, useRef } from "react";
import * as ipc from "@/ipc/bridge";
import type { ChannelLayout } from "@/ipc/types";
import type { ChannelGroup, ConnectionState } from "@/lib/types";
import { ungroupedChannelGroupId } from "@/lib/constants";
import {
  hasDirtyChannelGroups,
  hasMigratedChannelGroups,
  loadChannelGroups,
  markChannelGroupsDirty,
  markChannelGroupsMigrated,
  normalizeChannelGroups,
  normalizeChannelLayout,
  saveChannelGroups,
} from "@/lib/channel-utils";

type ChannelGroupSetter = (
  groups: ChannelGroup[] | ((current: ChannelGroup[]) => ChannelGroup[]),
) => void;

interface UseChannelGroupsParams {
  channelGroups: ChannelGroup[];
  setChannelGroups: ChannelGroupSetter;
  channelGroupsKey: string;
  connection: ConnectionState;
  workspaceId?: string;
}

interface PendingWrite {
  sequence: number;
  sections: ChannelGroup[];
}

interface LayoutSyncSession {
  id: number;
  key: string;
  cancelled: boolean;
  ready: boolean;
  support: "unknown" | "supported" | "unsupported";
  revision: number;
  migrated: boolean;
  migrationBaseSections: ChannelGroup[] | null;
  migrationRebaseSections: ChannelGroup[] | null;
  mutationSequence: number;
  writing: boolean;
  queued: PendingWrite | null;
  bufferedRemote: ChannelLayout | null;
}

let nextSessionId = 1;

function createSession(key: string): LayoutSyncSession {
  return {
    id: nextSessionId++,
    key,
    cancelled: false,
    ready: false,
    support: "unknown",
    revision: -1,
    migrated: hasMigratedChannelGroups(key),
    migrationBaseSections: null,
    migrationRebaseSections: null,
    mutationSequence: 0,
    writing: false,
    queued: null,
    bufferedRemote: null,
  };
}

/**
 * Rebase edits made while the one-time merge is in flight onto the server
 * snapshot read immediately before migration. This mirrors the server's
 * merge semantics: existing server sections and assignments win, while new
 * local sections/channels are appended in their original order.
 */
function rebaseMigrationSections(
  serverSections: ChannelGroup[],
  latestLocalSections: ChannelGroup[],
) {
  const merged = normalizeChannelGroups(serverSections).map((section) => ({
    ...section,
    channelIds: [...section.channelIds],
  }));
  const sectionIndexes = new Map(
    merged.map((section, index) => [section.id, index]),
  );
  const assignedChannelIds = new Set(
    merged.flatMap((section) => section.channelIds),
  );

  for (const localSection of normalizeChannelGroups(latestLocalSections)) {
    const existingIndex = sectionIndexes.get(localSection.id);
    if (existingIndex !== undefined) {
      const existing = merged[existingIndex]!;
      for (const channelId of localSection.channelIds) {
        if (assignedChannelIds.has(channelId)) continue;
        assignedChannelIds.add(channelId);
        existing.channelIds.push(channelId);
      }
      continue;
    }

    const channelIds = localSection.channelIds.filter((channelId) => {
      if (assignedChannelIds.has(channelId)) return false;
      assignedChannelIds.add(channelId);
      return true;
    });
    sectionIndexes.set(localSection.id, merged.length);
    merged.push({ ...localSection, channelIds });
  }
  return merged;
}

/**
 * Keep sections that were present at GET time plus server-only sections that
 * appeared in the merge response. Sections introduced by the first local
 * migration write are deliberately excluded so a newer local snapshot can
 * still rename or delete them before the migration is finalized.
 */
function serverSectionsAfterMigrationMerge(
  getSections: ChannelGroup[],
  sentLocalSections: ChannelGroup[],
  mergedSections: ChannelGroup[],
) {
  const knownServerIds = new Set(
    normalizeChannelGroups(getSections).map((section) => section.id),
  );
  const sentLocalIds = new Set(
    normalizeChannelGroups(sentLocalSections).map((section) => section.id),
  );
  return normalizeChannelGroups(mergedSections).filter(
    (section) => knownServerIds.has(section.id) || !sentLocalIds.has(section.id),
  );
}

function channelGroupsEqual(left: ChannelGroup[], right: ChannelGroup[]) {
  if (left.length !== right.length) return false;
  return left.every((group, index) => {
    const other = right[index];
    return Boolean(other) &&
      group.id === other.id &&
      group.title === other.title &&
      group.collapsed === other.collapsed &&
      group.channelIds.length === other.channelIds.length &&
      group.channelIds.every((channelId, channelIndex) =>
        channelId === other.channelIds[channelIndex]);
  });
}

export function isUnsupportedChannelLayoutError(error: unknown) {
  if (
    error &&
    typeof error === "object" &&
    Number((error as { code?: unknown }).code) === -32601
  ) {
    return true;
  }
  const message = error instanceof Error
    ? error.message
    : error && typeof error === "object" && "message" in error
      ? String((error as { message?: unknown }).message)
      : String(error);
  return /(?:-32601|method not found|unknown method)/i.test(message);
}

export function useChannelGroups({
  channelGroups,
  setChannelGroups,
  channelGroupsKey,
  connection,
  workspaceId,
}: UseChannelGroupsParams) {
  const groupsRef = useRef(channelGroups);
  groupsRef.current = channelGroups;
  const connectionRef = useRef(connection);
  connectionRef.current = connection;
  const sessionRef = useRef<LayoutSyncSession>(createSession(channelGroupsKey));

  const applyGroups = useCallback((
    session: LayoutSyncSession,
    groups: ChannelGroup[],
    dirty: boolean,
  ) => {
    if (session.cancelled || sessionRef.current !== session) return;
    const next = normalizeChannelGroups(groups);
    groupsRef.current = next;
    saveChannelGroups(session.key, next);
    markChannelGroupsDirty(session.key, dirty);
    setChannelGroups(next);
  }, [setChannelGroups]);

  const applyServerLayout = useCallback((
    session: LayoutSyncSession,
    value: unknown,
    clearDirty = true,
  ) => {
    if (session.cancelled || sessionRef.current !== session) return false;
    const layout = normalizeChannelLayout(value);
    if (!layout) return false;
    session.revision = Math.max(session.revision, layout.revision);
    applyGroups(session, layout.sections, clearDirty ? false : hasDirtyChannelGroups(session.key));
    return true;
  }, [applyGroups]);

  const flushBufferedRemote = useCallback((session: LayoutSyncSession) => {
    if (
      session.cancelled ||
      sessionRef.current !== session ||
      session.writing ||
      session.queued ||
      hasDirtyChannelGroups(session.key)
    ) {
      return;
    }
    const buffered = session.bufferedRemote;
    session.bufferedRemote = null;
    if (buffered && buffered.revision > session.revision) {
      applyServerLayout(session, buffered);
    }
  }, [applyServerLayout]);

  const drainWrites = useCallback((session: LayoutSyncSession) => {
    if (
      session.cancelled ||
      sessionRef.current !== session ||
      session.writing ||
      !session.ready ||
      session.support === "unsupported" ||
      connectionRef.current !== "open"
    ) {
      return;
    }

    session.writing = true;
    void (async () => {
      let failed = false;
      while (
        !session.cancelled &&
        sessionRef.current === session &&
        connectionRef.current === "open" &&
        session.queued
      ) {
        const write = session.queued;
        session.queued = null;
        const migration = !session.migrated;
        try {
          const result = await ipc.channelLayoutSet({
            sections: write.sections,
            ...(migration ? { merge: true } : {}),
          });
          if (session.cancelled || sessionRef.current !== session) return;
          const layout = normalizeChannelLayout(result.layout);
          if (!layout) throw new Error("channel.layout.set returned an invalid layout");
          session.support = "supported";
          session.revision = Math.max(session.revision, layout.revision);
          if (migration) {
            const queuedDuringMigration = session.queued as PendingWrite | null;
            if (queuedDuringMigration) {
              const serverSections = session.migrationBaseSections
                ? serverSectionsAfterMigrationMerge(
                    session.migrationBaseSections,
                    write.sections,
                    layout.sections,
                  )
                : layout.sections;
              const rebasedSections = rebaseMigrationSections(
                serverSections,
                queuedDuringMigration.sections,
              );
              session.migrated = true;
              session.migrationRebaseSections = serverSections;
              session.queued = {
                ...queuedDuringMigration,
                sections: rebasedSections,
              };
            }
          }

          const isLatest =
            !session.queued && write.sequence === session.mutationSequence;
          if (isLatest) {
            applyGroups(session, layout.sections, false);
            if (migration || session.migrationRebaseSections) {
              session.migrated = true;
              markChannelGroupsMigrated(session.key);
            }
            session.migrationBaseSections = null;
            session.migrationRebaseSections = null;
          }
        } catch (error) {
          if (session.cancelled || sessionRef.current !== session) return;
          session.support = isUnsupportedChannelLayoutError(error)
            ? "unsupported"
            : "unknown";
          const queuedAfterFailure = session.queued as PendingWrite | null;
          if (!queuedAfterFailure || queuedAfterFailure.sequence < write.sequence) {
            session.queued = write;
          }
          failed = true;
          break;
        }
      }

      session.writing = false;
      if (!failed) flushBufferedRemote(session);
    })();
  }, [applyGroups, flushBufferedRemote]);

  const queueWrite = useCallback((session: LayoutSyncSession, groups: ChannelGroup[]) => {
    if (session.cancelled || sessionRef.current !== session) return;
    const sequence = ++session.mutationSequence;
    const sections = normalizeChannelGroups(groups);
    session.queued = {
      sequence,
      sections: session.migrationRebaseSections
        ? rebaseMigrationSections(session.migrationRebaseSections, sections)
        : sections,
    };
    drainWrites(session);
  }, [drainWrites]);

  const applyRemoteChannelLayout = useCallback((value: unknown) => {
    const layout = normalizeChannelLayout(value);
    if (!layout) return;
    const session = sessionRef.current;
    if (session.cancelled) return;
    session.support = "supported";

    if (
      !session.ready ||
      session.writing ||
      session.queued ||
      hasDirtyChannelGroups(session.key)
    ) {
      if (!session.bufferedRemote || layout.revision > session.bufferedRemote.revision) {
        session.bufferedRemote = layout;
      }
      return;
    }
    if (layout.revision > session.revision) applyServerLayout(session, layout);
  }, [applyServerLayout]);

  // Workspace switch: show the workspace-specific cache immediately. It is
  // the complete fallback for offline and pre-channel.layout servers.
  useEffect(() => {
    const previous = sessionRef.current;
    previous.cancelled = true;
    const session = createSession(channelGroupsKey);
    session.ready = connection !== "open";
    session.support = connection === "open" ? "unknown" : "unsupported";
    sessionRef.current = session;
    const cached = loadChannelGroups(channelGroupsKey);
    groupsRef.current = cached;
    setChannelGroups(cached);
    return () => {
      session.cancelled = true;
    };
  }, [channelGroupsKey, setChannelGroups]);

  // Each live connection gets a fresh synchronization session, invalidating
  // late responses from the previous socket generation.
  useEffect(() => {
    const previous = sessionRef.current;
    previous.cancelled = true;
    const session = createSession(channelGroupsKey);
    sessionRef.current = session;

    if (connection !== "open" || !workspaceId) {
      session.ready = true;
      session.support = "unsupported";
      return () => {
        session.cancelled = true;
      };
    }

    void (async () => {
      try {
        const result = await ipc.channelLayoutGet();
        if (session.cancelled || sessionRef.current !== session) return;
        const serverLayout = normalizeChannelLayout(result.layout);
        if (!serverLayout) throw new Error("channel.layout.get returned an invalid layout");
        session.support = "supported";
        session.revision = serverLayout.revision;
        session.ready = true;
        if (!session.migrated) {
          session.migrationBaseSections = serverLayout.sections;
        }

        const local = normalizeChannelGroups(groupsRef.current);
        const needsMigration =
          !session.migrated && local.length > 0;
        const dirty = hasDirtyChannelGroups(session.key);
        if (needsMigration || dirty || session.queued) {
          queueWrite(session, session.queued?.sections ?? local);
          return;
        }

        applyServerLayout(session, serverLayout);
        if (!session.migrated) {
          session.migrated = true;
          markChannelGroupsMigrated(session.key);
        }
        flushBufferedRemote(session);
      } catch (error) {
        if (session.cancelled || sessionRef.current !== session) return;
        session.ready = true;
        session.support = isUnsupportedChannelLayoutError(error)
          ? "unsupported"
          : "unknown";
        if (session.support !== "unsupported" && hasDirtyChannelGroups(session.key)) {
          queueWrite(session, groupsRef.current);
        }
      }
    })();

    return () => {
      session.cancelled = true;
    };
  }, [
    applyServerLayout,
    channelGroupsKey,
    connection,
    flushBufferedRemote,
    queueWrite,
    workspaceId,
  ]);

  const updateChannelGroups = useCallback(
    (updater: (current: ChannelGroup[]) => ChannelGroup[]) => {
      const current = groupsRef.current;
      const next = normalizeChannelGroups(updater(current));
      if (channelGroupsEqual(current, next)) return;
      groupsRef.current = next;
      saveChannelGroups(channelGroupsKey, next);
      markChannelGroupsDirty(channelGroupsKey, true);
      setChannelGroups(next);
      const session = sessionRef.current;
      if (session.key === channelGroupsKey && connectionRef.current === "open") {
        queueWrite(session, next);
      }
    },
    [channelGroupsKey, queueWrite, setChannelGroups],
  );

  // Channel deletion/revocation is already enforced by the server's layout
  // visibility filter. Keep the local cache tidy without turning that cleanup
  // into an authoritative full-layout write that could overwrite a newer
  // edit made by another client.
  const removeChannelFromGroupsLocally = useCallback((channelId: string) => {
    const current = groupsRef.current;
    const next = normalizeChannelGroups(current.map((group) => ({
      ...group,
      channelIds: group.channelIds.filter((id) => id !== channelId),
    })));
    if (channelGroupsEqual(current, next)) return;
    groupsRef.current = next;
    saveChannelGroups(channelGroupsKey, next);
    setChannelGroups(next);
  }, [channelGroupsKey, setChannelGroups]);

  const addChannelGroup = useCallback(
    (title: string) => {
      const trimmed = title.trim();
      if (!trimmed) return;
      updateChannelGroups((current) => [
        ...current,
        {
          id: `local-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
          title: trimmed,
          channelIds: [],
          collapsed: false,
        },
      ]);
    },
    [updateChannelGroups],
  );

  const renameChannelGroup = useCallback(
    (groupId: string, title: string) => {
      const trimmed = title.trim();
      if (!trimmed) return;
      updateChannelGroups((current) =>
        current.map((item) =>
          item.id === groupId ? { ...item, title: trimmed } : item,
        ),
      );
    },
    [updateChannelGroups],
  );

  const removeChannelGroup = useCallback(
    (groupId: string) => {
      updateChannelGroups((current) => current.filter((item) => item.id !== groupId));
    },
    [updateChannelGroups],
  );

  const toggleChannelGroup = useCallback(
    (groupId: string) => {
      updateChannelGroups((current) =>
        current.map((item) =>
          item.id === groupId ? { ...item, collapsed: !item.collapsed } : item,
        ),
      );
    },
    [updateChannelGroups],
  );

  const moveChannelToGroup = useCallback(
    (channelId: string, groupId: string) => {
      updateChannelGroups((current) =>
        current.map((group) => {
          const channelIds = group.channelIds.filter((id) => id !== channelId);
          if (group.id === groupId && groupId !== ungroupedChannelGroupId) {
            channelIds.push(channelId);
            return { ...group, channelIds, collapsed: false };
          }
          return { ...group, channelIds };
        }),
      );
    },
    [updateChannelGroups],
  );

  return {
    updateChannelGroups,
    removeChannelFromGroupsLocally,
    applyRemoteChannelLayout,
    addChannelGroup,
    renameChannelGroup,
    removeChannelGroup,
    toggleChannelGroup,
    moveChannelToGroup,
  };
}
