/**
 * Shared artifact blob reading utilities.
 *
 * Extracted from AttachmentStack to avoid circular imports between
 * AttachmentStack → InlineImage → useAutoDownloadImage → AttachmentStack.
 */

import * as ipc from "@/ipc/bridge";
import type { Artifact, ArtifactReadResult } from "@/ipc/types";

const artifactReadChunkBytes = 1024 * 1024;

function artifactReadBytes(read: ArtifactReadResult) {
  if (read.bytes && read.bytes.length > 0) return Uint8Array.from(read.bytes);
  if (read.content) return new TextEncoder().encode(read.content);
  return new Uint8Array();
}

/**
 * Read an artifact's full binary content as a Blob, paginating through
 * the IPC artifactRead endpoint in 1MB chunks.
 */
export async function readArtifactBlob(artifact: Artifact): Promise<Blob> {
  const chunks: Uint8Array[] = [];
  let offset = 0;
  let mediaType = artifact.mediaType;
  for (let chunkIndex = 0; chunkIndex < 512; chunkIndex += 1) {
    const read = await ipc.artifactRead({
      artifactId: artifact.id,
      offset,
      maxBytes: artifactReadChunkBytes,
    });
    mediaType = read.mediaType || mediaType;
    const bytes = artifactReadBytes(read);
    chunks.push(bytes);
    if (!read.truncated) {
      const parts = chunks.map((chunk) => {
        const copy = new Uint8Array(chunk.byteLength);
        copy.set(chunk);
        return copy.buffer;
      });
      return new Blob(parts, { type: mediaType || "application/octet-stream" });
    }
    const nextOffset = read.nextOffset ?? offset + bytes.byteLength;
    if (nextOffset <= offset) {
      throw new Error("Artifact read did not advance.");
    }
    offset = nextOffset;
  }
  throw new Error("Artifact is too large to download in one operation.");
}
