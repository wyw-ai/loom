import { useEffect, useState } from "react";
import clsx from "clsx";

import * as ipc from "@/ipc/bridge";
import { useWorkspaces } from "@/store/workspaces";
import { connectWorkspace } from "./connect";

// A separate flag in the workspaces store drives this modal's visibility.
// Keeping it outside the generic ModalHost lets us carry four fields with
// inline validation instead of squeezing the flow into the single-input
// `InputModal` spec.

export function openAddWorkspaceModal() {
  useWorkspaces.getState().setAddOpen(true);
}

export function AddWorkspaceHost() {
  const open = useWorkspaces((s) => s.addOpen);
  const close = () => useWorkspaces.getState().setAddOpen(false);

  const [name, setName] = useState("");
  const [serverUrl, setServerUrl] = useState("ws://127.0.0.1:7878/rpc");
  const [actorId, setActorId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setName("");
    setActorId(suggestActorId());
    setDisplayName("you");
    setServerUrl("ws://127.0.0.1:7878/rpc");
    setError(null);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (!name.trim() || !serverUrl.trim() || !actorId.trim()) {
      setError("name, server URL and actor id are required");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const cfg = await ipc.workspaceAdd({
        name: name.trim(),
        serverUrl: serverUrl.trim(),
        actorId: actorId.trim(),
        displayName: displayName.trim() || actorId.trim(),
        activate: true,
      });
      useWorkspaces.getState().setConfig(cfg);
      const newId = cfg.workspaces[cfg.workspaces.length - 1]?.id;
      close();
      if (newId) void connectWorkspace(newId);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[55] flex items-center justify-center bg-black/60"
      onClick={close}
    >
      <form
        onClick={(e) => e.stopPropagation()}
        onSubmit={submit}
        className="w-[480px] max-w-[90vw] rounded-lg border border-border bg-elevated p-5 shadow-[0_8px_24px_rgba(0,0,0,.45)]"
      >
        <h3 className="mb-1 text-base font-semibold text-primary">Add workspace</h3>
        <p className="mb-4 text-xs text-muted">
          Point at a running <code className="font-mono">joi-server</code>.
          You can add several and switch between them in the left rail.
        </p>

        <Field label="Name">
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Local dev"
            className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent"
          />
        </Field>

        <Field label="Server URL">
          <input
            value={serverUrl}
            onChange={(e) => setServerUrl(e.target.value)}
            placeholder="ws://127.0.0.1:7878/rpc"
            className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent font-mono"
          />
        </Field>

        <Field label="Actor id">
          <input
            value={actorId}
            onChange={(e) => setActorId(e.target.value)}
            placeholder="actor_human_abcdef12"
            className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent font-mono"
          />
        </Field>

        <Field label="Display name">
          <input
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder="you"
            className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent"
          />
        </Field>

        {error && (
          <p className="mt-2 text-xs text-danger" role="alert">
            {error}
          </p>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={close}
            className="rounded px-3 py-1 text-sm text-secondary hover:text-primary"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy}
            className={clsx(
              "rounded px-3 py-1 text-sm",
              "bg-accent text-accent-contrast hover:bg-accent-hover",
            )}
          >
            {busy ? "Connecting…" : "Add & connect"}
          </button>
        </div>
      </form>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="mb-3 block">
      <span className="mb-1 block text-[11px] uppercase tracking-wide text-muted">
        {label}
      </span>
      {children}
    </label>
  );
}

function suggestActorId(): string {
  const suffix = Math.random().toString(16).slice(2, 10);
  return `actor_human_${suffix}`;
}
