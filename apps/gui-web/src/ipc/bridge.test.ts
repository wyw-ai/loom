import { describe, expect, it, vi } from "vitest";
import { mapBatched } from "@/ipc/bridge";

describe("mapBatched", () => {
  it("processes all items and preserves order", async () => {
    const items = [1, 2, 3, 4, 5];
    const task = vi.fn(async (n: number) => n * 10);
    const results = await mapBatched(items, 2, task);
    expect(results).toEqual([10, 20, 30, 40, 50]);
    expect(task).toHaveBeenCalledTimes(5);
  });

  it("handles empty input", async () => {
    const results = await mapBatched([], 8, async (n: number) => n);
    expect(results).toEqual([]);
  });

  it("handles single item", async () => {
    const results = await mapBatched([42], 8, async (n: number) => n);
    expect(results).toEqual([42]);
  });

  it("splits >8 items into multiple batches", async () => {
    // Track concurrency: record max simultaneous in-flight tasks.
    let inFlight = 0;
    let maxInFlight = 0;
    const items = Array.from({ length: 20 }, (_, i) => i);
    const task = vi.fn(async (n: number) => {
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await Promise.resolve();
      inFlight--;
      return n;
    });

    const results = await mapBatched(items, 8, task);

    expect(results).toEqual(items);
    expect(task).toHaveBeenCalledTimes(20);
    // Concurrency must never exceed the batch size of 8.
    expect(maxInFlight).toBeLessThanOrEqual(8);
  });

  it("merges results completely across batches", async () => {
    const items = Array.from({ length: 25 }, (_, i) => `item-${i}`);
    const results = await mapBatched(items, 8, async (s: string) => s.toUpperCase());
    expect(results).toEqual(items.map((s) => s.toUpperCase()));
    expect(results.length).toBe(25);
  });

  it("produces undefined for rejected tasks without failing the batch", async () => {
    const items = [1, 2, 3, 4];
    const task = vi.fn(async (n: number) => {
      if (n === 2) throw new Error("fail");
      return n;
    });
    const results = await mapBatched(items, 2, task);
    expect(results).toEqual([1, undefined, 3, 4]);
  });

  it("respects custom batch size of 1 (sequential)", async () => {
    let inFlight = 0;
    let maxInFlight = 0;
    const items = [1, 2, 3];
    const task = vi.fn(async (n: number) => {
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await Promise.resolve();
      inFlight--;
      return n;
    });
    const results = await mapBatched(items, 1, task);
    expect(results).toEqual([1, 2, 3]);
    expect(maxInFlight).toBe(1);
  });
});
