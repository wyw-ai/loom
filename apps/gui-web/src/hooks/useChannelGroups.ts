import { useCallback } from "react";
import type { ChannelGroup } from "@/lib/types";
import { ungroupedChannelGroupId } from "@/lib/constants";
import { normalizeChannelGroups, saveChannelGroups } from "@/lib/channel-utils";

interface UseChannelGroupsParams {
  channelGroups: ChannelGroup[];
  setChannelGroups: (updater: (current: ChannelGroup[]) => ChannelGroup[]) => void;
  channelGroupsKey: string;
}

export function useChannelGroups({
  setChannelGroups,
  channelGroupsKey,
}: UseChannelGroupsParams) {
  const updateChannelGroups = useCallback(
    (updater: (current: ChannelGroup[]) => ChannelGroup[]) => {
      setChannelGroups((current) => {
        const next = normalizeChannelGroups(updater(current));
        saveChannelGroups(channelGroupsKey, next);
        return next;
      });
    },
    [channelGroupsKey, setChannelGroups],
  );

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
    addChannelGroup,
    renameChannelGroup,
    removeChannelGroup,
    toggleChannelGroup,
    moveChannelToGroup,
  };
}
