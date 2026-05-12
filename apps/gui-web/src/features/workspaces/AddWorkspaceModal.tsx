import { useEffect, useState } from "react";
import clsx from "clsx";

import * as ipc from "@/ipc/bridge";
import { useUI } from "@/store/ui";
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
  const account = useWorkspaces((s) => s.account);
  const close = () => useWorkspaces.getState().setAddOpen(false);

  const [name, setName] = useState("");
  const [serverUrl, setServerUrl] = useState("ws://127.0.0.1:7878/rpc");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setName("");
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
    if (!account) {
      setError("account login is required before adding a workspace");
      return;
    }
    if (!name.trim() || !serverUrl.trim()) {
      setError("name and server URL are required");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const cfg = await ipc.workspaceAdd({
        name: name.trim(),
        serverUrl: serverUrl.trim(),
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
        </p>

        <Field label="Account">
          {account ? (
            <div className="input-brutal min-h-[40px] truncate text-sm">
              {account.nickname || account.realName || account.staffId}
              <span className="ml-2 font-mono text-xs text-black/45">
                {account.actorId}
              </span>
            </div>
          ) : (
            <button
              type="button"
              className="btn-brutal w-full justify-start bg-brutal-cyan px-3 py-2 text-sm"
              onClick={() => {
                close();
                useUI.getState().setView("settings");
              }}
            >
              Sign in first
            </button>
          )}
        </Field>

        <Field label="Name">
          <input
            autoFocus
            aria-label="Name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Local dev"
            className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent"
          />
        </Field>

        <Field label="Server URL">
          <input
            aria-label="Server URL"
            value={serverUrl}
            onChange={(e) => setServerUrl(e.target.value)}
            placeholder="ws://127.0.0.1:7878/rpc"
            className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent font-mono"
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
            disabled={busy || !account}
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
    <div className="mb-3 block">
      <span className="mb-1 block text-[11px] uppercase tracking-wide text-muted">
        {label}
      </span>
      {children}
    </div>
  );
}
