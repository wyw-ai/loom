import { create } from "zustand";

import type { ScopeRef } from "@/ipc/types";
import { scopeKey } from "@/ipc/types";

type View = "chat" | "inbox" | "settings";

export type ModalSpec =
  | {
      type: "input";
      title: string;
      label?: string;
      placeholder?: string;
      initial?: string;
      confirmLabel?: string;
      danger?: boolean;
      onSubmit: (value: string) => void | Promise<void>;
    }
  | {
      type: "confirm";
      title: string;
      body: string;
      confirmLabel?: string;
      danger?: boolean;
      onConfirm: () => void | Promise<void>;
    }
  | {
      type: "picker";
      title: string;
      items: Array<{ id: string; label: string; hint?: string }>;
      onPick: (id: string) => void | Promise<void>;
    }
  | {
      type: "quickSwitch";
    };

export interface ContextMenuSpec {
  x: number;
  y: number;
  items: Array<
    | { kind: "divider" }
    | {
        kind: "item";
        label: string;
        danger?: boolean;
        disabled?: boolean;
        onClick: () => void;
      }
  >;
}

interface UIState {
  view: View;
  sidebarVisible: boolean;
  membersVisible: boolean;
  drafts: Record<string, string>;
  // `actorId` is the author of the target event. `handoffTarget`, when
  // present, is the explicit actor target of that event. The Prompt uses
  // `handoffTarget ?? actorId` to auto-attach a `hands_off_to` relation when
  // replying, mirroring crates/cli chat `message_relations` while preserving
  // replies to handoff bubbles.
  replyTargets: Record<
    string,
    {
      eventId: string;
      actorId: string;
      preview: string;
      handoffTarget?: string;
    } | null
  >;
  toast?: { level: "info" | "warn" | "error"; message: string; id: number };
  modal: ModalSpec | null;
  contextMenu: ContextMenuSpec | null;

  setView: (v: View) => void;
  toggleSidebar: () => void;
  toggleMembers: () => void;
  setDraft: (scope: ScopeRef, text: string) => void;
  setReplyTarget: (
    scope: ScopeRef,
    target: {
      eventId: string;
      actorId: string;
      preview: string;
      handoffTarget?: string;
    } | null,
  ) => void;
  pushToast: (level: "info" | "warn" | "error", message: string) => void;
  clearToast: () => void;
  openModal: (m: ModalSpec) => void;
  closeModal: () => void;
  openContextMenu: (m: ContextMenuSpec) => void;
  closeContextMenu: () => void;
}

let toastCounter = 0;

export const useUI = create<UIState>((set) => ({
  view: "chat",
  sidebarVisible: true,
  membersVisible: false,
  drafts: {},
  replyTargets: {},
  toast: undefined,
  modal: null,
  contextMenu: null,

  setView: (v) => set({ view: v }),
  toggleSidebar: () => set((s) => ({ sidebarVisible: !s.sidebarVisible })),
  toggleMembers: () => set((s) => ({ membersVisible: !s.membersVisible })),
  setDraft: (scope, text) =>
    set((s) => ({ drafts: { ...s.drafts, [scopeKey(scope)]: text } })),
  setReplyTarget: (scope, target) =>
    set((s) => ({
      replyTargets: { ...s.replyTargets, [scopeKey(scope)]: target },
    })),
  pushToast: (level, message) =>
    set({ toast: { level, message, id: ++toastCounter } }),
  clearToast: () => set({ toast: undefined }),
  openModal: (m) => set({ modal: m }),
  closeModal: () => set({ modal: null }),
  openContextMenu: (m) => set({ contextMenu: m }),
  closeContextMenu: () => set({ contextMenu: null }),
}));
