import { Plus, Server } from "lucide-react";

import { useSession } from "@/store/session";
import { useWorkspaces } from "@/store/workspaces";
import { openAddWorkspaceModal } from "@/features/workspaces/AddWorkspaceModal";
import { connectWorkspace } from "@/features/workspaces/connect";

export function Landing() {
  const workspaces = useWorkspaces((s) => s.workspaces);
  const connection = useSession((s) => s.connection);
  const error = useSession((s) => s.error);
  const connecting = useWorkspaces((s) => s.connectingId);

  return (
    <div className="flex h-full flex-col items-center justify-center p-10">
      <div className="w-[520px] max-w-full">
        <div className="mb-6 flex items-center gap-3">
          <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-accent text-accent-contrast">
            <Server size={22} />
          </div>
          <div>
            <h1 className="text-xl font-semibold text-primary">
              Welcome to Joi Desktop
            </h1>
            <p className="text-sm text-muted">
              Pick a workspace on the left to connect, or add a new one.
            </p>
          </div>
        </div>

        {workspaces.length === 0 ? (
          <EmptyState />
        ) : (
          <div className="rounded-lg border border-border bg-elevated">
            {workspaces.map((w) => {
              const isBusy = connecting === w.id;
              return (
                <button
                  key={w.id}
                  onClick={() => void connectWorkspace(w.id)}
                  className="flex w-full items-center gap-3 border-b border-border/60 px-4 py-3 text-left last:border-b-0 hover:bg-hover"
                >
                  <WorkspaceIcon name={w.name} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-primary">
                      {w.name}
                    </div>
                    <div className="truncate text-xs text-muted font-mono">
                      {w.serverUrl}
                    </div>
                    <div className="truncate text-[11px] text-muted">
                      as {w.actorId}
                    </div>
                  </div>
                  <span className="text-xs text-muted">
                    {isBusy ? "connecting…" : "Open"}
                  </span>
                </button>
              );
            })}
            <button
              onClick={openAddWorkspaceModal}
              className="flex w-full items-center gap-2 px-4 py-3 text-sm text-accent hover:bg-hover"
            >
              <Plus size={16} />
              Add workspace
            </button>
          </div>
        )}

        {connection === "error" && error && (
          <p className="mt-4 rounded border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="rounded-lg border border-dashed border-border bg-elevated/60 p-6 text-center">
      <p className="mb-4 text-sm text-secondary">
        No workspaces yet. Point at a running <code className="font-mono">joi-server</code> to get started.
      </p>
      <button
        onClick={openAddWorkspaceModal}
        className="inline-flex items-center gap-2 rounded bg-accent px-4 py-2 text-sm text-accent-contrast hover:bg-accent-hover"
      >
        <Plus size={16} />
        Add your first workspace
      </button>
    </div>
  );
}

function WorkspaceIcon({ name }: { name: string }) {
  const initials = name
    .split(/\s+/)
    .map((s) => s[0])
    .filter(Boolean)
    .slice(0, 2)
    .join("")
    .toUpperCase() || "W";
  return (
    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-accent/80 text-sm font-semibold text-accent-contrast">
      {initials}
    </div>
  );
}
