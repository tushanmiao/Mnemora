import { describe, expect, it } from "vitest";
import { PdfRenderScheduler } from "./pdfRenderScheduler";

describe("PdfRenderScheduler", () => {
  it("preempts an adjacent render when the current page is queued", async () => {
    const scheduler = new PdfRenderScheduler();
    let adjacentAborted = false;
    const adjacent = scheduler.schedule("adjacent", 10, (signal) => new Promise<void>((resolve) => {
      signal.addEventListener("abort", () => { adjacentAborted = true; resolve(); }, { once: true });
    }));
    const current = scheduler.schedule("current", 0, async () => undefined);
    await Promise.all([adjacent.promise, current.promise]);
    expect(adjacentAborted).toBe(true);
    scheduler.dispose();
  });

  it("cancels queued work by resource prefix", async () => {
    const scheduler = new PdfRenderScheduler();
    let activeAborted = false;
    const first = scheduler.schedule("pdf-thumbnail:1", 20, (signal) => new Promise<void>((resolve) => {
      signal.addEventListener("abort", () => { activeAborted = true; resolve(); }, { once: true });
    }));
    const second = scheduler.schedule("pdf-thumbnail:2", 20, async () => undefined);
    scheduler.cancelByPrefix("pdf-thumbnail:");
    await Promise.all([first.promise, second.promise]);
    expect(activeAborted).toBe(true);
    scheduler.dispose();
  });
});
