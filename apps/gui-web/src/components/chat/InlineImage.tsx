import { useState } from "react";
import { AlertCircle, Loader2 } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Artifact } from "@/ipc/types";
import { errorText } from "@/lib/format-utils";
import { readArtifactBlob } from "@/lib/artifact-blob";
import { useAutoDownloadImage } from "@/hooks/useAutoDownloadImage";
import { ImageLightbox } from "@/components/chat/ImageLightbox";
import { ImageContextMenu } from "@/components/chat/ImageContextMenu";

/**
 * Inline image display component.
 *
 * ARCH D3 design:
 * - Auto-downloads via useAutoDownloadImage (two-level cache)
 * - Gray placeholder 400x300px while loading (AC-P0-6)
 * - Inline max 400x300px (AC-P0-7)
 * - Click opens ImageLightbox (AC-P0-3/P0-4)
 * - ImageContextMenu for actions (AC-P0-5)
 */

export function InlineImage({ artifact }: { artifact: Artifact }) {
  const { objectUrl, loading, error, localPath } = useAutoDownloadImage(artifact);
  const [showLightbox, setShowLightbox] = useState(false);
  const [currentLocalPath, setCurrentLocalPath] = useState(localPath);

  // Sync localPath from hook
  if (localPath !== currentLocalPath && localPath !== null) {
    setCurrentLocalPath(localPath);
  }

  const title = artifact.name || artifact.id;

  async function handleSaveAs() {
    try {
      const blob = await readArtifactBlob(artifact);
      const bytes = new Uint8Array(await blob.arrayBuffer());
      await ipc.saveFileDialog(artifact.name || `${artifact.id}.bin`, bytes);
    } catch (err) {
      void errorText(err);
    }
  }

  return (
    <div className="inline-image-wrapper">
      {loading && (
        <div className="inline-image-placeholder" aria-label={`Loading image ${title}`}>
          <Loader2 className="animate-spin text-[#667085]" size={24} />
        </div>
      )}

      {error && !loading && (
        <div
          className="inline-image-placeholder inline-image-error"
          aria-label={`Failed to load image ${title}`}
        >
          <AlertCircle className="text-red-400" size={24} />
          <span className="mt-1 text-xs text-red-500">Failed to load</span>
        </div>
      )}

      {objectUrl && !loading && !error && (
        <div className="inline-image-container">
          <img
            src={objectUrl}
            alt={title}
            className="inline-image-thumb"
            onClick={() => setShowLightbox(true)}
            draggable={false}
          />
          <ImageContextMenu
            artifact={artifact}
            localPath={currentLocalPath}
            onPathReady={(path) => setCurrentLocalPath(path)}
          />
        </div>
      )}

      {showLightbox && objectUrl && (
        <ImageLightbox
          artifact={artifact}
          objectUrl={objectUrl}
          onClose={() => setShowLightbox(false)}
          onDownload={handleSaveAs}
        />
      )}
    </div>
  );
}
