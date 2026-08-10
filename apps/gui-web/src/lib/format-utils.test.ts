import { describe, expect, it } from "vitest";
import { reconnectDelayMs } from "./format-utils";

describe("reconnectDelayMs", () => {
  it("retries once immediately, then backs off with a cap", () => {
    expect(reconnectDelayMs(1)).toBe(0);
    expect(reconnectDelayMs(2)).toBe(500);
    expect(reconnectDelayMs(3)).toBe(1_000);
    expect(reconnectDelayMs(20)).toBe(15_000);
  });
});
