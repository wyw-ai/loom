import { useState, useEffect, useRef } from "react";
import type { FormEvent, MouseEvent, PointerEvent } from "react";
import { createPortal } from "react-dom";
import {
  ChevronDown,
  Folder,
  GripVertical,
  Hash,
  Loader2,
  Pencil,
  Plus,
  Split,
  Trash2,
  Bell,
  Check,
  Home,
  MessageCircle,
  MessageSquare,
  Server,
} from "lucide-react";
import type { Actor, Channel, MachineInfo, Run, Thread } from "@/ipc/types";
import type {
  ChannelContextMenu,
  ChannelGroup,
  ChannelGroupSection,
  ChannelPointerDrag,
  ConnectionState,
  View,
} from "@/lib/types";
import { channelContextMenuHeightPx, channelContextMenuViewportPaddingPx, channelContextMenuWidthPx } from "@/lib/constants";
import { channelGroupSections } from "@/lib/channel-utils";
import { displayName } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ChannelDeleteConfirm } from "@/components/channel/ChannelDeleteConfirm";
import { ActorAvatar } from "@/components/agent/ActorAvatar";
import { getActorRunContext, runStatusAnimationName, runStatusDotClass, runStatusFullLabel, memberPresence } from "@/lib/agent-utils";

export function Sidebar({
  view,
  setView,
  busy,
  channels,
  channelGroups,
  connection,
  hasWorkspace,
  workspaceName,
  activeChannelId,
  activeDirectActorId,
  activeThreadId,
  directAgents,
  runs,
  machines,
  threadsByChannel,
  onAddChannel,
  onAddChannelGroup,
  onMoveChannelToGroup,
  onDeleteChannel,
  onRenameChannel,
  onRemoveChannelGroup,
  onRenameChannelGroup,
  onSelectChannel,
  onSelectDirectAgent,
  onSelectThread,
  onToggleChannelGroup,
}: {
  view: View;
  setView: (view: View) => void;
  busy: string | null;
  channels: Channel[];
  channelGroups: ChannelGroup[];
  connection: ConnectionState;
  hasWorkspace: boolean;
  workspaceName: string | null;
  activeChannelId: string | null;
  activeDirectActorId: string | null;
  activeThreadId: string | null;
  directAgents: Actor[];
  runs: Record<string, Run>;
  machines: MachineInfo[];
  threadsByChannel: Record<string, Thread[]>;
  onAddChannel: (title: string) => void;
  onAddChannelGroup: (title: string) => void;
  onMoveChannelToGroup: (channelId: string, groupId: string) => void;
  onDeleteChannel: (channel: Channel) => void;
  onRenameChannel: (channel: Channel, title: string) => void;
  onRemoveChannelGroup: (groupId: string) => void;
  onRenameChannelGroup: (groupId: string, title: string) => void;
  onSelectChannel: (channelId: string) => void;
  onSelectDirectAgent: (actorId: string) => void;
  onSelectThread: (thread: Thread) => void;
  onToggleChannelGroup: (groupId: string) => void;
}) {
  const [createMenuOpen, setCreateMenuOpen] = useState(false);
  const [createKind, setCreateKind] = useState<"channel" | "section" | null>(null);
  const [createTitle, setCreateTitle] = useState("");
  const [editingSectionId, setEditingSectionId] = useState<string | null>(null);
  const [sectionTitleDraft, setSectionTitleDraft] = useState("");
  const [deleteSectionId, setDeleteSectionId] = useState<string | null>(null);
  const [editingChannelId, setEditingChannelId] = useState<string | null>(null);
  const [channelTitleDraft, setChannelTitleDraft] = useState("");
  const [deleteChannelId, setDeleteChannelId] = useState<string | null>(null);
  const [channelContextMenu, setChannelContextMenu] =
    useState<ChannelContextMenu | null>(null);
  const [draggingChannelId, setDraggingChannelId] = useState<string | null>(null);
  const [dragOverSectionId, setDragOverSectionId] = useState<string | null>(null);
  const dragSessionRef = useRef<ChannelPointerDrag | null>(null);
  const dragListenerCleanupRef = useRef<(() => void) | null>(null);
  const suppressChannelClickRef = useRef<string | null>(null);
  const sections = channelGroupSections(channelGroups, channels);
  const contextMenuChannel = channelContextMenu
    ? channels.find((channel) => channel.id === channelContextMenu.channelId) ?? null
    : null;
  const navItems = [
    { id: "chat" as const, label: "Home", icon: Home },
    { id: "channels" as const, label: "All Channels", icon: Hash },
    { id: "direct" as const, label: "Direct Messages", icon: MessageCircle },
    { id: "threads" as const, label: "Threads", icon: MessageSquare },
    { id: "inbox" as const, label: "Inbox", icon: Bell },
    { id: "tasks" as const, label: "Tasks", icon: Check },
    { id: "settings" as const, label: "Actors", icon: Server },
  ];
  const closeCreateMenu = () => {
    setCreateMenuOpen(false);
    setCreateKind(null);
    setCreateTitle("");
  };

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
    setChannelContextMenu(null);
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
    setChannelContextMenu(null);
    setDeleteChannelId(channelId);
  };

  const startRenameChannel = (channel: Channel) => {
    closeCreateMenu();
    closeDeleteChannelConfirm();
    setChannelContextMenu(null);
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
    cancelChannelDrag();
    closeCreateMenu();
    closeDeleteChannelConfirm();
    closeRenameChannel();
    setDeleteSectionId(null);
    setEditingSectionId(null);
    setSectionTitleDraft("");
    setChannelContextMenu({
      channelId: channel.id,
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
          window.innerHeight - channelContextMenuHeightPx - channelContextMenuViewportPaddingPx,
        ),
      ),
    });
  };

  const sectionIdAtPoint = (x: number, y: number) => {
    const element = document.elementFromPoint(x, y);
    const section = element?.closest("[data-channel-section-id]") as HTMLElement | null;
    return section?.dataset.channelSectionId ?? null;
  };

  const cleanupChannelDragListeners = () => {
    dragListenerCleanupRef.current?.();
    dragListenerCleanupRef.current = null;
  };

  const updateChannelDragAtPoint = (
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
      setDraggingChannelId(session.channelId);
    }
    preventDefault?.();
    setDragOverSectionId(sectionIdAtPoint(clientX, clientY));
  };

  const finishChannelDragAtPoint = (
    clientX: number,
    clientY: number,
    pointerId: number,
    preventDefault?: () => void,
    stopPropagation?: () => void,
  ) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== pointerId) return;
    const didDrag = session.dragging;
    const sectionId = didDrag
      ? sectionIdAtPoint(clientX, clientY) ?? dragOverSectionId
      : null;
    dragSessionRef.current = null;
    cleanupChannelDragListeners();
    if (didDrag) {
      preventDefault?.();
      stopPropagation?.();
      suppressChannelClickRef.current = session.channelId;
      window.setTimeout(() => {
        if (suppressChannelClickRef.current === session.channelId) {
          suppressChannelClickRef.current = null;
        }
      }, 120);
      if (sectionId) onMoveChannelToGroup(session.channelId, sectionId);
    }
    setDraggingChannelId(null);
    setDragOverSectionId(null);
  };

  const cancelChannelDrag = () => {
    cleanupChannelDragListeners();
    dragSessionRef.current = null;
    setDraggingChannelId(null);
    setDragOverSectionId(null);
  };

  const beginChannelDrag = (
    event: PointerEvent<HTMLElement>,
    channelId: string,
  ) => {
    if (event.button !== 0) return;
    cleanupChannelDragListeners();
    dragSessionRef.current = {
      channelId,
      startX: event.clientX,
      startY: event.clientY,
      pointerId: event.pointerId,
      dragging: false,
    };
    const handlePointerMove = (moveEvent: globalThis.PointerEvent) => {
      updateChannelDragAtPoint(
        moveEvent.clientX,
        moveEvent.clientY,
        moveEvent.pointerId,
        () => moveEvent.preventDefault(),
      );
    };
    const handlePointerUp = (upEvent: globalThis.PointerEvent) => {
      finishChannelDragAtPoint(
        upEvent.clientX,
        upEvent.clientY,
        upEvent.pointerId,
        () => upEvent.preventDefault(),
        () => upEvent.stopPropagation(),
      );
    };
    const handlePointerCancel = (cancelEvent: globalThis.PointerEvent) => {
      if (dragSessionRef.current?.pointerId === cancelEvent.pointerId) {
        cancelChannelDrag();
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

  const updateChannelDrag = (event: PointerEvent<HTMLElement>) => {
    updateChannelDragAtPoint(
      event.clientX,
      event.clientY,
      event.pointerId,
      () => event.preventDefault(),
    );
  };

  const finishChannelDrag = (event: PointerEvent<HTMLElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    try {
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    } catch {
      /* Ignore pointer-capture differences across desktop webviews. */
    }
    finishChannelDragAtPoint(
      event.clientX,
      event.clientY,
      event.pointerId,
      () => event.preventDefault(),
      () => event.stopPropagation(),
    );
  };

  useEffect(() => {
    if (!channelContextMenu) return;
    const close = () => setChannelContextMenu(null);
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
  }, [channelContextMenu]);

  useEffect(() => () => cleanupChannelDragListeners(), []);
  return (
    <aside className="flex min-h-0 min-w-0 flex-col bg-[#fbfbfd]">
      <div className="border-b border-[#edf0f5] p-3">
        <div className="space-y-1">
          {navItems.map((item) => {
            const Icon = item.icon;
            const selected = view === item.id;
            return (
              <button
                key={item.id}
                className={cn("nav-row h-9 text-sm", selected && "nav-row-active")}
                onClick={() => {
                  closeCreateMenu();
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

      {view === "direct" && (
        <div className="border-b border-[#edf0f5] p-3">
          <div className="mb-2 flex items-center justify-between px-1">
            <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
              Agents
            </span>
            <span className="count-badge h-5 min-w-5 text-[10px]">
              {directAgents.length}
            </span>
          </div>
          <div className="space-y-1">
            {directAgents.length === 0 ? (
              <div className="px-3 py-2 text-xs text-[#8a93a5]">
                No agents available.
              </div>
            ) : (
              directAgents.map((actor) => {
                const selected = actor.id === activeDirectActorId;
                const ctx = getActorRunContext(runs, actor.id);
                const presence = memberPresence(actor, machines, null);
                const dotClass = ctx ? runStatusDotClass(ctx) : (
                  presence.online ? "bg-green-500" : "bg-[#98a2b3]"
                );
                const animation = runStatusAnimationName(ctx);
                const label = runStatusFullLabel(ctx) ?? presence.label;
                return (
                  <button
                    key={actor.id}
                    type="button"
                    className={cn("nav-row h-10 text-sm", selected && "nav-row-active")}
                    onClick={() => onSelectDirectAgent(actor.id)}
                  >
                    <span className="relative shrink-0">
                      <ActorAvatar actor={actor} fallback={actor.id} small />
                      <span
                        className={cn(
                          "absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full border-2 border-white",
                          dotClass,
                        )}
                        style={animation ? { animationName: animation, animationDuration: "1.5s", animationIterationCount: "infinite", animationTimingFunction: "ease-in-out" } : undefined}
                      />
                    </span>
                    <div className="min-w-0 flex-1">
                      <span className="truncate">{displayName(actor)}</span>
                      {label && (
                        <span className="block truncate text-[10px] text-[#667085]">
                          {label}
                        </span>
                      )}
                    </div>
                  </button>
                );
              })
            )}
          </div>
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="border-b border-[#edf0f5] p-3">
          <div className="flex items-center justify-between px-1">
            <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
              Channels
            </span>
            <div className="relative">
              <button
                type="button"
                className="composer-icon h-6 min-w-6"
                title="Create channel or section"
                aria-haspopup="menu"
                aria-expanded={createMenuOpen}
                onClick={() => {
                  if (createMenuOpen) {
                    closeCreateMenu();
                  } else {
                    setCreateMenuOpen(true);
                  }
                }}
              >
                <Plus size={15} />
              </button>
              {createMenuOpen && (
                <div className="absolute right-0 top-7 z-30 w-64 rounded-lg border border-[#dfe3ec] bg-white p-1 text-sm shadow-soft">
                  {!createKind ? (
                    <>
                      <button
                        type="button"
                        className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                        onClick={() => handleOpenCreate("channel")}
                      >
                        <Hash size={15} />
                        New channel
                      </button>
                      <button
                        type="button"
                        className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
                        onClick={() => handleOpenCreate("section")}
                      >
                        <Folder size={15} />
                        New section
                      </button>
                    </>
                  ) : (
                    <form className="grid gap-2 p-2" onSubmit={handleCreateSubmit}>
                      <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-[#667085]">
                        {createKind === "channel" ? <Hash size={13} /> : <Folder size={13} />}
                        {createKind === "channel" ? "New channel" : "New section"}
                      </div>
                      <Input
                        autoFocus
                        value={createTitle}
                        onChange={(event) => setCreateTitle(event.target.value)}
                        placeholder={createKind === "channel" ? "Channel name" : "Section name"}
                        className="h-9 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                      />
                      {createKind === "channel" && !hasWorkspace && (
                        <div className="text-xs font-medium text-amber-700">
                          Add or select a space before creating a channel.
                        </div>
                      )}
                      {createKind === "channel" && hasWorkspace && connection !== "open" && (
                        <div className="text-xs font-medium text-amber-700">
                          {`Will connect to ${workspaceName ?? "this space"} before creating.`}
                        </div>
                      )}
                      <div className="flex justify-end gap-2 pt-1">
                        <Button type="button" variant="outline" size="sm" onClick={() => setCreateKind(null)}>
                          Back
                        </Button>
                        <Button
                          type="submit"
                          size="sm"
                          disabled={
                            !createTitle.trim() ||
                            (createKind === "channel" && !hasWorkspace)
                          }
                        >
                          {createKind === "channel" && connection !== "open"
                            ? "Connect & create"
                            : "Create"}
                        </Button>
                      </div>
                    </form>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-3 soft-scrollbar">
          {sections.map((section) => (
            <div
              key={section.id}
              data-channel-section-id={section.id}
              className={cn(
                "mb-3 rounded-lg transition-colors",
                draggingChannelId &&
                  dragOverSectionId === section.id &&
                  "channel-drop-target",
              )}
            >
              {(section.local || channelGroups.length > 0) && (
                <div className="channel-group-header group/channelgroup">
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                    onClick={() => section.local && onToggleChannelGroup(section.id)}
                    disabled={!section.local}
                  >
                    {section.local ? (
                      section.collapsed ? (
                        <ChevronDown size={13} className="-rotate-90 text-[#667085]" />
                      ) : (
                        <ChevronDown size={13} className="text-[#667085]" />
                      )
                    ) : (
                      <span className="w-[13px]" />
                    )}
                    <span className="min-w-0 truncate">{section.title}</span>
                    <span className="count-badge ml-1 h-5 min-w-5 text-[10px]">
                      {section.channels.length}
                    </span>
                  </button>
                  {section.local && (
                    <div className="flex items-center gap-1">
                      <button
                        type="button"
                        className="composer-icon h-6 min-w-6"
                        title="Rename section"
                        onClick={() => startRenameSection(section)}
                      >
                        <Pencil size={12} />
                      </button>
                      <button
                        type="button"
                        className="composer-icon h-6 min-w-6 text-red-500 hover:text-red-600"
                        title="Delete section"
                        onClick={() => {
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
                    placeholder="Section name"
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
                    Cancel
                  </Button>
                  <Button type="submit" size="sm" disabled={!sectionTitleDraft.trim()}>
                    Save
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
                    Cancel
                  </Button>
                  <Button type="button" size="sm" onClick={() => confirmDeleteSection(section.id)}>
                    Delete
                  </Button>
                </div>
              )}
              {!section.collapsed && (
                <div className="mt-1 space-y-1">
                  {section.channels.length === 0 ? (
                    <div className="px-3 py-2 text-xs text-[#8a93a5]">
                      {section.local ? "Drop channels here." : "No channels yet."}
                    </div>
                  ) : (
                    section.channels.map((channel) => {
                      const selected = channel.id === activeChannelId && !activeThreadId;
                      const threads = threadsByChannel[channel.id] ?? [];
                      const deleteBusy = busy === `channel:delete:${channel.id}`;
                      const renameBusy = busy === `channel:rename:${channel.id}`;
                      return (
                        <div
                          key={channel.id}
                          className={cn(
                            "group/channel",
                            draggingChannelId === channel.id && "opacity-45",
                          )}
                        >
                          <div className="flex items-center gap-1">
                            <button
                              className={cn(
                                "channel-row min-w-0 flex-1 touch-none select-none",
                                draggingChannelId === channel.id && "cursor-grabbing",
                                selected && "channel-row-active",
                              )}
                              onPointerDown={(event) => beginChannelDrag(event, channel.id)}
                              onPointerMove={updateChannelDrag}
                              onPointerUp={finishChannelDrag}
                              onPointerCancel={cancelChannelDrag}
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
                                placeholder="Channel name"
                                className="h-8 rounded-lg border-[#dfe3ec] bg-white text-xs shadow-none"
                              />
                              <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                disabled={renameBusy}
                                onClick={closeRenameChannel}
                              >
                                Cancel
                              </Button>
                              <Button
                                type="submit"
                                size="sm"
                                disabled={!channelTitleDraft.trim() || renameBusy}
                              >
                                {renameBusy ? (
                                  <Loader2 className="animate-spin" size={13} />
                                ) : (
                                  "Save"
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
                                    activeThreadId === thread.id && "bg-[#eeeaff] text-[#5843d7]",
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
                      );
                    })
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
      {channelContextMenu &&
        contextMenuChannel &&
        createPortal(
          <div
            className="fixed z-50 rounded-lg border border-[#dfe3ec] bg-white p-1 text-sm shadow-soft"
            style={{
              left: channelContextMenu.x,
              top: channelContextMenu.y,
              width: channelContextMenuWidthPx,
            }}
            role="menu"
            aria-label={`Channel actions for ${contextMenuChannel.title}`}
            onClick={(event) => event.stopPropagation()}
            onContextMenu={(event) => event.preventDefault()}
          >
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-[#303849] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
              role="menuitem"
              onClick={() => startRenameChannel(contextMenuChannel)}
            >
              <Pencil size={14} />
              Rename
            </button>
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-md px-3 text-left font-semibold text-red-600 hover:bg-red-50"
              role="menuitem"
              onClick={() => requestDeleteChannel(contextMenuChannel.id)}
            >
              <Trash2 size={14} />
              Delete
            </button>
          </div>,
          document.body,
        )}
    </aside>
  );
}
