import { describe, expect, it, vi } from "vitest";
import {
  DecodedImageBudget,
  estimateDecodedImageBytes,
  estimatePreviewDecodedBytes,
} from "./imageDecodeBudget";

describe("DecodedImageBudget", () => {
  it("bounds the total visible decoded image estimate", () => {
    const budget = new DecodedImageBudget(1_000);
    const first = budget.reserve({ owner: "first", estimatedBytes: 700, onEvict: vi.fn() });
    const second = budget.reserve({ owner: "second", estimatedBytes: 400, onEvict: vi.fn() });
    expect(first).not.toBeNull();
    expect(second).toBeNull();
    expect(budget.usedBytes).toBe(700);
    first?.release();
    expect(budget.usedBytes).toBe(0);
  });

  it("estimates RGBA decode size and clamps preview dimensions", () => {
    expect(estimateDecodedImageBytes(1_000, 500)).toBe(2_000_000);
    expect(estimatePreviewDecodedBytes(4_000, 2_000)).toBe(640 * 320 * 4);
  });
});
