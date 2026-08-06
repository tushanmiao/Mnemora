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
});
