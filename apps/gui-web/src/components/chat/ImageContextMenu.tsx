import { useCallback, useEffect, useRef, useState } from "react";
import { ExternalLink, FolderOpen, MoreVertical, Save } from "lucide-react";

import type { Artifact } from "@/ipc/types";
import { errorText } from "@/lib/format-utils";
import { readArtifactBlob } from "@/lib/artifact-blob";
import * as ipc from "@/ipc/bridge";
import { useDownloadToLocal } from "@/hooks/useAutoDownloadImage";

/**
 * Floating action button + right-click context menu for image attachments.
 *
 * ARCH D3 design: onContextMenu is cross-platform (Win/Mac/Linux).
 * Menu items "Open default" and "Reveal in folder" require a local file
 * path — if the image hasn't been downloaded to temp yet, we trigger
 * downloadToTemp first, then perform the action.
 */

interface MenuPosition {
  x: number;
  y: number;
}

export function ImageContextMenu({
  artifact,
  localPath,
  onPathReady,
}: {
  artifact: Artifact;
  localPath: string | null;
  /** Called when a download-to-local completes, so parent can update state. */
  onPathReady: (path: string) => void;
}) {
  const [menuPos, setMenuPos] = useState<MenuPosition | null>(null);
  const [busy, setBusy] = useState<"open" | "save" | "reveal" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const downloadToLocal = useDownloadToLocal();

  const closeMenu = useCallback(() => setMenuPos(null), []);

  // Close menu on outside click / escape
  useEffect(() => {
    if (!menuPos) return;
    function onDocClick(event: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        closeMenu();
      }
    }
    function onEsc(event: KeyboardEvent) {
      if (event.key === "Escape") closeMenu();
    }
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onEsc);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onEsc);
    };
  }, [menuPos, closeMenu]);

  // Cross-platform context menu trigger
  function onContextMenu(event: React.MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    const x = Math.min(event.clientX, window.innerWidth - 200);
    const y = Math.min(event.clientY, window.innerHeight - 180);
    setMenuPos({ x, y });
  }

  function toggleMenuFromButton(event: React.MouseEvent) {
    event.stopPropagation();
    if (menuPos) {
      closeMenu();
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    setMenuPos({
      x: Math.min(rect.left, window.innerWidth - 200),
      y: Math.min(rect.bottom + 4, window.innerHeight - 180),
    });
  }

  async function ensureLocalPath(): Promise<string | null> {
    if (localPath) {
      const exists = await ipc.pathExists(localPath);
      if (exists) return localPath;
    }
    // Download to temp
    const path = await downloadToLocal(artifact);
    onPathReady(path);
    return path;
  }

  async function handleOpen() {
    setBusy("open");
    setError(null);
    try {
      const path = await ensureLocalPath();
      if (!path) return;
      await ipc.openFileDefault(path);
      closeMenu();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function handleReveal() {
    setBusy("reveal");
    setError(null);
    try {
      const path = await ensureLocalPath();
      if (!path) return;
      await ipc.revealInFolder(path);
      closeMenu();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function handleSaveAs() {
    setBusy("save");
    setError(null);
    try {
      const blob = await readArtifactBlob(artifact);
      const bytes = new Uint8Array(await blob.arrayBuffer());
      await ipc.saveFileDialog(artifact.name || `${artifact.id}.bin`, bytes);
      closeMenu();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      {/* Floating button (top-right) */}
      <button
        type="button"
        className="image-context-menu-btn"
        title="Image actions"
        aria-label={`Actions for ${artifact.name || artifact.id}`}
        onClick={toggleMenuFromButton}
        onContextMenu={onContextMenu}
      >
        <MoreVertical size={16} />
      </button>

      {/* Context menu */}
      {menuPos && (
        <div
          ref={menuRef}
          className="image-context-menu"
          style={{ left: menuPos.x, top: menuPos.y }}
          role="menu"
        >
          <button
            type="button"
            className="image-context-menu-item"
            role="menuitem"
            disabled={busy !== null}
            onClick={handleOpen}
          >
            <ExternalLink size={14} />
            <span>Open with default app</span>
          </button>
          <button
            type="button"
            className="image-context-menu-item"
            role="menuitem"
            disabled={busy !== null}
            onClick={handleReveal}
          >
            <FolderOpen size={14} />
            <span>Reveal in folder</span>
          </button>
          <button
            type="button"
            className="image-context-menu-item"
            role="menuitem"
            disabled={busy !== null}
            onClick={handleSaveAs}
          >
            <Save size={14} />
            <span>Save as…</span>
          </button>
          {error && (
            <div className="image-context-menu-error">{error}</div>
          )}
        </div>
      )}
    </>
  );
}
