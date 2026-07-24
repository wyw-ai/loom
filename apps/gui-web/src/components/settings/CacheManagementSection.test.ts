import { describe, expect, it } from "vitest";

import { calcPercent } from "@/components/settings/CacheManagementSection";

describe("calcPercent", () => {
  it("returns 100 when size equals total", () => {
    expect(calcPercent(500, 500)).toBe(100);
  });

  it("returns 0 when size is zero", () => {
    expect(calcPercent(0, 500)).toBe(0);
  });

  it("returns 50 for half", () => {
    expect(calcPercent(250, 500)).toBe(50);
  });

  it("rounds to nearest integer", () => {
    // 1/3 ≈ 33.33% → rounds to 33
    expect(calcPercent(1, 3)).toBe(33);
    // 2/3 ≈ 66.67% → rounds to 67
    expect(calcPercent(2, 3)).toBe(67);
  });

  it("rounds 0.5 up (Math.round behavior)", () => {
    // 1/8 = 12.5% → Math.round rounds to 13
    expect(calcPercent(1, 8)).toBe(13);
  });

  it("returns null when size is null", () => {
    expect(calcPercent(null, 500)).toBeNull();
  });

  it("returns null when total is null", () => {
    expect(calcPercent(500, null)).toBeNull();
  });

  it("returns null when both are null", () => {
    expect(calcPercent(null, null)).toBeNull();
  });

  it("returns null when total is zero (division guard)", () => {
    expect(calcPercent(100, 0)).toBeNull();
  });

  it("returns null when total is negative (division guard)", () => {
    expect(calcPercent(100, -50)).toBeNull();
  });

  it("handles large numbers", () => {
    // 1GB out of 4GB = 25%
    expect(calcPercent(1_073_741_824, 4_294_967_296)).toBe(25);
  });

  it("can exceed 100 if size > total (edge case)", () => {
    expect(calcPercent(600, 500)).toBe(120);
  });
});
