import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Download, X } from "lucide-react";

import { formatBytes } from "@/lib/format-utils";
import type { Artifact } from "@/ipc/types";

/**
 * Full-screen image lightbox with native zoom/pan/double-click.
 *
 * ARCH D3 design: native implementation (~80 lines), zero new dependencies.
 * Supports: dark background, ESC close, click-backdrop close, wheel zoom,
 * drag to pan, double-click to toggle zoom.
 *
 * Cross-platform: uses standard Pointer events (works on Win/Mac/Linux).
 */

const MIN_SCALE = 1;
const MAX_SCALE = 8;
const DOUBLE_CLICK_SCALE = 2.5;

interface Transform {
  scale: number;
  x: number;
  y: number;
}

const INITIAL_TRANSFORM: Transform = { scale: 1, x: 0, y: 0 };

export function ImageLightbox({
  artifact,
  objectUrl,
  onClose,
  onDownload,
}: {
  artifact: Artifact;
  objectUrl: string;
  onClose: () => void;
  onDownload: () => void;
}) {
  const [transform, setTransform] = useState<Transform>(INITIAL_TRANSFORM);
  const [dragging, setDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; y: number; tx: number; ty: number } | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const title = artifact.name || artifact.id;

  const resetTransform = useCallback(() => {
    setTransform(INITIAL_TRANSFORM);
  }, []);

  // ESC to close
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        if (transform.scale !== 1) {
          resetTransform();
        } else {
          onClose();
        }
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, transform.scale, resetTransform]);

  // Wheel zoom (cross-platform: mouse wheel works on all three OSes)
  function onWheel(event: React.WheelEvent<HTMLImageElement>) {
    event.preventDefault();
    const delta = -event.deltaY * 0.0015;
    setTransform((prev) => {
      const nextScale = clamp(prev.scale * (1 + delta), MIN_SCALE, MAX_SCALE);
      return { ...prev, scale: nextScale };
    });
  }

  // Pointer drag to pan
  function onPointerDown(event: React.PointerEvent<HTMLImageElement>) {
    if (event.button !== 0) return;
    setDragging(true);
    dragStartRef.current = {
      x: event.clientX,
      y: event.clientY,
      tx: transform.x,
      ty: transform.y,
    };
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: React.PointerEvent<HTMLImageElement>) {
    if (!dragging || !dragStartRef.current) return;
    const dx = event.clientX - dragStartRef.current.x;
    const dy = event.clientY - dragStartRef.current.y;
    setTransform((prev) => ({
      ...prev,
      x: dragStartRef.current!.tx + dx,
      y: dragStartRef.current!.ty + dy,
    }));
  }

  function onPointerUp(event: React.PointerEvent<HTMLImageElement>) {
    setDragging(false);
    dragStartRef.current = null;
    try {
      (event.target as HTMLElement).releasePointerCapture(event.pointerId);
    } catch {
      // pointer already released
    }
  }

  // Double-click toggles zoom
  function onDoubleClick() {
    setTransform((prev) => {
      if (prev.scale > 1) return INITIAL_TRANSFORM;
      return { scale: DOUBLE_CLICK_SCALE, x: 0, y: 0 };
    });
  }

  // Click on backdrop (not image) closes
  function onBackdropClick(event: React.MouseEvent<HTMLDivElement>) {
    if (event.target === event.currentTarget) {
      onClose();
    }
  }

  return createPortal(
    <div
      ref={containerRef}
      className="image-lightbox-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={`Image preview ${title}`}
      onClick={onBackdropClick}
    >
      {/* Top toolbar */}
      <div className="image-lightbox-toolbar" onClick={(e) => e.stopPropagation()}>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-bold text-white/90" title={title}>
            {title}
          </div>
          <div className="truncate text-xs font-medium text-white/50">
            {formatBytes(artifact.size)} · {Math.round(transform.scale * 100)}%
          </div>
        </div>
        <button
          type="button"
          className="image-lightbox-toolbar-btn"
          title="Save as..."
          onClick={onDownload}
        >
          <Download size={16} />
        </button>
        <button
          type="button"
          className="image-lightbox-toolbar-btn"
          title="Close (ESC)"
          aria-label="Close lightbox"
          onClick={onClose}
        >
          <X size={18} />
        </button>
      </div>

      {/* Image */}
      <img
        src={objectUrl}
        alt={title}
        className="image-lightbox-img"
        style={{
          transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})`,
          cursor: dragging ? "grabbing" : transform.scale > 1 ? "grab" : "zoom-in",
        }}
        draggable={false}
        onWheel={onWheel}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={onDoubleClick}
        onClick={(e) => e.stopPropagation()}
      />
    </div>,
    document.body,
  );
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
