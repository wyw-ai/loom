import clsx from "clsx";
import { Bell, Plus, Settings } from "lucide-react";

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
import * as ipc from "@/ipc/bridge";

export function ServerRail() {
  const workspaces = useWorkspaces((s) => s.workspaces);
  const activeId = useWorkspaces((s) => s.activeId);
  const connectingId = useWorkspaces((s) => s.connectingId);
  const connection = useSession((s) => s.connection);
  const unseen = useInbox((s) => s.items.filter((x) => !x.seen).length);
  const view = useUI((s) => s.view);
  const setView = useUI((s) => s.setView);
  const openContextMenu = useUI((s) => s.openContextMenu);
  const pushToast = useUI((s) => s.pushToast);

  const toggleInbox = () =>
    setView(view === "inbox" ? "chat" : "inbox");

  return (
    <nav className="flex h-full w-[72px] shrink-0 flex-col items-center gap-2 overflow-hidden border-r border-border bg-servers py-3">
      {workspaces.map((w) => (
        <WorkspaceDot
          key={w.id}
          workspace={w}
          active={activeId === w.id && connection === "open"}
          connecting={connectingId === w.id}
          error={activeId === w.id && connection === "error"}
          onClick={() => {
            // Always land on the chat view — if the user bounced off to
            // Inbox/Settings and then clicked the workspace icon, they
            // expect to be back where the channels are.
            setView("chat");
            if (activeId === w.id && connection === "open") return;
            void connectWorkspace(w.id);
          }}
          onContextMenu={(e) => {
            e.preventDefault();
            openContextMenu({
              x: e.clientX,
              y: e.clientY,
              items: [
                {
                  kind: "item",
                  label: "Reconnect",
                  onClick: () => void connectWorkspace(w.id),
                },
                {
                  kind: "item",
                  label: "Disconnect",
                  disabled: activeId !== w.id || connection !== "open",
                  onClick: () => void disconnectWorkspace(),
                },
                { kind: "divider" },
                {
                  kind: "item",
                  label: "Remove…",
                  danger: true,
                  onClick: () =>
                    confirmRemoveWorkspace(w.id, w.name),
                },
              ],
            });
          }}
        />
      ))}

      <button
        aria-label="Add workspace"
        onClick={openAddWorkspaceModal}
        className="flex h-12 w-12 items-center justify-center rounded-xl bg-elevated text-secondary transition-colors hover:bg-success hover:text-white"
      >
        <Plus size={22} />
      </button>

      {workspaces.length > 0 && <div className="my-1 h-px w-8 bg-border" />}

      <button
        aria-label="Inbox"
        disabled={connection !== "open"}
        className={clsx(
          "relative flex h-12 w-12 items-center justify-center rounded-xl transition-colors",
          view === "inbox"
            ? "bg-warning/20 text-warning"
            : "bg-elevated text-secondary hover:bg-warning/20 hover:text-warning",
          connection !== "open" && "opacity-40",
        )}
        onClick={toggleInbox}
      >
        <Bell size={20} />
        {unseen > 0 && (
          <span className="absolute -right-1 -top-1 min-w-[18px] rounded-full bg-danger px-1 text-center text-[10px] font-semibold leading-[18px] text-white">
            {unseen > 99 ? "99+" : unseen}
          </span>
        )}
      </button>

      <div className="flex-1" />

      <button
        aria-label="Settings"
        className="flex h-12 w-12 items-center justify-center rounded-xl bg-elevated text-secondary transition-colors hover:bg-accent hover:text-accent-contrast"
        onClick={() =>
          pushToast("info", "Settings page not implemented yet")
        }
      >
        <Settings size={20} />
      </button>
    </nav>
  );
}

function WorkspaceDot({
  workspace,
  active,
  connecting,
  error,
  onClick,
  onContextMenu,
}: {
  workspace: Workspace;
  active: boolean;
  connecting: boolean;
  error: boolean;
  onClick: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
}) {
  const initials =
    workspace.name
      .split(/\s+/)
      .map((s) => s[0])
      .filter(Boolean)
      .slice(0, 2)
      .join("")
      .toUpperCase() || "W";

  const dotColor = connecting
    ? "bg-warning"
    : error
    ? "bg-danger"
    : active
    ? "bg-success"
    : "bg-muted";

  return (
    <div className="group relative flex h-12 w-12 items-center justify-center">
      <span
        className={clsx(
          "pointer-events-none absolute -left-3 h-8 w-1 rounded-r bg-primary transition-opacity",
          active ? "opacity-100" : "opacity-0 group-hover:opacity-40",
        )}
      />
      <button
        title={`${workspace.name}\n${workspace.serverUrl}`}
        onClick={onClick}
        onContextMenu={onContextMenu}
        className={clsx(
          "relative h-12 w-12 rounded-xl outline-none transition-transform focus-visible:ring-2 focus-visible:ring-accent",
          !active && "hover:translate-y-[-1px]",
        )}
      >
        <span
          className={clsx(
            "absolute inset-0 rounded-xl transition-colors",
            active ? "bg-accent" : "bg-elevated group-hover:bg-accent",
          )}
        />
        <span
          className={clsx(
            "relative z-10 flex h-full w-full items-center justify-center rounded-xl text-sm font-semibold transition-colors",
            active
              ? "text-accent-contrast"
              : "text-secondary group-hover:text-accent-contrast",
          )}
        >
          {initials}
        </span>
        <span
          className={clsx(
            "absolute -bottom-0.5 -right-0.5 z-20 h-3 w-3 rounded-full border-2 border-servers",
            dotColor,
          )}
        />
      </button>
    </div>
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
