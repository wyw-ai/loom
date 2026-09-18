import { describe, expect, it } from "vitest";
import {
  fitPanelSizes,
  reconnectDelayMs,
  shouldSnapThreadPanel,
} from "./format-utils";

describe("reconnectDelayMs", () => {
  it("retries once immediately, then backs off with a cap", () => {
    expect(reconnectDelayMs(1)).toBe(0);
    expect(reconnectDelayMs(2)).toBe(500);
    expect(reconnectDelayMs(3)).toBe(1_000);
    expect(reconnectDelayMs(20)).toBe(15_000);
  });
});

describe("thread panel sizing", () => {
  it("keeps ordinary detail panels capped but lets threads expand toward the left", () => {
    const sizes = { sidebar: 286, detail: 2_000 };

    expect(fitPanelSizes(sizes, 1_440, true).detail).toBe(560);
    expect(
      fitPanelSizes(sizes, 1_440, true, { allowWideDetail: true }).detail,
    ).toBe(906);
  });

  it("maximizes once the remaining main area reaches the snap threshold", () => {
    expect(shouldSnapThreadPanel(567, 286)).toBe(false);
    expect(shouldSnapThreadPanel(566, 286)).toBe(true);
    expect(shouldSnapThreadPanel(520, 286)).toBe(true);
  });
});
