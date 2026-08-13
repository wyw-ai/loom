// @vitest-environment jsdom
import React, { useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { usePanelResize } from "./usePanelResize";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const roots: Root[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) React.act(() => root.unmount());
  document.body.innerHTML = "";
  document.body.classList.remove("is-resizing-panels");
});

function ResizeHarness({ onSnap }: { onSnap: () => void }) {
  const [panelSizes, setPanelSizes] = useState({ sidebar: 286, detail: 340 });
  const [resizingPanel, setResizingPanel] = useState<"sidebar" | "detail" | null>(null);
  const { shellStyle, startPanelResize } = usePanelResize({
    panelSizes,
    setPanelSizes,
    viewportWidth: 1_440,
    detailVisibleInGrid: true,
    showChatDetail: true,
    allowDetailExpansion: true,
    onDetailSnap: onSnap,
    setResizingPanel,
  });

  return React.createElement("button", {
    type: "button",
    "data-detail-width": panelSizes.detail,
    "data-resizing": resizingPanel ?? "none",
    style: shellStyle,
    onPointerDown: (event: React.PointerEvent<HTMLButtonElement>) =>
      startPanelResize(event, "detail"),
  });
}

describe("usePanelResize thread snapping", () => {
  it("allows a wide thread and snaps after the divider crosses the threshold", () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1_440,
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    roots.push(root);
    const onSnap = vi.fn();

    React.act(() => root.render(React.createElement(ResizeHarness, { onSnap })));
    const handle = container.querySelector<HTMLButtonElement>("button");

    expect(handle?.style.getPropertyValue("--main-min-width")).toBe("160px");

    React.act(() => {
      handle?.dispatchEvent(new MouseEvent("pointerdown", {
        bubbles: true,
        button: 0,
        clientX: 1_100,
      }));
    });
    React.act(() => {
      window.dispatchEvent(new MouseEvent("pointermove", {
        bubbles: true,
        clientX: 600,
      }));
    });

    expect(handle?.dataset.detailWidth).toBe("840");
    expect(onSnap).not.toHaveBeenCalled();

    React.act(() => {
      window.dispatchEvent(new MouseEvent("pointermove", {
        bubbles: true,
        clientX: 560,
      }));
    });

    expect(onSnap).toHaveBeenCalledOnce();
    expect(document.body.classList.contains("is-resizing-panels")).toBe(false);
  });
});
