import { useEffect, useRef, useState, type ReactNode } from "react";
import clsx from "clsx";
import { Plus, Search, Trash2, X } from "lucide-react";

import { useUI, type ModalSpec } from "@/store/ui";
import { PixelAvatar } from "./PixelAvatar";

export function ModalHost() {
  const modal = useUI((s) => s.modal);
  const close = useUI((s) => s.closeModal);

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
      className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/60 p-4"
      onClick={close}
    >
      <div onClick={(e) => e.stopPropagation()}>
        {modal.type === "input" && <InputForm spec={modal} onDone={close} />}
        {modal.type === "confirm" && (
          <ConfirmForm spec={modal} onDone={close} />
        )}
        {modal.type === "picker" && <PickerForm spec={modal} onDone={close} />}
        {modal.type === "channelForm" && (
          <ChannelForm spec={modal} onDone={close} />
        )}
        {modal.type === "taskCreate" && (
          <TaskCreateForm spec={modal} onDone={close} />
        )}
        {modal.type === "quickSwitch" && <QuickSwitchForm onDone={close} />}
      </div>
    </div>
  );
}

function ModalFrame({
  title,
  children,
  onDone,
  max = "max-w-md",
}: {
  title: string;
  children: ReactNode;
  onDone: () => void;
  max?: string;
}) {
  return (
    <section className={clsx("card-brutal w-[calc(100vw-2rem)] p-6", max)}>
      <div className="mb-4 flex items-center justify-between gap-4">
        <h2 className="text-lg font-black uppercase text-black">{title}</h2>
        <button className="btn-brutal-sm bg-white p-1" onClick={onDone}>
          <X size={18} />
        </button>
      </div>
      {children}
    </section>
  );
}

function InputForm({
  spec,
  onDone,
}: {
  spec: Extract<ModalSpec, { type: "input" }>;
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
    <ModalFrame title={spec.title} onDone={onDone}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        className="space-y-4"
      >
        {spec.label && (
          <label className="block text-sm font-black uppercase tracking-wide text-black">
            {spec.label}
          </label>
        )}
        <input
          ref={ref}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={spec.placeholder}
          className="input-brutal w-full"
        />
        <FormActions
          busy={busy}
          danger={spec.danger}
          confirmLabel={spec.confirmLabel ?? "OK"}
          onCancel={onDone}
        />
      </form>
    </ModalFrame>
  );
}

function ConfirmForm({
  spec,
  onDone,
}: {
  spec: Extract<ModalSpec, { type: "confirm" }>;
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
    <ModalFrame title={spec.title} onDone={onDone}>
      <p className="text-sm leading-6 text-black/70">{spec.body}</p>
      <div className="mt-5 flex justify-end gap-3">
        <button className="btn-brutal bg-white px-4 py-2 text-sm" onClick={onDone}>
          Cancel
        </button>
        <button
          onClick={doIt}
          disabled={busy}
          className={clsx(
            "btn-brutal px-4 py-2 text-sm",
            spec.danger ? "bg-danger" : "bg-brutal-pink",
          )}
        >
          {spec.confirmLabel ?? "Confirm"}
        </button>
      </div>
    </ModalFrame>
  );
}

function PickerForm({
  spec,
  onDone,
}: {
  spec: Extract<ModalSpec, { type: "picker" }>;
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
    <ModalFrame title={spec.title} onDone={onDone}>
      <SearchInput
        value={filter}
        onChange={setFilter}
        placeholder="Filter..."
        autoFocus
      />
      <ul className="mt-3 max-h-72 overflow-y-auto border-2 border-black bg-white">
        {items.length === 0 ? (
          <li className="px-3 py-3 text-sm font-mono text-black/40">no matches</li>
        ) : (
          items.map((it) => (
            <li key={it.id} className="border-b-2 border-black last:border-b-0">
              <button
                onClick={async () => {
                  await spec.onPick(it.id);
                  onDone();
                }}
                className="flex w-full items-baseline gap-2 px-3 py-2 text-left text-sm hover:bg-brutal-cream"
              >
                <span className="font-black text-black">{it.label}</span>
                {it.hint && <span className="text-xs text-black/50">{it.hint}</span>}
                <span className="ml-auto truncate font-mono text-[11px] text-black/40">
                  {it.id}
                </span>
              </button>
            </li>
          ))
        )}
      </ul>
    </ModalFrame>
  );
}

function ChannelForm({
  spec,
  onDone,
}: {
  spec: Extract<ModalSpec, { type: "channelForm" }>;
  onDone: () => void;
}) {
  const [title, setTitle] = useState(spec.initialTitle ?? "");
  const [description, setDescription] = useState(spec.initialDescription ?? "");
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    nameRef.current?.focus();
    nameRef.current?.select();
  }, []);

  const actors = spec.actorItems ?? [];
  const filtered = actors.filter(
    (a) =>
      !filter ||
      a.label.toLowerCase().includes(filter.toLowerCase()) ||
      a.id.toLowerCase().includes(filter.toLowerCase()),
  );

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await spec.onSubmit({
        title,
        description,
        actorIds: Object.entries(selected)
          .filter(([, on]) => on)
          .map(([id]) => id),
      });
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <ModalFrame title={spec.title} onDone={onDone}>
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <div>
          <label className="mb-1 block text-sm font-black uppercase tracking-wide">
            {spec.nameLabel ?? "Name"} <span className="text-brutal-pink">*</span>
          </label>
          <input
            ref={nameRef}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            disabled={spec.titleLocked}
            placeholder="e.g. ai-research"
            className="input-brutal w-full disabled:bg-black/5"
          />
          {spec.titleLocked && (
            <div className="mt-1 font-mono text-xs text-black/45">
              The #{title} channel cannot be renamed
            </div>
          )}
        </div>

        <div>
          <label className="mb-1 block text-sm font-black uppercase tracking-wide">
            Description <span className="text-black/40 normal-case">(optional)</span>
          </label>
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="What is this channel about?"
            rows={3}
            className="input-brutal w-full resize-none"
          />
        </div>

        {actors.length > 0 && (
          <div>
            <label className="mb-1 block text-sm font-black uppercase tracking-wide">
              Initial members{" "}
              <span className="text-black/40 normal-case">(optional)</span>
            </label>
            <SearchInput
              value={filter}
              onChange={setFilter}
              placeholder="Search members by name"
            />
            <div className="mt-2 max-h-40 overflow-y-auto border-2 border-black bg-white">
              <div className="border-b-2 border-black px-3 py-2 text-xs font-black uppercase tracking-widest text-black/50">
                Agents
              </div>
              {filtered.map((actor) => (
                <button
                  key={actor.id}
                  type="button"
                  className={clsx(
                    "flex w-full items-center gap-2 px-3 py-2 text-left text-sm font-bold hover:bg-brutal-cream",
                    selected[actor.id] && "bg-brutal-yellow",
                  )}
                  onClick={() =>
                    setSelected((s) => ({ ...s, [actor.id]: !s[actor.id] }))
                  }
                >
                  <PixelAvatar id={actor.id} label={actor.label} size={20} />
                  <span className="min-w-0 flex-1 truncate">{actor.label}</span>
                  <span className="text-[11px] text-black/45">{actor.hint}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        <FormActions
          busy={busy}
          confirmLabel={spec.confirmLabel ?? "Save"}
          onCancel={onDone}
        />
      </form>
    </ModalFrame>
  );
}

function TaskCreateForm({
  spec,
  onDone,
}: {
  spec: Extract<ModalSpec, { type: "taskCreate" }>;
  onDone: () => void;
}) {
  const [titles, setTitles] = useState([""]);
  const [busy, setBusy] = useState(false);
  const count = titles.filter((x) => x.trim()).length;

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await spec.onSubmit?.(titles.filter((x) => x.trim()));
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <ModalFrame title={count > 1 ? "Create tasks" : spec.title ?? "Create task"} onDone={onDone}>
      <form
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        {titles.map((title, index) => (
          <div key={index} className="flex items-center gap-2">
            <input
              autoFocus={index === 0}
              value={title}
              onChange={(e) =>
                setTitles((xs) =>
                  xs.map((x, i) => (i === index ? e.target.value : x)),
                )
              }
              placeholder={`Task ${index + 1}`}
              className="input-brutal min-w-0 flex-1"
            />
            {titles.length > 1 && (
              <button
                type="button"
                className="btn-brutal-sm bg-white p-1"
                onClick={() =>
                  setTitles((xs) => xs.filter((_, i) => i !== index))
                }
              >
                <Trash2 size={15} />
              </button>
            )}
          </div>
        ))}
        <button
          type="button"
          className="btn-brutal-sm gap-1 bg-white px-2 text-xs"
          onClick={() => setTitles((xs) => [...xs, ""])}
        >
          <Plus size={13} /> Add another
        </button>
        <FormActions
          busy={busy}
          confirmLabel={count > 1 ? `Create ${count} Tasks` : "Create Task"}
          onCancel={onDone}
        />
      </form>
    </ModalFrame>
  );
}

function QuickSwitchForm({ onDone }: { onDone: () => void }) {
  const [filter, setFilter] = useState("");
  const [items, setItems] = useState<
    Array<{
      id: string;
      label: string;
      hint: string;
      scope: { kind: "channel" | "thread"; id: string };
    }>
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
          hint: ch.visibility === "private" ? "private" : "channel",
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

  const filtered = items.filter(
    (it) =>
      !filter ||
      it.label.toLowerCase().includes(filter.toLowerCase()) ||
      it.hint.toLowerCase().includes(filter.toLowerCase()),
  );

  const go = async (scope: { kind: "channel" | "thread"; id: string }) => {
    const { openScope } = await import("@/features/chat/scopeActions");
    const { useUI } = await import("@/store/ui");
    await openScope(scope);
    useUI.getState().setView("chat");
    onDone();
  };

  return (
    <ModalFrame title="Quick switch" onDone={onDone}>
      <SearchInput
        autoFocus
        value={filter}
        onChange={setFilter}
        placeholder="Jump to channel or thread..."
        onEnter={() => filtered[0] && void go(filtered[0].scope)}
      />
      <ul className="mt-3 max-h-80 overflow-y-auto border-2 border-black bg-white">
        {filtered.slice(0, 50).map((it) => (
          <li key={it.id} className="border-b-2 border-black last:border-b-0">
            <button
              onClick={() => void go(it.scope)}
              className="flex w-full items-baseline gap-2 px-3 py-2 text-left text-sm hover:bg-brutal-cream"
            >
              <span className="font-black">{it.label}</span>
              <span className="text-xs text-black/45">{it.hint}</span>
            </button>
          </li>
        ))}
      </ul>
    </ModalFrame>
  );
}

function SearchInput({
  value,
  onChange,
  placeholder,
  autoFocus,
  onEnter,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  autoFocus?: boolean;
  onEnter?: () => void;
}) {
  return (
    <div className="input-brutal flex items-center gap-2">
      <Search size={14} className="shrink-0 text-black/45" />
      <input
        autoFocus={autoFocus}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onEnter?.();
        }}
        placeholder={placeholder}
        className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-black/40"
      />
    </div>
  );
}

function FormActions({
  busy,
  danger,
  confirmLabel,
  onCancel,
}: {
  busy: boolean;
  danger?: boolean;
  confirmLabel: string;
  onCancel: () => void;
}) {
  return (
    <div className="flex justify-end gap-3 pt-2">
      <button
        type="button"
        className="btn-brutal bg-white px-4 py-2 text-sm"
        onClick={onCancel}
      >
        Cancel
      </button>
      <button
        type="submit"
        disabled={busy}
        className={clsx(
          "btn-brutal px-4 py-2 text-sm",
          danger ? "bg-danger" : "bg-brutal-pink",
        )}
      >
        {confirmLabel}
      </button>
    </div>
  );
}
