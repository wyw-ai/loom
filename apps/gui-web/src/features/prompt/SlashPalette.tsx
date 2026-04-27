import { forwardRef, useImperativeHandle, useMemo } from "react";

// Matches crates/cli/src/cmd/chat/app.rs::slash_command_items.
const COMMANDS: Array<{ cmd: string; desc: string; hint?: string }> = [
  { cmd: "/handoff", desc: "Hand off to an agent or human", hint: "@target [msg]" },
  { cmd: "/reply", desc: "Reply to a previous event" },
  { cmd: "/action", desc: "Respond to a pending action.request" },
  { cmd: "/agents", desc: "List active agents in the thread" },
  { cmd: "/cancel", desc: "Cancel an in-flight agent turn", hint: "[@agent]" },
  { cmd: "/invite", desc: "Invite an actor into the current channel" },
  { cmd: "/members", desc: "List members of the current channel" },
  { cmd: "/announce", desc: "Pin an announcement", hint: "<text> | clear" },
  { cmd: "/quit", desc: "Leave the chat" },
];

export interface SlashPaletteHandle {
  // Picks the first matching entry if any. Returns true if something was
  // picked so the caller can swallow the Enter key.
  pickFirst: () => boolean;
}

interface Props {
  filter: string;
  onPick: (cmd: string) => void;
}

export const SlashPalette = forwardRef<SlashPaletteHandle, Props>(
  function SlashPalette({ filter, onPick }, ref) {
    const f = filter.toLowerCase();
    const items = useMemo(
      () => COMMANDS.filter((c) => c.cmd.slice(1).startsWith(f)),
      [f],
    );

    useImperativeHandle(
      ref,
      () => ({
        pickFirst: () => {
          if (items.length === 0) return false;
          onPick(items[0].cmd);
          return true;
        },
      }),
      [items, onPick],
    );

    if (items.length === 0) return null;
    return (
      <div className="absolute bottom-full left-0 right-0 mb-2 max-h-72 overflow-y-auto rounded-md border border-border bg-elevated px-1 py-1 shadow-lg">
        <div className="px-2 pb-1 pt-1 text-[11px] font-semibold uppercase tracking-wide text-muted">
          Slash commands · Enter to pick first
        </div>
        {items.map((c, i) => (
          <button
            key={c.cmd}
            className={
              "flex w-full items-baseline gap-2 rounded px-2 py-1 text-left text-sm hover:bg-hover " +
              (i === 0 ? "bg-hover/60" : "")
            }
            onClick={() => onPick(c.cmd)}
          >
            <span className="font-mono text-accent">{c.cmd}</span>
            <span className="text-xs text-secondary">{c.desc}</span>
            {c.hint && <span className="ml-auto text-[11px] text-muted">{c.hint}</span>}
          </button>
        ))}
      </div>
    );
  },
);
