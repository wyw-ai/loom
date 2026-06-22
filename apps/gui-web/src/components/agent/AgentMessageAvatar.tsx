import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import type { Actor, MachineInfo, Run } from "@/ipc/types";
import { findAgentMemberEntry } from "@/lib/format-utils";
import { getActorRunContext } from "@/lib/agent-utils";
import { agentIdentityBadgeProps } from "@/lib/agent-identity-utils";
import { useAgentUsage } from "@/store/usageStore";
import { displayName } from "@/lib/format-utils";
import {
  agentMessageBadgeCompactWidth,
  agentMessageBadgeCompactHeight,
  agentMessageBadgeDetailWidth,
  agentMessageBadgeDetailHeight,
  agentMessageBadgePopoverScale,
} from "@/lib/constants";
import { AgentIdentityBadge } from "@/components/agent/AgentIdentityBadge";
import { ActorAvatar } from "@/components/agent/ActorAvatar";

export function AgentMessageAvatar({
  actor,
  fallback,
  machines,
  runs,
  onOpenAgentSettings,
  preferredPlacement = "right",
  small,
}: {
  actor?: Actor;
  fallback: string;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  onOpenAgentSettings: (actorId: string) => void;
  preferredPlacement?: "left" | "right";
  small?: boolean;
}) {
  const actorId = actor?.id ?? fallback;
  const entry = findAgentMemberEntry(machines, actorId);
  const avatarActor = actor ?? entry?.agent.spec.actor;
  const entryActorId = entry?.agent.spec.actor.id ?? null;
  const usageSnapshot = useAgentUsage(entryActorId);
  const [open, setOpen] = useState(false);
  const [badgeExpanded, setBadgeExpanded] = useState(false);
  const [popoverStyle, setPopoverStyle] = useState<CSSProperties>({});
  const anchorRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const closeTimerRef = useRef<number | null>(null);

  const clearCloseTimer = useCallback(() => {
    if (closeTimerRef.current === null) return;
    window.clearTimeout(closeTimerRef.current);
    closeTimerRef.current = null;
  }, []);

  const updatePopoverPosition = useCallback((expanded = badgeExpanded) => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const margin = 16;
    const gap = 12;
    const fallbackWidth =
      (expanded ? agentMessageBadgeDetailWidth : agentMessageBadgeCompactWidth) *
      agentMessageBadgePopoverScale;
    const fallbackHeight =
      (expanded ? agentMessageBadgeDetailHeight : agentMessageBadgeCompactHeight) *
      agentMessageBadgePopoverScale;
    const content = popoverRef.current?.firstElementChild;
    const contentRect = content?.getBoundingClientRect();
    const visualWidth =
      contentRect && contentRect.width > 0
        ? Math.min(contentRect.width, window.innerWidth - margin * 2)
        : Math.min(fallbackWidth, window.innerWidth - margin * 2);
    const visualHeight =
      contentRect && contentRect.height > 0 ? contentRect.height : fallbackHeight;
    const maxVisualHeight = Math.max(120, window.innerHeight - margin * 2);
    const clampedVisualHeight = Math.min(visualHeight, maxVisualHeight);
    let left =
      preferredPlacement === "left"
        ? rect.left - visualWidth - gap
        : rect.right + gap;

    if (left + visualWidth > window.innerWidth - margin) {
      left = rect.left - visualWidth - gap;
    }
    if (left < margin) {
      left = Math.min(window.innerWidth - margin - visualWidth, rect.right + gap);
    }
    if (left < margin) left = margin;

    const maxTop = Math.max(margin, window.innerHeight - margin - clampedVisualHeight);
    const top = Math.max(margin, Math.min(rect.top - 12, maxTop));
    const availableVisualHeight = Math.max(120, window.innerHeight - top - margin);
    setPopoverStyle({
      left,
      top,
      "--agent-message-avatar-popover-max-height": `${availableVisualHeight / agentMessageBadgePopoverScale}px`,
    } as CSSProperties);
  }, [badgeExpanded, preferredPlacement]);

  const openPopover = useCallback(() => {
    clearCloseTimer();
    if (!open) setBadgeExpanded(false);
    updatePopoverPosition(false);
    setOpen(true);
  }, [clearCloseTimer, open, updatePopoverPosition]);

  const scheduleClose = useCallback(() => {
    clearCloseTimer();
    closeTimerRef.current = window.setTimeout(() => setOpen(false), 180);
  }, [clearCloseTimer]);

  useEffect(() => {
    if (!open) return;
    updatePopoverPosition();
    const reposition = () => updatePopoverPosition();
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  }, [open, updatePopoverPosition]);

  useLayoutEffect(() => {
    if (!open) return;
    updatePopoverPosition();
    const popover = popoverRef.current;
    if (!popover || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => updatePopoverPosition());
    observer.observe(popover);
    if (popover.firstElementChild) observer.observe(popover.firstElementChild);
    return () => observer.disconnect();
  }, [badgeExpanded, open, updatePopoverPosition]);

  useEffect(() => {
    if (!open) setBadgeExpanded(false);
  }, [open]);

  useEffect(() => () => clearCloseTimer(), [clearCloseTimer]);

  if (!entry || avatarActor?.kind !== "agent") {
    return <ActorAvatar actor={actor} fallback={fallback} small={small} />;
  }

  const badgeProps = agentIdentityBadgeProps(
    entry,
    getActorRunContext(runs, entry.agent.spec.actor.id),
    usageSnapshot,
  );
  const settingsActorId = entry.agent.spec.actor.id;
  const display = displayName(avatarActor);

  function openSettings() {
    setOpen(false);
    onOpenAgentSettings(settingsActorId);
  }

  const scaledPopoverStyle = {
    ...popoverStyle,
    "--agent-message-avatar-popover-scale": agentMessageBadgePopoverScale,
  } as CSSProperties;

  const popover =
    open && typeof document !== "undefined"
      ? createPortal(
          <div
            ref={popoverRef}
            className="agent-message-avatar-popover"
            style={scaledPopoverStyle}
            onFocus={openPopover}
            onMouseEnter={openPopover}
            onMouseLeave={scheduleClose}
            onKeyDown={(event) => {
              if (event.key === "Escape") setOpen(false);
            }}
          >
            <AgentIdentityBadge
              {...badgeProps}
              onAvatarClick={openSettings}
              onExpandedChange={setBadgeExpanded}
            />
          </div>,
          document.body,
        )
      : null;

  return (
    <>
      <span className="agent-message-avatar">
        <button
          ref={anchorRef}
          type="button"
          className="agent-message-avatar__button"
          title={`Open ${display} agent settings`}
          aria-label={`Open ${display} agent settings`}
          aria-haspopup="dialog"
          aria-expanded={open}
          onClick={openSettings}
          onFocus={openPopover}
          onBlur={scheduleClose}
          onMouseEnter={openPopover}
          onMouseLeave={scheduleClose}
        >
          <ActorAvatar actor={avatarActor} fallback={fallback} small={small} />
        </button>
      </span>
      {popover}
    </>
  );
}
