import { afterEach, describe, expect, it, vi } from "vitest";

import * as ipc from "@/ipc/bridge";
import type { Artifact } from "@/ipc/types";
import { readArtifactBlob } from "@/lib/artifact-blob";

vi.mock("@/ipc/bridge");

const baseArtifact: Artifact = {
  id: "art-1",
  uri: "loom://artifact/art-1",
  kind: "file",
  name: "photo.png",
  mediaType: "image/png",
  size: 100,
  checksum: "abc",
  createdBy: "user-1",
  createdAt: "2026-07-24T00:00:00Z",
};

function mockArtifactRead(results: Array<{
  bytes?: number[];
  content?: string;
  truncated: boolean;
  nextOffset?: number;
  mediaType?: string;
}>) {
  let callIndex = 0;
  vi.mocked(ipc.artifactRead).mockImplementation(async () => {
    const result = results[callIndex] ?? results[results.length - 1];
    callIndex++;
    return {
      artifactId: "art-1",
      mediaType: result.mediaType ?? "image/png",
      offset: 0,
      truncated: result.truncated,
      nextOffset: result.nextOffset,
      content: result.content ?? "",
      bytes: result.bytes,
    };
  });
}

describe("readArtifactBlob", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("reads a single-chunk artifact (not truncated)", async () => {
    const data = [1, 2, 3, 4, 5];
    mockArtifactRead([{ bytes: data, truncated: false }]);

    const blob = await readArtifactBlob(baseArtifact);
    const buf = new Uint8Array(await blob.arrayBuffer());

    expect(Array.from(buf)).toEqual(data);
    expect(blob.type).toBe("image/png");
    expect(ipc.artifactRead).toHaveBeenCalledTimes(1);
  });

  it("paginates through multiple chunks until truncated=false", async () => {
    mockArtifactRead([
      { bytes: [1, 2], truncated: true, nextOffset: 2 },
      { bytes: [3, 4], truncated: true, nextOffset: 4 },
      { bytes: [5], truncated: false },
    ]);

    const blob = await readArtifactBlob(baseArtifact);
    const buf = new Uint8Array(await blob.arrayBuffer());

    expect(Array.from(buf)).toEqual([1, 2, 3, 4, 5]);
    expect(ipc.artifactRead).toHaveBeenCalledTimes(3);
  });

  it("falls back to content string when bytes are absent", async () => {
    mockArtifactRead([{ content: "hello", truncated: false }]);

    const blob = await readArtifactBlob(baseArtifact);
    const text = await blob.text();

    expect(text).toBe("hello");
  });

  it("prefers bytes over content when both are present", async () => {
    mockArtifactRead([{ bytes: [10, 20], content: "hello", truncated: false }]);

    const blob = await readArtifactBlob(baseArtifact);
    const buf = new Uint8Array(await blob.arrayBuffer());

    expect(Array.from(buf)).toEqual([10, 20]);
  });

  it("throws on zero-progress (nextOffset <= offset) — BLK-2 guard", async () => {
    mockArtifactRead([
      { bytes: [1, 2], truncated: true, nextOffset: 0 },
    ]);

    await expect(readArtifactBlob(baseArtifact)).rejects.toThrow(
      "Artifact read did not advance.",
    );
  });

  it("throws when nextOffset equals current offset", async () => {
    // Simulate: first chunk at offset 0, nextOffset also 0 (stuck)
    let callIndex = 0;
    vi.mocked(ipc.artifactRead).mockImplementation(async () => {
      callIndex++;
      return {
        artifactId: "art-1",
        mediaType: "image/png",
        offset: 0,
        truncated: true,
        nextOffset: 0,
        content: "",
        bytes: [1],
      };
    });

    await expect(readArtifactBlob(baseArtifact)).rejects.toThrow(
      "Artifact read did not advance.",
    );
    expect(callIndex).toBe(1);
  });

  it("throws when exceeding 512 chunks (too large)", async () => {
    // Every chunk is truncated with always-advancing offset, never terminates
    let callOffset = 0;
    vi.mocked(ipc.artifactRead).mockImplementation(async () => {
      const result = {
        artifactId: "art-1",
        mediaType: "image/png",
        offset: callOffset,
        truncated: true,
        nextOffset: callOffset + 1, // always advances by 1
        content: "",
        bytes: [1],
      };
      callOffset += 1;
      return result;
    });

    await expect(readArtifactBlob(baseArtifact)).rejects.toThrow(
      "Artifact is too large to download in one operation.",
    );
  });

  it("uses mediaType from response if artifact has none", async () => {
    const noTypeArtifact: Artifact = { ...baseArtifact, mediaType: "" };
    mockArtifactRead([{ bytes: [1], truncated: false, mediaType: "image/jpeg" }]);

    const blob = await readArtifactBlob(noTypeArtifact);

    expect(blob.type).toBe("image/jpeg");
  });

  it("defaults to octet-stream when no mediaType anywhere", async () => {
    const noTypeArtifact: Artifact = { ...baseArtifact, mediaType: "" };
    mockArtifactRead([{ bytes: [1], truncated: false, mediaType: "" }]);

    const blob = await readArtifactBlob(noTypeArtifact);

    expect(blob.type).toBe("application/octet-stream");
  });

  it("handles empty artifact (zero bytes, not truncated)", async () => {
    mockArtifactRead([{ bytes: [], content: "", truncated: false }]);

    const blob = await readArtifactBlob(baseArtifact);
    const buf = new Uint8Array(await blob.arrayBuffer());

    expect(buf.byteLength).toBe(0);
  });
});
