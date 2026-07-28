import { describe, expect, it } from "vitest";
import { resolvePdfCanvasScale, resolvePdfPageDisplaySize } from "./pdfViewport";

describe("PDF 页面尺寸", () => {
  it("以阅读区域而不是已经放大的页面宽度计算缩放", () => {
    const normal = resolvePdfPageDisplaySize(1000, 600, 800, 1);
    const enlarged = resolvePdfPageDisplaySize(1000, 600, 800, 2);

    expect(normal.width).toBe(924);
    expect(enlarged.width).toBe(1848);
    expect(enlarged.height).toBeCloseTo(normal.height * 2);
  });

  it("限制高倍缩放页面的 Canvas 后备像素", () => {
    const scale = resolvePdfCanvasScale(3300, 4667, 2);
    expect(3300 * scale * 4667 * scale).toBeLessThanOrEqual(12_000_001);
    expect(scale).toBeLessThan(1);
  });
});
