import { describe, expect, it } from "vitest";

import type { Artifact } from "@/ipc/types";
import { isImageArtifact } from "@/components/chat/AttachmentStack";

function makeArtifact(mediaType: string): Artifact {
  return {
    id: "art-1",
    uri: "loom://artifact/art-1",
    kind: "file",
    name: "test.bin",
    mediaType,
    size: 0,
    checksum: "",
    createdBy: "user",
    createdAt: "2026-07-24T00:00:00Z",
  };
}

describe("isImageArtifact", () => {
  it("returns true for image/png", () => {
    expect(isImageArtifact(makeArtifact("image/png"))).toBe(true);
  });

  it("returns true for image/jpeg", () => {
    expect(isImageArtifact(makeArtifact("image/jpeg"))).toBe(true);
  });

  it("returns true for image/gif", () => {
    expect(isImageArtifact(makeArtifact("image/gif"))).toBe(true);
  });

  it("returns true for image/svg+xml", () => {
    expect(isImageArtifact(makeArtifact("image/svg+xml"))).toBe(true);
  });

  it("returns true for image/webp", () => {
    expect(isImageArtifact(makeArtifact("image/webp"))).toBe(true);
  });

  it("returns false for application/pdf", () => {
    expect(isImageArtifact(makeArtifact("application/pdf"))).toBe(false);
  });

  it("returns false for text/plain", () => {
    expect(isImageArtifact(makeArtifact("text/plain"))).toBe(false);
  });

  it("returns false for video/mp4", () => {
    expect(isImageArtifact(makeArtifact("video/mp4"))).toBe(false);
  });

  it("returns false for audio/mpeg", () => {
    expect(isImageArtifact(makeArtifact("audio/mpeg"))).toBe(false);
  });

  it("returns false for empty string", () => {
    expect(isImageArtifact(makeArtifact(""))).toBe(false);
  });

  // SF-8: null-safety guard
  it("returns false for null mediaType (SF-8 null-safety)", () => {
    const artifact = makeArtifact("image/png");
    // Simulate null mediaType at runtime (type says string, but IPC can return null)
    (artifact as unknown as { mediaType: string | null }).mediaType = null;
    expect(isImageArtifact(artifact)).toBe(false);
  });

  it("returns false for undefined mediaType (SF-8 null-safety)", () => {
    const artifact = makeArtifact("image/png");
    // Simulate undefined mediaType at runtime
    (artifact as unknown as { mediaType: string | undefined }).mediaType = undefined;
    expect(isImageArtifact(artifact)).toBe(false);
  });

  it("is case-insensitive (IMAGE/PNG)", () => {
    expect(isImageArtifact(makeArtifact("IMAGE/PNG"))).toBe(true);
  });

  it("is case-insensitive (Image/Jpeg)", () => {
    expect(isImageArtifact(makeArtifact("Image/Jpeg"))).toBe(true);
  });

  it("returns false for non-image prefix (imagination)", () => {
    expect(isImageArtifact(makeArtifact("imagination/data"))).toBe(false);
  });
});
