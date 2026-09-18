import { useCallback, useRef, type PointerEvent } from "react";
import type { CSSProperties } from "react";
import type { PanelResizeDrag, PanelResizeKind } from "@/lib/types";
import {
  detailPanelBreakpoint,
  mainMinWidth,
  threadPanelDragMainMinWidth,
} from "@/lib/constants";
import {
  fitPanelSizes,
  initialViewportWidth,
  shouldSnapThreadPanel,
} from "@/lib/format-utils";

interface UsePanelResizeParams {
  panelSizes: { sidebar: number; detail: number };
  setPanelSizes: (next: { sidebar: number; detail: number }) => void;
  viewportWidth: number;
  detailVisibleInGrid: boolean;
  showChatDetail: boolean;
  allowDetailExpansion: boolean;
  onDetailSnap: () => void;
  setResizingPanel: (kind: PanelResizeKind | null) => void;
}

export function usePanelResize({
  panelSizes,
  setPanelSizes,
  viewportWidth,
  detailVisibleInGrid,
  showChatDetail,
  allowDetailExpansion,
  onDetailSnap,
  setResizingPanel,
}: UsePanelResizeParams) {
  const panelResizeDragRef = useRef<PanelResizeDrag | null>(null);
  const panelResizeCleanupRef = useRef<(() => void) | null>(null);

  const fittedPanelSizes = fitPanelSizes(
    panelSizes,
    viewportWidth,
    detailVisibleInGrid,
    { allowWideDetail: allowDetailExpansion },
  );

  const shellStyle = {
    "--sidebar-width": `${fittedPanelSizes.sidebar}px`,
    "--detail-width": `${fittedPanelSizes.detail}px`,
    "--main-min-width": `${
      allowDetailExpansion ? threadPanelDragMainMinWidth : mainMinWidth
    }px`,
  } as CSSProperties;

  const cleanupPanelResize = useCallback(() => {
    panelResizeCleanupRef.current?.();
    panelResizeCleanupRef.current = null;
    panelResizeDragRef.current = null;
    setResizingPanel(null);
    document.body.classList.remove("is-resizing-panels");
  }, [setResizingPanel]);

  const applyPanelResize = useCallback(
    (drag: PanelResizeDrag, clientX: number, width = initialViewportWidth()) => {
      const detailVisible = width >= detailPanelBreakpoint && showChatDetail;
      if (
        drag.kind === "detail" &&
        allowDetailExpansion &&
        detailVisible &&
        shouldSnapThreadPanel(clientX, drag.sidebar)
      ) {
        onDetailSnap();
        return true;
      }
      const delta = clientX - drag.startX;
      const next =
        drag.kind === "sidebar"
          ? { sidebar: drag.sidebar + delta, detail: drag.detail }
          : { sidebar: drag.sidebar, detail: drag.detail - delta };
      setPanelSizes(
        fitPanelSizes(next, width, detailVisible, {
          allowWideDetail: allowDetailExpansion,
        }),
      );
      return false;
    },
    [allowDetailExpansion, onDetailSnap, setPanelSizes, showChatDetail],
  );

  const startPanelResize = useCallback(
    (event: PointerEvent<HTMLButtonElement>, kind: PanelResizeKind) => {
      if (event.button !== 0) return;
      event.preventDefault();
      cleanupPanelResize();
      const drag: PanelResizeDrag = {
        kind,
        startX: event.clientX,
        sidebar: fittedPanelSizes.sidebar,
        detail: fittedPanelSizes.detail,
      };
      panelResizeDragRef.current = drag;
      setResizingPanel(kind);
      document.body.classList.add("is-resizing-panels");
      const handlePointerMove = (moveEvent: globalThis.PointerEvent) => {
        const current = panelResizeDragRef.current;
        if (!current) return;
        moveEvent.preventDefault();
        if (applyPanelResize(current, moveEvent.clientX)) cleanupPanelResize();
      };
      const handlePointerUp = (upEvent: globalThis.PointerEvent) => {
        const current = panelResizeDragRef.current;
        if (current) applyPanelResize(current, upEvent.clientX);
        cleanupPanelResize();
      };
      window.addEventListener("pointermove", handlePointerMove, { passive: false });
      window.addEventListener("pointerup", handlePointerUp);
      window.addEventListener("pointercancel", cleanupPanelResize);
      panelResizeCleanupRef.current = () => {
        window.removeEventListener("pointermove", handlePointerMove);
        window.removeEventListener("pointerup", handlePointerUp);
        window.removeEventListener("pointercancel", cleanupPanelResize);
      };
    },
    [applyPanelResize, cleanupPanelResize, fittedPanelSizes, setResizingPanel],
  );

  const resizePanelByKeyboard = useCallback(
    (kind: PanelResizeKind, delta: number) => {
      const next =
        kind === "sidebar"
          ? { ...fittedPanelSizes, sidebar: fittedPanelSizes.sidebar + delta }
          : { ...fittedPanelSizes, detail: fittedPanelSizes.detail - delta };
      setPanelSizes(
        fitPanelSizes(next, viewportWidth, detailVisibleInGrid, {
          allowWideDetail: allowDetailExpansion,
        }),
      );
    },
    [
      allowDetailExpansion,
      detailVisibleInGrid,
      fittedPanelSizes,
      setPanelSizes,
      viewportWidth,
    ],
  );

  return {
    fittedPanelSizes,
    shellStyle,
    cleanupPanelResize,
    startPanelResize,
    resizePanelByKeyboard,
  };
}
