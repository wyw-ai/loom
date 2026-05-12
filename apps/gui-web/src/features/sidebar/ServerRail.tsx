import clsx from "clsx";
import type { MouseEvent } from "react";
import {
  type LucideIcon,
  MessageSquare,
  Monitor,
  Plus,
  Settings,
  SquareCheckBig,
  TriangleAlert,
  Users,
} from "lucide-react";

import { useInbox } from "@/store/inbox";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";
import type { Workspace } from "@/ipc/types";
import {
  connectWorkspace,
  disconnectWorkspace,
} from "@/features/workspaces/connect";
import { openAddWorkspaceModal } from "@/features/workspaces/AddWorkspaceModal";
import { openWorkspaceSwitcher } from "@/features/workspaces/WorkspaceSwitcher";
import * as ipc from "@/ipc/bridge";

type RailView = "chat" | "tasks" | "members" | "machines";

const railItems: Array<{
  view: RailView;
  label: string;
  icon: LucideIcon;
}> = [
  { view: "chat", label: "Chat", icon: MessageSquare },
  { view: "tasks", label: "Tasks", icon: SquareCheckBig },
  { view: "members", label: "Members", icon: Users },
  { view: "machines", label: "Machines", icon: Monitor },
];

export function ServerRail() {
  const workspaces = useWorkspaces((s) => s.workspaces);
  const activeId = useWorkspaces((s) => s.activeId);
  const connectingId = useWorkspaces((s) => s.connectingId);
  const connection = useSession((s) => s.connection);
  const pendingInboxItems = useInbox((s) => s.items.length);
  const unseen = useInbox((s) => s.items.filter((x) => !x.seen).length);
  const view = useUI((s) => s.view);
  const setView = useUI((s) => s.setView);
  const openContextMenu = useUI((s) => s.openContextMenu);

  const activeWorkspace =
    workspaces.find((w) => w.id === activeId) ?? workspaces[0] ?? null;

  return (
    <nav className="relative hidden h-full w-16 shrink-0 select-none flex-col items-center border-r-2 border-black bg-brutal-yellow md:flex">
      <div className="flex h-panel-header w-full items-center justify-center border-b-2 border-black">
        {activeWorkspace ? (
          <WorkspaceButton
            workspace={activeWorkspace}
            active={activeId === activeWorkspace.id && connection === "open"}
            connecting={connectingId === activeWorkspace.id}
            onClick={openWorkspaceSwitcher}
            onContextMenu={(e) => {
              e.preventDefault();
              openContextMenu({
                x: e.clientX,
                y: e.clientY,
                items: [
                  {
                    kind: "item",
                    label: "Reconnect",
                    onClick: () => void connectWorkspace(activeWorkspace.id),
                  },
                  {
                    kind: "item",
                    label: "Disconnect",
                    disabled:
                      activeId !== activeWorkspace.id || connection !== "open",
                    onClick: () => void disconnectWorkspace(),
                  },
                  { kind: "divider" },
                  {
                    kind: "item",
                    label: "Add workspace...",
                    onClick: openAddWorkspaceModal,
                  },
                  {
                    kind: "item",
                    label: "Remove...",
                    danger: true,
                    onClick: () =>
                      confirmRemoveWorkspace(
                        activeWorkspace.id,
                        activeWorkspace.name,
                      ),
                  },
                ],
              });
            }}
          />
        ) : (
          <button
            aria-label="Add workspace"
            className="btn-brutal h-10 w-10 bg-black text-brutal-yellow"
            onClick={openAddWorkspaceModal}
          >
            <Plus size={18} />
          </button>
        )}
      </div>

      <div className="flex w-full flex-1 flex-col items-center gap-1.5 py-2">
        {railItems.map((item) => {
          const Icon = item.icon;
          const active = view === item.view;
          return (
            <button
              key={item.view}
              type="button"
              title={item.label}
              aria-label={item.label}
              aria-pressed={active}
              className={clsx(
                "flex h-10 w-10 items-center justify-center border-2 transition-colors",
                active
                  ? "border-black bg-white shadow-brutal-sm"
                  : "border-transparent hover:border-black hover:bg-white/70",
              )}
              onClick={() => setView(item.view)}
            >
              <Icon size={18} />
            </button>
          );
        })}
      </div>

      {pendingInboxItems > 0 && (
        <button
          aria-label={`Pending action requests (${pendingInboxItems} pending${
            unseen > 0 ? `, ${unseen} new` : ""
          })`}
          title="Pending action requests"
          className={clsx(
            "btn-brutal-sm relative mb-3 h-9 w-9 bg-brutal-orange",
            view === "inbox" && "bg-white",
          )}
          onClick={() => setView(view === "inbox" ? "chat" : "inbox")}
        >
          <TriangleAlert size={18} />
          {unseen > 0 && (
            <span className="absolute -right-1 -top-1 h-2.5 w-2.5 rounded-full border border-black bg-brutal-orange" />
          )}
        </button>
      )}

      <button
        aria-label="Settings"
        title="Settings"
        aria-pressed={view === "settings"}
        className={clsx(
          "mb-2 flex h-10 w-10 items-center justify-center border-2 transition-colors",
          view === "settings"
            ? "border-black bg-white shadow-brutal-sm"
            : "border-transparent hover:border-black hover:bg-white/70",
        )}
        onClick={() => setView("settings")}
      >
        <Settings size={18} />
      </button>
    </nav>
  );
}

function WorkspaceButton({
  workspace,
  active,
  connecting,
  onClick,
  onContextMenu,
}: {
  workspace: Workspace;
  active: boolean;
  connecting: boolean;
  onClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
}) {
  const initial =
    workspace.name
      .split(/\s+/)
      .map((s) => s[0])
      .filter(Boolean)
      .slice(0, 1)
      .join("")
      .toUpperCase() || "B";

  return (
    <button
      title={`${workspace.name}\n${workspace.serverUrl}`}
      aria-label={`Switch server (current: ${workspace.name})`}
      onClick={onClick}
      onContextMenu={onContextMenu}
      className="btn-brutal relative h-10 w-10 bg-black text-base font-black text-brutal-yellow"
    >
      {initial}
      <span
        className={clsx(
          "absolute -right-1 -top-1 h-2.5 w-2.5 rounded-full border border-black",
          connecting ? "bg-brutal-orange" : active ? "bg-brutal-lime" : "bg-white",
        )}
      />
    </button>
  );
}

function confirmRemoveWorkspace(id: string, name: string) {
  useUI.getState().openModal({
    type: "confirm",
    title: `Remove "${name}"?`,
    body: "This only removes the local profile. The server and its data are untouched.",
    confirmLabel: "Remove",
    danger: true,
    onConfirm: async () => {
      const cfg = await ipc.workspaceRemove(id);
      useWorkspaces.getState().setConfig(cfg);
      const active = useSession.getState().workspace;
      if (active?.id === id) {
        await disconnectWorkspace();
      }
    },
  });
}
