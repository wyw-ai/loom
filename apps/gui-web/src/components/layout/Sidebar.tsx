import { Fragment, useState, useEffect, useRef } from "react";
import type { FormEvent, MouseEvent, PointerEvent } from "react";
import { createPortal } from "react-dom";
import {
  ArrowRight,
  ChevronDown,
  ChevronRight,
  Copy,
  Folder,
  GripVertical,
  Hash,
  Loader2,
  LogOut,
  Menu,
  Pencil,
  Plus,
  Split,
  Trash2,
  Bell,
  Bot,
  Check,
  MessageCircle,
  MessageSquare,
  Play,
  QrCode,
  Server,
  UserPlus,
  X,
} from "lucide-react";
import type { Channel, HumanAccount, Thread, Workspace } from "@/ipc/types";
import type {
  ChannelGroup,
  ChannelGroupSection,
  ConnectionState,
  View,
} from "@/lib/types";
import {
  channelContextMenuViewportPaddingPx,
  channelContextMenuWidthPx,
  ungroupedChannelGroupId,
} from "@/lib/constants";
import { channelGroupSections } from "@/lib/channel-utils";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ChannelDeleteConfirm } from "@/components/channel/ChannelDeleteConfirm";
import { MobileConnectDialog } from "@/components/views/AccountView";
import { useUIStore } from "@/store/uiStore";
import { usePresence } from "@/hooks/usePresence";
import { useI18n } from "@/lib/i18n";

type SidebarContextMenu =
  | { kind: "blank"; x: number; y: number }
  | { kind: "channel"; channelId: string; x: number; y: number }
  | { kind: "section"; sectionId: string; x: number; y: number };

type SidebarDragKind = "channel" | "section";

type SidebarPointerDrag = {
  kind: SidebarDragKind;
  itemId: string;
  label: string;
  startX: number;
  startY: number;
  originLeft: number;
  originWidth: number;
  pointerOffsetY: number;
  pointerId: number;
  dragging: boolean;
};

type SidebarDragVisual = {
  kind: SidebarDragKind;
  itemId: string;
  label: string;
  left: number;
  top: number;
  width: number;
};

type SidebarDropTarget =
  | {
      kind: "channel";
      sectionId: string;
      beforeChannelId: string | null;
    }
  | {
      kind: "section";
      beforeSectionId: string | null;
    };

const sidebarContextMenuMaxHeightPx = 248;
const sectionChannelMenuWidthPx = 240;
const sectionChannelMenuMaxHeightPx = 224;

const sidebarMoreExpandedStorageKey = "loom.sidebar.moreExpanded";

function loadSidebarMoreExpanded() {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(sidebarMoreExpandedStorageKey) === "true";
  } catch {
    return false;
  }
}

function saveSidebarMoreExpanded(expanded: boolean) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      sidebarMoreExpandedStorageKey,
      expanded ? "true" : "false",
    );
  } catch {
    /* local-only preference; ignore quota or privacy-mode failures */
  }
}

export function Sidebar({
  view,
  setView,
  busy,
  channels,
  channelGroups,
  connection,
  account,
  workspace,
  hasWorkspace,
  workspaceId,
  workspaceName,
  workspaceServerUrl,
  activeChannelId,
  activeThreadId,
  inboxCount,
  threadsByChannel,
  onAddChannel,
  onAddChannelGroup,
  onMoveChannelToGroup,
  onReorderChannelGroup,
  onDeleteChannel,
  onRenameChannel,
  onRemoveChannelGroup,
  onRenameChannelGroup,
  onLeaveServer,
  onSelectChannel,
  onSelectThread,
  onToggleChannelGroup,
}: {
  view: View;
  setView: (view: View) => void;
  busy: string | null;
  channels: Channel[];
  channelGroups: ChannelGroup[];
  connection: ConnectionState;
  account: HumanAccount | null;
  workspace: Workspace | null;
  hasWorkspace: boolean;
  workspaceId: string | null;
  workspaceName: string | null;
  workspaceServerUrl: string | null;
  activeChannelId: string | null;
  activeThreadId: string | null;
  inboxCount: number;
  threadsByChannel: Record<string, Thread[]>;
  onAddChannel: (title: string) => void;
  onAddChannelGroup: (title: string) => void;
  onMoveChannelToGroup: (
    channelId: string,
    groupId: string,
    beforeChannelId?: string | null,
    targetChannelIds?: readonly string[],
  ) => void;
  onReorderChannelGroup: (
    groupId: string,
    beforeGroupId: string | null,
  ) => void;
  onDeleteChannel: (channel: Channel) => void;
  onRenameChannel: (channel: Channel, title: string) => void;
  onRemoveChannelGroup: (groupId: string) => void;
  onRenameChannelGroup: (groupId: string, title: string) => void;
  onLeaveServer: () => void;
  onSelectChannel: (channelId: string) => void;
  onSelectThread: (thread: Thread) => void;
  onToggleChannelGroup: (groupId: string) => void;
}) {
  const [createMenuOpen, setCreateMenuOpen] = useState(false);
  const createMenuMounted = usePresence(createMenuOpen);
  const [createKind, setCreateKind] = useState<"channel" | "section" | null>(null);
  const [createTitle, setCreateTitle] = useState("");
  const [editingSectionId, setEditingSectionId] = useState<string | null>(null);
  const [sectionTitleDraft, setSectionTitleDraft] = useState("");
  const [deleteSectionId, setDeleteSectionId] = useState<string | null>(null);
  const [editingChannelId, setEditingChannelId] = useState<string | null>(null);
  const [channelTitleDraft, setChannelTitleDraft] = useState("");
  const [deleteChannelId, setDeleteChannelId] = useState<string | null>(null);
  const [leaveServerConfirming, setLeaveServerConfirming] = useState(false);
  const [mobileQrOpen, setMobileQrOpen] = useState(false);
  const [serverMenuNotice, setServerMenuNotice] = useState<
    "copied" | "missing" | "copy_failed" | null
  >(null);
  const [sectionChannelMenuId, setSectionChannelMenuId] = useState<string | null>(null);
  const [sectionChannelMenuPosition, setSectionChannelMenuPosition] = useState<{
    right: number;
    top: number;
  } | null>(null);
  const [channelMoveMenuOpen, setChannelMoveMenuOpen] = useState(false);
  const [moreExpanded, setMoreExpanded] = useState(loadSidebarMoreExpanded);
  const [sidebarContextMenu, setSidebarContextMenu] =
    useState<SidebarContextMenu | null>(null);
  const [dragVisual, setDragVisual] = useState<SidebarDragVisual | null>(null);
  const [dropTarget, setDropTarget] = useState<SidebarDropTarget | null>(null);
  const dragSessionRef = useRef<SidebarPointerDrag | null>(null);
  const dragListenerCleanupRef = useRef<(() => void) | null>(null);
  const channelListRef = useRef<HTMLDivElement | null>(null);
  const suppressChannelClickRef = useRef<string | null>(null);
  const suppressSectionClickRef = useRef<string | null>(null);
  const sections = channelGroupSections(channelGroups, channels);
  const hasUserChannelGroups = channelGroups.some(
    (group) => group.id !== ungroupedChannelGroupId,
  );
  const settingsSection = useUIStore((state) => state.settingsSection);
  const setSettingsSection = useUIStore((state) => state.setSettingsSection);
  const { t } = useI18n();
  const contextMenuChannel = sidebarContextMenu?.kind === "channel"
    ? channels.find((channel) => channel.id === sidebarContextMenu.channelId) ?? null
    : null;
  const contextMenuSection = sidebarContextMenu?.kind === "section"
    ? sections.find((section) => section.id === sidebarContextMenu.sectionId) ?? null
    : null;
  const leaveServerBusy = Boolean(
    workspaceId && busy === `workspace:remove:${workspaceId}`,
  );
  const serverActionBusy = busy !== null;
  const mainNavItems = [
    { id: "direct" as const, label: t("Direct Messages"), icon: MessageCircle },
    { id: "settings" as const, label: t("Actors"), icon: Bot, section: "agents" as const },
    { id: "settings" as const, label: t("Managed Hosts"), icon: Server, section: "hosts" as const },
  ];
  const moreNavItems = [
    { id: "threads" as const, label: t("Threads"), icon: MessageSquare },
    { id: "inbox" as const, label: t("Inbox"), icon: Bell },
    { id: "tasks" as const, label: t("Tasks"), icon: Check },
    { id: "runs" as const, label: t("Runs"), icon: Play },
  ];
  const moreActive = moreNavItems.some((item) => item.id === view);
  const toggleMoreExpanded = () => {
    setMoreExpanded((expanded) => {
      const next = !expanded;
      saveSidebarMoreExpanded(next);
      return next;
    });
  };
  const closeCreateMenu = () => {
    setCreateMenuOpen(false);
    setCreateKind(null);
    setCreateTitle("");
    setLeaveServerConfirming(false);
    setServerMenuNotice(null);
  };

  useEffect(() => {
    setCreateMenuOpen(false);
    setCreateKind(null);
    setCreateTitle("");
    setLeaveServerConfirming(false);
    setServerMenuNotice(null);
    setSidebarContextMenu(null);
    setSectionChannelMenuId(null);
    setSectionChannelMenuPosition(null);
  }, [workspaceId]);

  const closeDeleteChannelConfirm = () => {
    setDeleteChannelId(null);
  };

  const closeRenameChannel = () => {
    setEditingChannelId(null);
    setChannelTitleDraft("");
  };

  const handleOpenCreate = (kind: "channel" | "section") => {
    setCreateKind(kind);
    setCreateTitle("");
  };

  const handleCreateSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const title = createTitle.trim();
    if (!title || !createKind) return;
    if (createKind === "channel") {
      if (!hasWorkspace) return;
      onAddChannel(title);
    } else {
      onAddChannelGroup(title);
    }
    closeCreateMenu();
  };

  const startRenameSection = (section: ChannelGroupSection) => {
    setEditingSectionId(section.id);
    setSectionTitleDraft(section.title);
    setDeleteSectionId(null);
    closeDeleteChannelConfirm();
    closeRenameChannel();
    setSidebarContextMenu(null);
    setSectionChannelMenuId(null);
    setSectionChannelMenuPosition(null);
  };

  const submitRenameSection = (
    event: FormEvent<HTMLFormElement>,
    sectionId: string,
  ) => {
    event.preventDefault();
    const title = sectionTitleDraft.trim();
    if (!title) return;
    onRenameChannelGroup(sectionId, title);
    setEditingSectionId(null);
    setSectionTitleDraft("");
  };

  const confirmDeleteSection = (sectionId: string) => {
    onRemoveChannelGroup(sectionId);
    if (editingSectionId === sectionId) {
      setEditingSectionId(null);
      setSectionTitleDraft("");
    }
    setDeleteSectionId(null);
  };

  const requestDeleteChannel = (channelId: string) => {
    closeCreateMenu();
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    closeRenameChannel();
    setSidebarContextMenu(null);
    setChannelMoveMenuOpen(false);
    setDeleteChannelId(channelId);
  };

  const startRenameChannel = (channel: Channel) => {
    closeCreateMenu();
    closeDeleteChannelConfirm();
    setSidebarContextMenu(null);
    setChannelMoveMenuOpen(false);
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    setEditingChannelId(channel.id);
    setChannelTitleDraft(channel.title);
  };

  const submitRenameChannel = (
    event: FormEvent<HTMLFormElement>,
    channel: Channel,
  ) => {
    event.preventDefault();
    const title = channelTitleDraft.trim();
    if (!title) return;
    onRenameChannel(channel, title);
    closeRenameChannel();
  };

  const openChannelContextMenu = (
    event: MouseEvent<HTMLElement>,
    channel: Channel,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    cancelSidebarDrag();
    closeCreateMenu();
    closeDeleteChannelConfirm();
    closeRenameChannel();
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    const position = contextMenuPosition(event);
    setChannelMoveMenuOpen(false);
    setSectionChannelMenuId(null);
    setSectionChannelMenuPosition(null);
    setSidebarContextMenu({
      kind: "channel",
      channelId: channel.id,
      ...position,
    });
  };

  const openSectionContextMenu = (
    event: MouseEvent<HTMLElement>,
    section: ChannelGroupSection,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    cancelSidebarDrag();
    closeCreateMenu();
    closeDeleteChannelConfirm();
    closeRenameChannel();
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    setChannelMoveMenuOpen(false);
    setSectionChannelMenuId(null);
    setSectionChannelMenuPosition(null);
    setSidebarContextMenu({
      kind: "section",
      sectionId: section.id,
      ...contextMenuPosition(event),
    });
  };

  const openBlankContextMenu = (event: MouseEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    cancelSidebarDrag();
    closeCreateMenu();
    closeDeleteChannelConfirm();
    closeRenameChannel();
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    setChannelMoveMenuOpen(false);
    setSectionChannelMenuId(null);
    setSectionChannelMenuPosition(null);
    setSidebarContextMenu({ kind: "blank", ...contextMenuPosition(event) });
  };

  const openCreateFromContextMenu = (kind: "channel" | "section") => {
    setSidebarContextMenu(null);
    setCreateMenuOpen(true);
    handleOpenCreate(kind);
  };

  const openSectionChannelMenu = (
    sectionId: string,
    anchor: Pick<DOMRect, "left" | "bottom">,
  ) => {
    setSidebarContextMenu(null);
    setSectionChannelMenuPosition({
      right: Math.max(
        channelContextMenuViewportPaddingPx,
        window.innerWidth - anchor.left - sectionChannelMenuWidthPx,
      ),
      top: Math.max(
        channelContextMenuViewportPaddingPx,
        Math.min(
          anchor.bottom + 4,
          window.innerHeight -
            sectionChannelMenuMaxHeightPx -
            channelContextMenuViewportPaddingPx,
        ),
      ),
    });
    setSectionChannelMenuId(sectionId);
  };

  const handleInvitePeople = async () => {
    const serverUrl = workspaceServerUrl?.trim();
    if (!serverUrl) {
      setServerMenuNotice("missing");
      return;
    }
    try {
      await navigator.clipboard.writeText(serverUrl);
      setServerMenuNotice("copied");
    } catch {
      setServerMenuNotice("copy_failed");
    }
  };

  const serverMenuNoticeText = serverMenuNotice === "copied"
    ? t("Server address copied")
    : serverMenuNotice === "missing"
      ? t("No server address is available.")
      : serverMenuNotice === "copy_failed"
        ? t("Could not copy the server address.")
        : null;

  function contextMenuPosition(event: MouseEvent<HTMLElement>) {
    return {
      x: Math.max(
        channelContextMenuViewportPaddingPx,
        Math.min(
          event.clientX,
          window.innerWidth - channelContextMenuWidthPx - channelContextMenuViewportPaddingPx,
        ),
      ),
      y: Math.max(
        channelContextMenuViewportPaddingPx,
        Math.min(
          event.clientY,
          window.innerHeight - sidebarContextMenuMaxHeightPx - channelContextMenuViewportPaddingPx,
        ),
      ),
    };
  }

  const cleanupSidebarDragListeners = () => {
    dragListenerCleanupRef.current?.();
    dragListenerCleanupRef.current = null;
  };

  const scrollChannelListForDrag = (clientY: number) => {
    const list = channelListRef.current;
    if (!list) return;
    const rect = list.getBoundingClientRect();
    const edgeSize = Math.min(44, rect.height / 4);
    let delta = 0;
    if (clientY < rect.top + edgeSize) {
      delta = -Math.ceil((rect.top + edgeSize - clientY) / 4);
    } else if (clientY > rect.bottom - edgeSize) {
      delta = Math.ceil((clientY - (rect.bottom - edgeSize)) / 4);
    }
    if (delta !== 0) list.scrollTop += Math.max(-14, Math.min(14, delta));
  };

  const channelDropTargetAtPoint = (clientY: number, channelId: string) => {
    const sectionElements = Array.from(
      document.querySelectorAll<HTMLElement>("[data-channel-section-id]"),
    );
    if (sectionElements.length === 0) return null;

    const listRect = channelListRef.current?.getBoundingClientRect();
    const boundedY = listRect
      ? Math.max(listRect.top, Math.min(listRect.bottom, clientY))
      : clientY;
    const section = sectionElements.find((element) => {
      const rect = element.getBoundingClientRect();
      return boundedY >= rect.top && boundedY <= rect.bottom;
    }) ?? sectionElements.reduce((closest, element) => {
      const closestRect = closest.getBoundingClientRect();
      const rect = element.getBoundingClientRect();
      const closestDistance = Math.min(
        Math.abs(boundedY - closestRect.top),
        Math.abs(boundedY - closestRect.bottom),
      );
      const distance = Math.min(
        Math.abs(boundedY - rect.top),
        Math.abs(boundedY - rect.bottom),
      );
      return distance < closestDistance ? element : closest;
    });

    const rows = Array.from(
      section.querySelectorAll<HTMLElement>("[data-channel-row-id]"),
    ).filter((row) =>
      row.dataset.channelRowId !== channelId &&
      row.getBoundingClientRect().height > 0
    );
    const beforeRow = rows.find((row) => {
      const rect = row.getBoundingClientRect();
      return boundedY < rect.top + rect.height / 2;
    });
    return {
      kind: "channel" as const,
      sectionId: section.dataset.channelSectionId!,
      beforeChannelId: beforeRow?.dataset.channelRowId ?? null,
    };
  };

  const sectionDropTargetAtPoint = (clientY: number, sectionId: string) => {
    const sectionElements = Array.from(
      document.querySelectorAll<HTMLElement>(
        '[data-channel-section-local="true"]',
      ),
    ).filter((element) => element.dataset.channelSectionId !== sectionId);
    const beforeSection = sectionElements.find((element) => {
      const rect = element.getBoundingClientRect();
      return clientY < rect.top + rect.height / 2;
    });
    return {
      kind: "section" as const,
      beforeSectionId: beforeSection?.dataset.channelSectionId ?? null,
    };
  };

  const dropTargetAtPoint = (
    session: SidebarPointerDrag,
    clientY: number,
  ): SidebarDropTarget | null =>
    session.kind === "channel"
      ? channelDropTargetAtPoint(clientY, session.itemId)
      : sectionDropTargetAtPoint(clientY, session.itemId);

  const updateSidebarDragAtPoint = (
    clientX: number,
    clientY: number,
    pointerId: number,
    preventDefault?: () => void,
  ) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== pointerId) return;
    const distance =
      Math.abs(clientX - session.startX) + Math.abs(clientY - session.startY);
    if (!session.dragging && distance < 6) return;
    if (!session.dragging) {
      session.dragging = true;
      closeCreateMenu();
      setSidebarContextMenu(null);
      setSectionChannelMenuId(null);
      document.body.classList.add("sidebar-dragging");
    }
    preventDefault?.();
    scrollChannelListForDrag(clientY);
    setDragVisual({
      kind: session.kind,
      itemId: session.itemId,
      label: session.label,
      left:
        session.originLeft +
        Math.max(-8, Math.min(8, (clientX - session.startX) * 0.12)),
      top: clientY - session.pointerOffsetY,
      width: session.originWidth,
    });
    setDropTarget(dropTargetAtPoint(session, clientY));
  };

  const finishSidebarDragAtPoint = (
    _clientX: number,
    clientY: number,
    pointerId: number,
    preventDefault?: () => void,
    stopPropagation?: () => void,
  ) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== pointerId) return;
    const didDrag = session.dragging;
    const finalTarget = didDrag ? dropTargetAtPoint(session, clientY) : null;
    dragSessionRef.current = null;
    cleanupSidebarDragListeners();
    if (didDrag) {
      preventDefault?.();
      stopPropagation?.();
      const suppressRef = session.kind === "channel"
        ? suppressChannelClickRef
        : suppressSectionClickRef;
      suppressRef.current = session.itemId;
      window.setTimeout(() => {
        if (suppressRef.current === session.itemId) {
          suppressRef.current = null;
        }
      }, 160);
      if (session.kind === "channel" && finalTarget?.kind === "channel") {
        const targetSection = sections.find(
          (section) => section.id === finalTarget.sectionId,
        );
        onMoveChannelToGroup(
          session.itemId,
          finalTarget.sectionId,
          finalTarget.beforeChannelId,
          targetSection?.channels.map((channel) => channel.id) ?? [],
        );
      } else if (
        session.kind === "section" &&
        finalTarget?.kind === "section"
      ) {
        onReorderChannelGroup(
          session.itemId,
          finalTarget.beforeSectionId,
        );
      }
    }
    document.body.classList.remove("sidebar-dragging");
    setDragVisual(null);
    setDropTarget(null);
  };

  const cancelSidebarDrag = () => {
    cleanupSidebarDragListeners();
    dragSessionRef.current = null;
    document.body.classList.remove("sidebar-dragging");
    setDragVisual(null);
    setDropTarget(null);
  };

  const beginSidebarDrag = (
    event: PointerEvent<HTMLElement>,
    kind: SidebarDragKind,
    itemId: string,
    label: string,
  ) => {
    if (event.button !== 0) return;
    cleanupSidebarDragListeners();
    const dragShell = event.currentTarget.closest<HTMLElement>(
      kind === "section"
        ? "[data-channel-section-id]"
        : "[data-channel-row-id]",
    );
    const rect = (dragShell ?? event.currentTarget).getBoundingClientRect();
    dragSessionRef.current = {
      kind,
      itemId,
      label,
      startX: event.clientX,
      startY: event.clientY,
      originLeft: rect.left,
      originWidth: rect.width,
      pointerOffsetY: Math.max(0, event.clientY - rect.top),
      pointerId: event.pointerId,
      dragging: false,
    };
    const handlePointerMove = (moveEvent: globalThis.PointerEvent) => {
      updateSidebarDragAtPoint(
        moveEvent.clientX,
        moveEvent.clientY,
        moveEvent.pointerId,
        () => moveEvent.preventDefault(),
      );
    };
    const handlePointerUp = (upEvent: globalThis.PointerEvent) => {
      finishSidebarDragAtPoint(
        upEvent.clientX,
        upEvent.clientY,
        upEvent.pointerId,
        () => upEvent.preventDefault(),
        () => upEvent.stopPropagation(),
      );
    };
    const handlePointerCancel = (cancelEvent: globalThis.PointerEvent) => {
      if (dragSessionRef.current?.pointerId === cancelEvent.pointerId) {
        cancelSidebarDrag();
      }
    };
    window.addEventListener("pointermove", handlePointerMove, { passive: false });
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);
    dragListenerCleanupRef.current = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerCancel);
    };
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      /* Some webviews do not support pointer capture; window listeners still handle drag. */
    }
  };

  const updateSidebarDrag = (event: PointerEvent<HTMLElement>) => {
    updateSidebarDragAtPoint(
      event.clientX,
      event.clientY,
      event.pointerId,
      () => event.preventDefault(),
    );
  };

  const finishSidebarDrag = (event: PointerEvent<HTMLElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    try {
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    } catch {
      /* Ignore pointer-capture differences across desktop webviews. */
    }
    finishSidebarDragAtPoint(
      event.clientX,
      event.clientY,
      event.pointerId,
      () => event.preventDefault(),
      () => event.stopPropagation(),
    );
  };

  useEffect(() => {
    if (!sidebarContextMenu) return;
    const close = () => {
      setSidebarContextMenu(null);
      setChannelMoveMenuOpen(false);
    };
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [sidebarContextMenu]);

  useEffect(() => {
    if (!sectionChannelMenuId) return;
    const close = () => {
      setSectionChannelMenuId(null);
      setSectionChannelMenuPosition(null);
    };
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [sectionChannelMenuId]);

  useEffect(() => {
    if (!createMenuOpen) return;
    const close = () => closeCreateMenu();
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [createMenuOpen]);

  useEffect(() => {
    if (!serverMenuNotice) return;
    const timeout = window.setTimeout(() => setServerMenuNotice(null), 2200);
    return () => window.clearTimeout(timeout);
  }, [serverMenuNotice]);

  useEffect(() => () => cancelSidebarDrag(), []);
  return (
    <aside className="flex min-h-0 min-w-0 flex-col bg-[#fbfbfd]">
      <div className="relative border-b border-[#edf0f5] p-2">
        <button
          type="button"
          className={cn(
            "group flex h-11 w-full items-center gap-3 rounded-lg px-3 text-left transition-colors hover:bg-[#f0f1f8]",
            createMenuOpen && "bg-[#f0f1f8]",
          )}
          aria-label={t("Server menu")}
          aria-haspopup="menu"
          aria-expanded={createMenuOpen}
          onClick={(event) => {
            event.stopPropagation();
            if (createMenuOpen) {
              closeCreateMenu();
            } else {
              setCreateMenuOpen(true);
              setSidebarContextMenu(null);
              setSectionChannelMenuId(null);
            }
          }}
        >
          <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-[#ede9fe] text-[#5843d7]">
            <Server size={15} />
          </span>
          <span className="min-w-0 flex-1 truncate text-sm font-bold text-[#202635]">
            {workspaceName ?? t("No server selected")}
          </span>
          <span className="relative h-[17px] w-[17px] shrink-0 text-[#596174]" aria-hidden>
            <Menu
              size={17}
              className={cn(
                "absolute inset-0 transition-[opacity,transform] duration-150",
                createMenuOpen ? "rotate-90 scale-75 opacity-0" : "rotate-0 scale-100 opacity-100",
              )}
            />
            <X
              size={17}
              className={cn(
                "absolute inset-0 transition-[opacity,transform] duration-150",
                createMenuOpen ? "rotate-0 scale-100 opacity-100" : "-rotate-90 scale-75 opacity-0",
              )}
            />
          </span>
        </button>

        {createMenuMounted && (
          <div
            className={cn(
              "surface-menu surface-menu-origin-top-left absolute left-2 right-2 top-[calc(100%-2px)] z-40 rounded-lg border border-[#dfe3ec] bg-white p-1 text-sm shadow-[0_14px_36px_rgb(16_24_40_/_0.13)]",
              !createMenuOpen && "motion-menu-closing pointer-events-none",
            )}
            role={createMenuOpen ? "menu" : undefined}
            aria-hidden={!createMenuOpen}
            aria-label={t("{{server}} actions", { server: workspaceName ?? t("Server") })}
            onClick={(event) => event.stopPropagation()}
          >
            {createKind ? (
              <form className="grid gap-2 p-2" onSubmit={handleCreateSubmit}>
                <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-[#667085]">
                  {createKind === "channel" ? <Hash size={13} /> : <Folder size={13} />}
                  {createKind === "channel" ? t("Create new channel") : t("Create new section")}
                </div>
                <Input
                  autoFocus
                  value={createTitle}
                  onChange={(event) => setCreateTitle(event.target.value)}
                  placeholder={createKind === "channel" ? t("Channel name") : t("Section name")}
                  className="h-9 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                />
                {createKind === "channel" && !hasWorkspace && (
                  <div className="text-xs font-medium text-amber-700">
                    {t("Select a server before creating a channel.")}
                  </div>
                )}
                {createKind === "channel" && hasWorkspace && connection !== "open" && (
                  <div className="text-xs font-medium text-amber-700">
                    {t("Will connect to {{server}} before creating.", {
                      server: workspaceName ?? t("this server"),
                    })}
                  </div>
                )}
                <div className="flex justify-end gap-2 pt-1">
                  <Button type="button" variant="outline" size="sm" onClick={() => setCreateKind(null)}>
                    {t("Back")}
                  </Button>
                  <Button
                    type="submit"
                    size="sm"
                    disabled={!createTitle.trim() || (createKind === "channel" && !hasWorkspace)}
                  >
                    {createKind === "channel" && connection !== "open"
                      ? t("Connect & create")
                      : t("Create")}
                  </Button>
                </div>
              </form>
            ) : leaveServerConfirming ? (
              <div className="grid gap-2 p-2">
                <div className="text-sm font-bold text-[#202635]">{t("Leave this server?")}</div>
                <div className="text-xs leading-5 text-[#667085]">
                  {t("This removes the saved server from Loom Desktop. Server data is not deleted.")}
                </div>
                <div className="flex justify-end gap-2 pt-1">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={leaveServerBusy}
                    onClick={() => setLeaveServerConfirming(false)}
                  >
                    {t("Cancel")}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    className="bg-red-600 text-white hover:bg-red-700"
                    disabled={serverActionBusy || !workspaceId}
                    onClick={onLeaveServer}
                  >
                    {leaveServerBusy ? <Loader2 className="animate-spin" size={13} /> : <LogOut size={13} />}
                    {t("Confirm leave server")}
                  </Button>
                </div>
              </div>
            ) : (
              <>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => handleOpenCreate("section")}
                >
                  <Folder size={15} />
                  {t("Create new section")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => handleOpenCreate("channel")}
                >
                  <Hash size={15} />
                  {t("Create new channel")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4] disabled:cursor-not-allowed disabled:opacity-50"
                  role="menuitem"
                  disabled={!workspaceServerUrl?.trim()}
                  onClick={() => void handleInvitePeople()}
                >
                  {serverMenuNotice === "copied" ? <Copy size={15} /> : <UserPlus size={15} />}
                  {t("Invite other people")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4] disabled:cursor-not-allowed disabled:opacity-50"
                  role="menuitem"
                  disabled={!account || !workspace}
                  title={workspace ? t("Connect Loom Mobile") : t("Connect to a server first")}
                  onClick={() => {
                    closeCreateMenu();
                    setMobileQrOpen(true);
                  }}
                >
                  <QrCode size={15} />
                  {t("Mobile QR Code")}
                </button>
                {serverMenuNotice && (
                  <div
                    className={cn(
                      "mx-2 my-1 rounded-md px-2 py-1.5 text-xs font-semibold",
                      serverMenuNotice === "copied"
                        ? "bg-emerald-50 text-emerald-700"
                        : "bg-amber-50 text-amber-700",
                    )}
                    role="status"
                  >
                    {serverMenuNoticeText}
                  </div>
                )}
                <div className="my-1 border-t border-[#edf0f5]" />
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-red-600 hover:bg-red-50 disabled:cursor-not-allowed disabled:opacity-50"
                  role="menuitem"
                  disabled={!workspaceId || serverActionBusy}
                  onClick={() => setLeaveServerConfirming(true)}
                >
                  {leaveServerBusy ? <Loader2 className="animate-spin" size={15} /> : <LogOut size={15} />}
                  {t("Leave server")}
                </button>
              </>
            )}
          </div>
        )}
      </div>

      <div className="border-b border-[#edf0f5] p-3">
        <div className="space-y-1">
          {mainNavItems.map((item) => {
            const Icon = item.icon;
            const selected =
              item.id === "settings"
                ? view === "settings" &&
                  (item.section === "hosts"
                    ? settingsSection === "hosts"
                    : settingsSection !== "hosts")
                : view === item.id;
            return (
              <button
                key={item.label}
                type="button"
                className={cn("nav-row h-9 text-sm", selected && "nav-row-active")}
                onClick={() => {
                  closeCreateMenu();
                  if (item.id === "settings" && item.section) {
                    setSettingsSection(item.section);
                  }
                  setView(item.id);
                }}
              >
                <Icon size={16} />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div
          ref={channelListRef}
          className="min-h-0 flex-1 overflow-y-auto p-3 soft-scrollbar"
          aria-label={t("Channel sections")}
          onContextMenu={openBlankContextMenu}
        >
          {sections.map((section) => (
            <Fragment key={section.id}>
              {dropTarget?.kind === "section" &&
                (dropTarget.beforeSectionId === section.id ||
                  (dropTarget.beforeSectionId === null && !section.local)) && (
                  <div className="sidebar-drop-indicator sidebar-drop-indicator-section" />
                )}
            <div
              data-channel-section-id={section.id}
              data-channel-section-local={section.local ? "true" : "false"}
              className={cn(
                "mb-3 rounded-lg",
                dragVisual?.kind === "section" &&
                  dragVisual.itemId === section.id &&
                  "sidebar-drag-placeholder",
              )}
              onContextMenu={(event) => {
                if (section.local) {
                  openSectionContextMenu(event, section);
                } else {
                  openBlankContextMenu(event);
                }
              }}
            >
              {(section.local || hasUserChannelGroups) && (
                <div className="channel-group-header group/channelgroup relative">
                  <button
                    type="button"
                    className={cn(
                      "flex min-w-0 flex-1 touch-none select-none items-center gap-1.5 text-left",
                      section.local && "cursor-grab active:cursor-grabbing",
                    )}
                    onPointerDown={(event) => {
                      if (section.local) {
                        beginSidebarDrag(
                          event,
                          "section",
                          section.id,
                          section.title,
                        );
                      }
                    }}
                    onPointerMove={updateSidebarDrag}
                    onPointerUp={finishSidebarDrag}
                    onPointerCancel={cancelSidebarDrag}
                    onClick={() => {
                      if (suppressSectionClickRef.current === section.id) {
                        suppressSectionClickRef.current = null;
                        return;
                      }
                      if (section.local) onToggleChannelGroup(section.id);
                    }}
                    disabled={!section.local}
                  >
                    {section.local ? (
                      <>
                        <GripVertical
                          size={12}
                          className="shrink-0 text-[#98a2b3] opacity-0 transition-opacity group-hover/channelgroup:opacity-100"
                        />
                        <ChevronDown
                          size={13}
                          className={cn(
                            "motion-chevron text-[#667085]",
                            section.collapsed && "-rotate-90",
                          )}
                        />
                      </>
                    ) : (
                      <span className="w-[29px]" />
                    )}
                    <span className="min-w-0 truncate">{section.title}</span>
                    <span className="count-badge ml-1 h-5 min-w-5 text-[10px]">
                      {section.channels.length}
                    </span>
                  </button>
                  {section.local && (
                    <div
                      className={cn(
                        "pointer-events-none relative flex items-center gap-1 opacity-0 transition-opacity group-hover/channelgroup:pointer-events-auto group-hover/channelgroup:opacity-100 focus-within:pointer-events-auto focus-within:opacity-100",
                        sectionChannelMenuId === section.id && "pointer-events-auto opacity-100",
                      )}
                    >
                      <button
                        type="button"
                        className="composer-icon h-6 min-w-6"
                        title={t("Add channel to {{section}}", { section: section.title })}
                        aria-label={t("Add channel to {{section}}", { section: section.title })}
                        aria-haspopup="menu"
                        aria-expanded={sectionChannelMenuId === section.id}
                        onClick={(event) => {
                          event.stopPropagation();
                          setSidebarContextMenu(null);
                          if (sectionChannelMenuId === section.id) {
                            setSectionChannelMenuId(null);
                            setSectionChannelMenuPosition(null);
                            return;
                          }
                          openSectionChannelMenu(
                            section.id,
                            event.currentTarget.getBoundingClientRect(),
                          );
                        }}
                      >
                        <Plus size={12} />
                      </button>
                      <button
                        type="button"
                        className="composer-icon h-6 min-w-6"
                        title={t("Rename section")}
                        onClick={() => {
                          setSectionChannelMenuId(null);
                          setSectionChannelMenuPosition(null);
                          startRenameSection(section);
                        }}
                      >
                        <Pencil size={12} />
                      </button>
                      <button
                        type="button"
                        className="composer-icon h-6 min-w-6 text-red-500 hover:text-red-600"
                        title={t("Delete section")}
                        onClick={() => {
                          setSectionChannelMenuId(null);
                          setSectionChannelMenuPosition(null);
                          setDeleteSectionId(section.id);
                          setEditingSectionId(null);
                          setSectionTitleDraft("");
                          closeDeleteChannelConfirm();
                        }}
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  )}
                </div>
              )}
              {editingSectionId === section.id && (
                <form
                  className="channel-section-editor"
                  onSubmit={(event) => submitRenameSection(event, section.id)}
                >
                  <Input
                    autoFocus
                    value={sectionTitleDraft}
                    onChange={(event) => setSectionTitleDraft(event.target.value)}
                    placeholder={t("Section name")}
                    className="h-8 rounded-lg border-[#dfe3ec] bg-white text-xs shadow-none"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      setEditingSectionId(null);
                      setSectionTitleDraft("");
                    }}
                  >
                    {t("Cancel")}
                  </Button>
                  <Button type="submit" size="sm" disabled={!sectionTitleDraft.trim()}>
                    {t("Save")}
                  </Button>
                </form>
              )}
              {deleteSectionId === section.id && (
                <div className="channel-section-editor">
                  <div className="min-w-0 flex-1 text-xs font-medium text-[#667085]">
                    Delete "{section.title}"? Channels stay available.
                  </div>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => setDeleteSectionId(null)}
                  >
                    {t("Cancel")}
                  </Button>
                  <Button type="button" size="sm" onClick={() => confirmDeleteSection(section.id)}>
                    {t("Delete")}
                  </Button>
                </div>
              )}
              <div
                className={cn(
                  "collapsible-region",
                  section.collapsed && "collapsible-region-collapsed",
                )}
                aria-hidden={section.collapsed}
              >
                <div className="collapsible-region-inner">
                  <div className="mt-1 space-y-1">
                  {section.channels.length === 0 ? (
                    <>
                      {dropTarget?.kind === "channel" &&
                        dropTarget.sectionId === section.id && (
                          <div className="sidebar-drop-indicator" />
                        )}
                      <div className="px-3 py-2 text-xs text-[#8a93a5]">
                        {section.local ? t("Drop channels here.") : t("No channels yet.")}
                      </div>
                    </>
                  ) : (
                    <>
                    {section.channels.map((channel) => {
                      const selected =
                        view === "chat" &&
                        channel.id === activeChannelId &&
                        !activeThreadId;
                      const threads = threadsByChannel[channel.id] ?? [];
                      const deleteBusy = busy === `channel:delete:${channel.id}`;
                      const renameBusy = busy === `channel:rename:${channel.id}`;
                      return (
                        <Fragment key={channel.id}>
                        {dropTarget?.kind === "channel" &&
                          dropTarget.sectionId === section.id &&
                          dropTarget.beforeChannelId === channel.id && (
                            <div className="sidebar-drop-indicator" />
                          )}
                        <div
                          data-channel-row-id={channel.id}
                          className={cn(
                            "group/channel",
                            dragVisual?.kind === "channel" &&
                              dragVisual.itemId === channel.id &&
                              "sidebar-drag-placeholder",
                          )}
                        >
                          <div className="flex items-center gap-1">
                            <button
                              className={cn(
                                "channel-row min-w-0 flex-1 touch-none select-none",
                                dragVisual?.kind === "channel" &&
                                  dragVisual.itemId === channel.id &&
                                  "cursor-grabbing",
                                selected && "channel-row-active",
                              )}
                              onPointerDown={(event) =>
                                beginSidebarDrag(
                                  event,
                                  "channel",
                                  channel.id,
                                  channel.title,
                                )
                              }
                              onPointerMove={updateSidebarDrag}
                              onPointerUp={finishSidebarDrag}
                              onPointerCancel={cancelSidebarDrag}
                              onContextMenu={(event) =>
                                openChannelContextMenu(event, channel)
                              }
                              onClick={() => {
                                if (suppressChannelClickRef.current === channel.id) {
                                  suppressChannelClickRef.current = null;
                                  return;
                                }
                                closeCreateMenu();
                                closeDeleteChannelConfirm();
                                closeRenameChannel();
                                onSelectChannel(channel.id);
                              }}
                            >
                              <GripVertical
                                size={13}
                                className={cn(
                                  "shrink-0 text-[#98a2b3] opacity-0 transition-opacity group-hover/channel:opacity-100",
                                  selected && "text-white/70",
                                )}
                              />
                              <Hash size={15} />
                              <span className="min-w-0 flex-1 truncate">{channel.title}</span>
                              <Badge
                                variant="outline"
                                className={cn(
                                  "ml-auto h-5 border-transparent bg-[#f1efff] px-1.5 text-[10px] text-[#5843d7]",
                                  selected && "bg-white/20 text-white",
                                )}
                              >
                                {threads.length}
                              </Badge>
                            </button>
                          </div>
                          {editingChannelId === channel.id && (
                            <form
                              className="channel-section-editor"
                              onSubmit={(event) => submitRenameChannel(event, channel)}
                            >
                              <Input
                                autoFocus
                                value={channelTitleDraft}
                                onChange={(event) =>
                                  setChannelTitleDraft(event.target.value)
                                }
                                placeholder={t("Channel name")}
                                className="h-8 rounded-lg border-[#dfe3ec] bg-white text-xs shadow-none"
                              />
                              <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                disabled={renameBusy}
                                onClick={closeRenameChannel}
                              >
                                {t("Cancel")}
                              </Button>
                              <Button
                                type="submit"
                                size="sm"
                                disabled={!channelTitleDraft.trim() || renameBusy}
                              >
                                {renameBusy ? (
                                  <Loader2 className="animate-spin" size={13} />
                                ) : (
                                  t("Save")
                                )}
                              </Button>
                            </form>
                          )}
                          {deleteChannelId === channel.id && (
                            <ChannelDeleteConfirm
                              compact
                              channel={channel}
                              deleteBusy={deleteBusy}
                              onCancel={closeDeleteChannelConfirm}
                              onConfirm={() => {
                                onDeleteChannel(channel);
                                closeDeleteChannelConfirm();
                              }}
                            />
                          )}
                          {channel.id === activeChannelId && threads.length > 0 && (
                            <div className="ml-4 mt-1 space-y-1 border-l border-[#e1e5ef] pl-2">
                              {threads.map((thread) => (
                                <button
                                  key={thread.id}
                                  className={cn(
                                    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs font-medium text-[#667085] hover:bg-[#f0f1f8] hover:text-[#303849]",
                                    view === "chat" &&
                                      activeThreadId === thread.id &&
                                      "bg-[#eeeaff] text-[#5843d7]",
                                  )}
                                  onClick={() => {
                                    closeCreateMenu();
                                    closeDeleteChannelConfirm();
                                    closeRenameChannel();
                                    onSelectThread(thread);
                                  }}
                                >
                                  <Split size={13} />
                                  <span className="min-w-0 flex-1 truncate">{thread.title}</span>
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                        </Fragment>
                      );
                    })}
                    {dropTarget?.kind === "channel" &&
                      dropTarget.sectionId === section.id &&
                      dropTarget.beforeChannelId === null && (
                        <div className="sidebar-drop-indicator" />
                      )}
                    </>
                  )}
                  </div>
                </div>
              </div>
            </div>
            </Fragment>
          ))}
          {dropTarget?.kind === "section" &&
            dropTarget.beforeSectionId === null &&
            sections.every((section) => section.local) && (
              <div className="sidebar-drop-indicator sidebar-drop-indicator-section" />
            )}
        </div>
      </div>
      <div className="flex flex-col-reverse border-t border-[#edf0f5] p-3">
        <button
          type="button"
          className={cn("nav-row h-9 text-sm", moreActive && "nav-row-active")}
          aria-expanded={moreExpanded}
          aria-controls="sidebar-more-nav"
          onClick={() => {
            closeCreateMenu();
            closeDeleteChannelConfirm();
            closeRenameChannel();
            toggleMoreExpanded();
          }}
        >
          <ChevronDown
            size={15}
            className={cn(
              "motion-chevron shrink-0 text-[#667085]",
              !moreExpanded && "-rotate-90",
            )}
          />
          <span className="min-w-0 flex-1 truncate">{t("More")}</span>
          {inboxCount > 0 && (
            <span className="count-badge h-5 min-w-5 text-[10px]">
              {inboxCount}
            </span>
          )}
        </button>
        <div
          id="sidebar-more-nav"
          className={cn(
            "collapsible-region w-full",
            !moreExpanded && "collapsible-region-collapsed",
          )}
          aria-hidden={!moreExpanded}
        >
          <div className="collapsible-region-inner">
            <div className="mb-1 space-y-1">
            {moreNavItems.map((item) => {
              const Icon = item.icon;
              const selected = view === item.id;
              const count = item.id === "inbox" ? inboxCount : 0;
              return (
                <button
                  key={item.id}
                  type="button"
                  className={cn("nav-row h-9 text-sm", selected && "nav-row-active")}
                  onClick={() => {
                    closeCreateMenu();
                    closeDeleteChannelConfirm();
                    closeRenameChannel();
                    setView(item.id);
                  }}
                >
                  <Icon size={16} />
                  <span className="min-w-0 flex-1 truncate">{item.label}</span>
                  {count > 0 && (
                    <span className="count-badge h-5 min-w-5 text-[10px]">
                      {count}
                    </span>
                  )}
                </button>
              );
            })}
            </div>
          </div>
        </div>
      </div>
      {dragVisual &&
        createPortal(
          <div
            className={cn(
              "sidebar-drag-overlay",
              dragVisual.kind === "section" && "sidebar-drag-overlay-section",
            )}
            style={{
              left: dragVisual.left,
              top: dragVisual.top,
              width: dragVisual.width,
            }}
            aria-hidden="true"
          >
            <GripVertical size={14} className="shrink-0 text-[#8b82d9]" />
            {dragVisual.kind === "channel" ? (
              <Hash size={15} className="shrink-0 text-[#6553dc]" />
            ) : (
              <Folder size={15} className="shrink-0 text-[#6553dc]" />
            )}
            <span className="min-w-0 flex-1 truncate">{dragVisual.label}</span>
          </div>,
          document.body,
        )}
      {sidebarContextMenu &&
        createPortal(
          <div
            className="surface-menu surface-menu-origin-top-left fixed z-50 rounded-lg border border-[#dfe3ec] bg-white p-1 text-sm shadow-[0_14px_36px_rgb(16_24_40_/_0.14)]"
            style={{
              left: sidebarContextMenu.x,
              top: sidebarContextMenu.y,
              width: channelContextMenuWidthPx,
            }}
            role="menu"
            aria-label={
              sidebarContextMenu.kind === "channel" && contextMenuChannel
                ? t("Channel actions for {{channel}}", { channel: contextMenuChannel.title })
                : sidebarContextMenu.kind === "section" && contextMenuSection
                  ? t("Section actions for {{section}}", { section: contextMenuSection.title })
                  : t("Channel list actions")
            }
            onClick={(event) => event.stopPropagation()}
            onContextMenu={(event) => event.preventDefault()}
          >
            {sidebarContextMenu.kind === "blank" && (
              <>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => openCreateFromContextMenu("channel")}
                >
                  <Hash size={14} />
                  {t("New channel")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => openCreateFromContextMenu("section")}
                >
                  <Folder size={14} />
                  {t("New section")}
                </button>
              </>
            )}

            {sidebarContextMenu.kind === "channel" && contextMenuChannel && (
              <>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => {
                    setSidebarContextMenu(null);
                    onSelectChannel(contextMenuChannel.id);
                  }}
                >
                  <Hash size={14} />
                  {t("Open")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  aria-expanded={channelMoveMenuOpen}
                  onClick={() => setChannelMoveMenuOpen((open) => !open)}
                >
                  <Folder size={14} />
                  {t("Move to section")}
                  <ChevronRight
                    size={13}
                    className={cn("ml-auto transition-transform", channelMoveMenuOpen && "rotate-90")}
                  />
                </button>
                {channelMoveMenuOpen && (
                  <div className="max-h-44 overflow-y-auto border-y border-[#edf0f5] py-1 soft-scrollbar">
                    {sections.filter(
                      (section) => !section.channels.some((channel) => channel.id === contextMenuChannel.id),
                    ).length === 0 ? (
                      <div className="px-3 py-2 text-xs font-medium text-[#8a93a5]">
                        {t("No other sections available.")}
                      </div>
                    ) : (
                      sections
                        .filter(
                          (section) => !section.channels.some((channel) => channel.id === contextMenuChannel.id),
                        )
                        .map((section) => (
                          <button
                            key={section.id}
                            type="button"
                            className="flex min-h-8 w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs font-semibold text-[#596174] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                            role="menuitem"
                            onClick={() => {
                              onMoveChannelToGroup(contextMenuChannel.id, section.id);
                              setSidebarContextMenu(null);
                              setChannelMoveMenuOpen(false);
                            }}
                          >
                            <ArrowRight size={12} />
                            <span className="min-w-0 flex-1 truncate">{section.title}</span>
                          </button>
                        ))
                    )}
                  </div>
                )}
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => startRenameChannel(contextMenuChannel)}
                >
                  <Pencil size={14} />
                  {t("Rename")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-red-600 hover:bg-red-50"
                  role="menuitem"
                  onClick={() => requestDeleteChannel(contextMenuChannel.id)}
                >
                  <Trash2 size={14} />
                  {t("Delete")}
                </button>
              </>
            )}

            {sidebarContextMenu.kind === "section" && contextMenuSection?.local && (
              <>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => {
                    openSectionChannelMenu(contextMenuSection.id, {
                      left: sidebarContextMenu.x,
                      bottom: sidebarContextMenu.y,
                    });
                  }}
                >
                  <Plus size={14} />
                  {t("Add channel")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                  role="menuitem"
                  onClick={() => startRenameSection(contextMenuSection)}
                >
                  <Pencil size={14} />
                  {t("Rename")}
                </button>
                <button
                  type="button"
                  className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-red-600 hover:bg-red-50"
                  role="menuitem"
                  onClick={() => {
                    setSidebarContextMenu(null);
                    setDeleteSectionId(contextMenuSection.id);
                    setEditingSectionId(null);
                    setSectionTitleDraft("");
                    closeDeleteChannelConfirm();
                  }}
                >
                  <Trash2 size={14} />
                  {t("Delete")}
                </button>
              </>
            )}
          </div>,
          document.body,
        )}
      {sectionChannelMenuId && sectionChannelMenuPosition &&
        createPortal(
          <div
            className="surface-menu surface-menu-origin-top-left fixed z-50 max-h-56 w-60 overflow-y-auto rounded-lg border border-[#dfe3ec] bg-white p-1 text-left text-sm normal-case tracking-normal shadow-[0_14px_36px_rgb(16_24_40_/_0.14)] soft-scrollbar"
            style={{
              right: sectionChannelMenuPosition.right,
              top: sectionChannelMenuPosition.top,
            }}
            role="menu"
            aria-label={t("Channels available for {{section}}", {
              section: sections.find((item) => item.id === sectionChannelMenuId)?.title ?? "",
            })}
            onClick={(event) => event.stopPropagation()}
            onContextMenu={(event) => event.preventDefault()}
          >
            {(() => {
              const section = sections.find((item) => item.id === sectionChannelMenuId);
              if (!section) return null;
              const availableChannels = channels.filter(
                (channel) => !section.channels.some((item) => item.id === channel.id),
              );
              return availableChannels.length === 0 ? (
                <div className="px-3 py-2 text-xs font-medium text-[#8a93a5]">
                  {t("All channels are already in this section.")}
                </div>
              ) : (
                availableChannels.map((channel) => (
                  <button
                    key={channel.id}
                    type="button"
                    className="flex min-h-9 w-full items-center gap-2 rounded-md px-3 py-2 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                    role="menuitem"
                    onClick={() => {
                      onMoveChannelToGroup(channel.id, section.id);
                      setSectionChannelMenuId(null);
                      setSectionChannelMenuPosition(null);
                    }}
                  >
                    <ArrowRight size={14} />
                    <span className="min-w-0 flex-1 truncate">
                      {t("Move #{{channel}} here", { channel: channel.title })}
                    </span>
                  </button>
                ))
              );
            })()}
          </div>,
          document.body,
        )}
      {mobileQrOpen && account && workspace ? (
        <MobileConnectDialog
          account={account}
          workspace={workspace}
          onClose={() => setMobileQrOpen(false)}
        />
      ) : null}
    </aside>
  );
}
