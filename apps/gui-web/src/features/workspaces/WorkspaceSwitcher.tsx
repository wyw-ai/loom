import { useEffect, useMemo, useState } from "react";
import clsx from "clsx";
import { Check, Loader2, Plus, Search, Server, X } from "lucide-react";

import { type Workspace } from "@/ipc/types";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";
import { openAddWorkspaceModal } from "./AddWorkspaceModal";
import { connectWorkspace } from "./connect";

export function openWorkspaceSwitcher() {
  useWorkspaces.getState().setSwitchOpen(true);
}

export function WorkspaceSwitcherHost() {
  const open = useWorkspaces((s) => s.switchOpen);
  const workspaces = useWorkspaces((s) => s.workspaces);
  const activeId = useWorkspaces((s) => s.activeId);
  const connectingId = useWorkspaces((s) => s.connectingId);
  const sessionWorkspace = useSession((s) => s.workspace);
  const connection = useSession((s) => s.connection);
  const [filter, setFilter] = useState("");

  const close = () => useWorkspaces.getState().setSwitchOpen(false);

  useEffect(() => {
    if (!open) return;
    setFilter("");
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return workspaces;
    return workspaces.filter((w) =>
      [w.name, w.serverUrl, w.actorId, w.displayName]
        .join("\n")
        .toLowerCase()
        .includes(q),
    );
  }, [filter, workspaces]);

  if (!open) return null;

  const connect = (workspace: Workspace) => {
    useUI.getState().setView("chat");
    close();
    if (connection === "open" && sessionWorkspace?.id === workspace.id) {
      return;
    }
    void connectWorkspace(workspace.id);
  };

  const add = () => {
    close();
    openAddWorkspaceModal();
  };

  return (
    <div
      className="fixed inset-0 z-[54] flex items-center justify-center overflow-y-auto bg-black/60 p-4"
      onClick={close}
    >
      <section
        className="card-brutal w-[calc(100vw-2rem)] max-w-xl p-5"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between gap-4">
          <div className="flex min-w-0 items-center gap-3">
            <div className="btn-brutal-sm h-9 w-9 shrink-0 bg-brutal-yellow">
              <Server size={17} />
            </div>
            <h2 className="truncate text-lg font-black uppercase text-black">
              Switch server
            </h2>
          </div>
          <button
            type="button"
            className="btn-brutal-sm bg-white p-1"
            onClick={close}
            aria-label="Close"
          >
            <X size={18} />
          </button>
        </div>

        <div className="input-brutal flex items-center gap-2">
          <Search size={14} className="shrink-0 text-black/45" />
          <input
            autoFocus
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Search servers..."
            className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-black/40"
          />
        </div>

        <div className="mt-3 max-h-[360px] overflow-y-auto border-2 border-black bg-white">
          {visible.length === 0 ? (
            <div className="px-3 py-4 text-sm font-mono text-black/40">
              no matches
            </div>
          ) : (
            visible.map((workspace) => {
              const connected =
                connection === "open" && sessionWorkspace?.id === workspace.id;
              const selected = connected || activeId === workspace.id;
              const connecting = connectingId === workspace.id;
              return (
                <button
                  key={workspace.id}
                  type="button"
                  className={clsx(
                    "flex w-full items-center gap-3 border-b-2 border-black px-3 py-3 text-left last:border-b-0 hover:bg-brutal-cream",
                    selected && "bg-brutal-yellow",
                  )}
                  onClick={() => connect(workspace)}
                >
                  <WorkspaceMark workspace={workspace} current={connected} />
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 items-center gap-2">
                      <span className="truncate text-sm font-black text-black">
                        {workspace.name}
                      </span>
                      {connected ? (
                        <span className="chip-brutal shrink-0 bg-brutal-lime">
                          current
                        </span>
                      ) : selected ? (
                        <span className="chip-brutal shrink-0 bg-white">
                          selected
                        </span>
                      ) : null}
                    </div>
                    <div className="truncate font-mono text-[11px] text-black/55">
                      {workspace.serverUrl}
                    </div>
                    <div className="truncate text-[11px] text-black/45">
                      as {workspace.displayName || workspace.actorId}
                    </div>
                  </div>
                  <div className="flex h-8 w-8 shrink-0 items-center justify-center">
                    {connecting ? (
                      <Loader2 size={16} className="animate-spin" />
                    ) : connected ? (
                      <Check size={18} />
                    ) : (
                      <span className="font-mono text-[11px] font-black uppercase text-black/45">
                        open
                      </span>
                    )}
                  </div>
                </button>
              );
            })
          )}
        </div>

        <button
          type="button"
          className="btn-brutal mt-4 w-full gap-2 bg-brutal-cyan px-3 py-2 text-sm"
          onClick={add}
        >
          <Plus size={16} />
          Add workspace
        </button>
      </section>
    </div>
  );
}

function WorkspaceMark({
  workspace,
  current,
}: {
  workspace: Workspace;
  current: boolean;
}) {
  const initial =
    workspace.name
      .split(/\s+/)
      .map((s) => s[0])
      .filter(Boolean)
      .slice(0, 1)
      .join("")
      .toUpperCase() || "S";

  return (
    <div
      className={clsx(
        "relative flex h-10 w-10 shrink-0 items-center justify-center border-2 border-black text-base font-black",
        current ? "bg-black text-brutal-yellow" : "bg-brutal-cream text-black",
      )}
    >
      {initial}
    </div>
  );
}
