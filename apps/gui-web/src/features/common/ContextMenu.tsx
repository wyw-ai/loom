import { useEffect, useRef } from "react";
import clsx from "clsx";

import { useUI } from "@/store/ui";

export function ContextMenuHost() {
  const menu = useUI((s) => s.contextMenu);
  const close = useUI((s) => s.closeContextMenu);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu, close]);

  if (!menu) return null;

  // Clamp near viewport edges so the menu never draws off-screen.
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const width = 200;
  const height = menu.items.length * 30 + 8;
  const x = Math.min(menu.x, vw - width - 8);
  const y = Math.min(menu.y, vh - height - 8);

  return (
    <div
      ref={ref}
      role="menu"
      style={{ left: x, top: y, width }}
      className="fixed z-[60] border-2 border-black bg-white py-1 shadow-brutal"
    >
      {menu.items.map((it, i) =>
        it.kind === "divider" ? (
          <div key={i} className="my-1 h-0.5 bg-black" />
        ) : (
          <button
            key={i}
            disabled={it.disabled}
            onClick={() => {
              it.onClick();
              close();
            }}
            className={clsx(
              "flex w-full items-center px-3 py-1.5 text-sm font-bold",
              it.danger
                ? "text-black hover:bg-danger"
                : "text-black hover:bg-brutal-yellow",
              it.disabled && "opacity-50",
            )}
          >
            {it.label}
          </button>
        ),
      )}
    </div>
  );
}
