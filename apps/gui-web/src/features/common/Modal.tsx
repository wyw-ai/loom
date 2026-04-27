import { useEffect, useRef, useState } from "react";
import clsx from "clsx";

import { useUI } from "@/store/ui";

export function ModalHost() {
  const modal = useUI((s) => s.modal);
  const close = useUI((s) => s.closeModal);

  // Esc closes globally. Enter is handled inside the specific modal so we
  // don't steal it from input rendering.
  useEffect(() => {
    if (!modal) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [modal, close]);

  if (!modal) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={close}
    >
      <div
        className="w-[420px] max-w-[90vw] rounded-lg border border-border bg-elevated shadow-[0_8px_24px_rgba(0,0,0,.45)]"
        onClick={(e) => e.stopPropagation()}
      >
        {modal.type === "input" && <InputForm spec={modal} onDone={close} />}
        {modal.type === "confirm" && (
          <ConfirmForm spec={modal} onDone={close} />
        )}
        {modal.type === "picker" && <PickerForm spec={modal} onDone={close} />}
        {modal.type === "quickSwitch" && <QuickSwitchForm onDone={close} />}
      </div>
    </div>
  );
}

function InputForm({
  spec,
  onDone,
}: {
  spec: Extract<NonNullable<ReturnType<typeof useUI.getState>["modal"]>, { type: "input" }>;
  onDone: () => void;
}) {
  const [value, setValue] = useState(spec.initial ?? "");
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await spec.onSubmit(value);
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
      className="p-5"
    >
      <h3 className="mb-3 text-base font-semibold text-primary">{spec.title}</h3>
      {spec.label && (
        <label className="mb-1 block text-xs uppercase tracking-wide text-muted">
          {spec.label}
        </label>
      )}
      <input
        ref={ref}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder={spec.placeholder}
        className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent"
      />
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onDone}
          className="rounded px-3 py-1 text-sm text-secondary hover:text-primary"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={busy}
          className={clsx(
            "rounded px-3 py-1 text-sm",
            spec.danger
              ? "bg-danger text-white hover:opacity-90"
              : "bg-accent text-accent-contrast hover:bg-accent-hover",
          )}
        >
          {spec.confirmLabel ?? "OK"}
        </button>
      </div>
    </form>
  );
}

function ConfirmForm({
  spec,
  onDone,
}: {
  spec: Extract<NonNullable<ReturnType<typeof useUI.getState>["modal"]>, { type: "confirm" }>;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const doIt = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await spec.onConfirm();
      onDone();
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="p-5">
      <h3 className="mb-2 text-base font-semibold text-primary">{spec.title}</h3>
      <p className="text-sm text-secondary">{spec.body}</p>
      <div className="mt-4 flex justify-end gap-2">
        <button
          onClick={onDone}
          className="rounded px-3 py-1 text-sm text-secondary hover:text-primary"
        >
          Cancel
        </button>
        <button
          onClick={doIt}
          disabled={busy}
          className={clsx(
            "rounded px-3 py-1 text-sm",
            spec.danger
              ? "bg-danger text-white hover:opacity-90"
              : "bg-accent text-accent-contrast hover:bg-accent-hover",
          )}
        >
          {spec.confirmLabel ?? "Confirm"}
        </button>
      </div>
    </div>
  );
}

function PickerForm({
  spec,
  onDone,
}: {
  spec: Extract<NonNullable<ReturnType<typeof useUI.getState>["modal"]>, { type: "picker" }>;
  onDone: () => void;
}) {
  const [filter, setFilter] = useState("");
  const items = spec.items.filter(
    (it) =>
      !filter ||
      it.label.toLowerCase().includes(filter.toLowerCase()) ||
      it.id.toLowerCase().includes(filter.toLowerCase()),
  );
  return (
    <div className="p-5">
      <h3 className="mb-3 text-base font-semibold text-primary">{spec.title}</h3>
      <input
        autoFocus
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder="Filter…"
        className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent"
      />
      <ul className="mt-3 max-h-72 overflow-y-auto">
        {items.length === 0 ? (
          <li className="px-2 py-2 text-xs text-muted">no matches</li>
        ) : (
          items.map((it) => (
            <li key={it.id}>
              <button
                onClick={async () => {
                  await spec.onPick(it.id);
                  onDone();
                }}
                className="flex w-full items-baseline gap-2 rounded px-2 py-1 text-left text-sm hover:bg-hover"
              >
                <span className="text-primary">{it.label}</span>
                {it.hint && <span className="text-xs text-muted">{it.hint}</span>}
                <span className="ml-auto text-[11px] text-muted">{it.id}</span>
              </button>
            </li>
          ))
        )}
      </ul>
    </div>
  );
}

function QuickSwitchForm({ onDone }: { onDone: () => void }) {
  // Lazy imports to keep the modal module free of store-cycle risk.
  const [filter, setFilter] = useState("");
  const [items, setItems] = useState<
    Array<{ id: string; label: string; hint: string; scope: { kind: "channel" | "thread"; id: string } }>
  >([]);

  useEffect(() => {
    (async () => {
      const { useChannels } = await import("@/store/channels");
      const s = useChannels.getState();
      const rows: typeof items = [];
      for (const ch of s.channels) {
        rows.push({
          id: `channel:${ch.id}`,
          label: `# ${ch.title}`,
          hint: ch.visibility === "private" ? "private" : "public",
          scope: { kind: "channel", id: ch.id },
        });
      }
      for (const [chId, threads] of Object.entries(s.threadsByChannel)) {
        const ch = s.channels.find((c) => c.id === chId);
        for (const t of threads) {
          rows.push({
            id: `thread:${t.id}`,
            label: t.title,
            hint: `# ${ch?.title ?? chId}`,
            scope: { kind: "thread", id: t.id },
          });
        }
      }
      setItems(rows);
    })();
  }, []);

  const go = async (scope: { kind: "channel" | "thread"; id: string }) => {
    const { useChannels } = await import("@/store/channels");
    const { useMessages } = await import("@/store/messages");
    const { useUI } = await import("@/store/ui");
    const ipc = await import("@/ipc/bridge");
    useChannels.getState().setCurrentScope(scope);
    useMessages.getState().ensureScope(scope);
    useUI.getState().setView("chat");
    try {
      await ipc.scopeSubscribe(scope);
      const r = await ipc.scopeRead(scope, 100);
      useMessages.getState().ingestBackfill(scope, r.events);
    } catch {
      /* surfaced via toast elsewhere */
    }
    onDone();
  };

  const filtered = items.filter(
    (it) =>
      !filter ||
      it.label.toLowerCase().includes(filter.toLowerCase()) ||
      it.hint.toLowerCase().includes(filter.toLowerCase()),
  );

  return (
    <div className="p-5">
      <h3 className="mb-3 text-base font-semibold text-primary">Quick switch</h3>
      <input
        autoFocus
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder="Jump to channel or thread…"
        onKeyDown={(e) => {
          if (e.key === "Enter" && filtered[0]) void go(filtered[0].scope);
        }}
        className="w-full rounded border border-border bg-main px-3 py-2 text-sm text-primary outline-none focus:border-accent"
      />
      <ul className="mt-3 max-h-80 overflow-y-auto">
        {filtered.slice(0, 50).map((it) => (
          <li key={it.id}>
            <button
              onClick={() => void go(it.scope)}
              className="flex w-full items-baseline gap-2 rounded px-2 py-1 text-left text-sm hover:bg-hover"
            >
              <span className="text-primary">{it.label}</span>
              <span className="text-xs text-muted">{it.hint}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
