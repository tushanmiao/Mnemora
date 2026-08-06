import { describe, expect, it, vi } from "vitest";
import { PDF_CANVAS_BUDGET_BYTES, PdfCanvasBudget } from "./pdfCanvasBudget";

describe("PdfCanvasBudget", () => {
  it("keeps the default GPU-facing canvas budget at 32 MiB", () => {
    expect(PDF_CANVAS_BUDGET_BYTES).toBe(32 * 1024 * 1024);
  });

  it("never allocates beyond the total byte budget", () => {
    const budget = new PdfCanvasBudget(1_000_000);
    const first = budget.reserve({ owner: "first", width: 500, height: 500, requestedScale: 1, priority: 1, onEvict: vi.fn() });
    const second = budget.reserve({ owner: "second", width: 500, height: 500, requestedScale: 1, priority: 1, onEvict: vi.fn() });
    expect(first).not.toBeNull();
    expect(second).toBeNull();
    expect(budget.usedBytes).toBeLessThanOrEqual(1_000_000);
    first?.release();
  });

  it("evicts lower-priority adjacent pages for the current page", () => {
    const budget = new PdfCanvasBudget(1_500_000);
    const evicted = vi.fn();
    budget.reserve({ owner: "adjacent", width: 500, height: 500, requestedScale: 1, priority: 10, onEvict: evicted });
    const current = budget.reserve({ owner: "current", width: 500, height: 500, requestedScale: 1, priority: 0, onEvict: vi.fn() });
    expect(current).not.toBeNull();
    expect(evicted).toHaveBeenCalledOnce();
    current?.release();
  });

  it("releases every reservation and invokes eviction cleanup", () => {
    const budget = new PdfCanvasBudget(2_000_000);
    const firstEvicted = vi.fn();
    const secondEvicted = vi.fn();
    budget.reserve({ owner: "first", width: 500, height: 500, requestedScale: 1, priority: 1, onEvict: firstEvicted });
    budget.reserve({ owner: "second", width: 250, height: 500, requestedScale: 1, priority: 1, onEvict: secondEvicted });

    budget.releaseAll();

    expect(budget.usedBytes).toBe(0);
    expect(firstEvicted).toHaveBeenCalledOnce();
    expect(secondEvicted).toHaveBeenCalledOnce();
  });
});
