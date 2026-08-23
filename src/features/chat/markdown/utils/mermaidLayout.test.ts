import { describe, expect, it } from "vitest";
import {
  getDefaultMermaidViewMode,
  getMermaidPreviewLayout,
  getMermaidPreviewViewMode,
  getMermaidViewerScale,
  getMermaidViewerViewport,
  isLargeMermaidDiagram,
} from "./mermaidLayout";

describe("mermaidLayout", () => {
  const tallDiagram = { width: 617, height: 1162, aspectRatio: 617 / 1162 };

  it("keeps a tall diagram readable instead of compressing it to a fixed height", () => {
    const layout = getMermaidPreviewLayout(tallDiagram, 868);

    expect(layout.minRenderWidth).toBe(617);
    expect(layout.maxRenderWidth).toBe(617);
    expect(layout.projectedWidth).toBe(617);
    expect(layout.projectedHeight).toBe(1_162);
    expect(layout.requiresViewport).toBe(true);
  });

  it("leaves an ordinary landscape diagram fully visible", () => {
    const metrics = { width: 900, height: 500, aspectRatio: 1.8 };
    const layout = getMermaidPreviewLayout(metrics, 868);

    expect(layout.projectedWidth).toBe(868);
    expect(layout.projectedHeight).toBeCloseTo(482.22, 1);
    expect(isLargeMermaidDiagram(metrics, 868)).toBe(false);
  });

  it("uses fit-width by default for tall diagrams and fit-window for other diagrams", () => {
    expect(getDefaultMermaidViewMode(tallDiagram)).toBe("width");
    expect(getDefaultMermaidViewMode({ width: 900, height: 500, aspectRatio: 1.8 })).toBe("fit");
  });

  it("calculates distinct fit-window, fit-width, and actual-size scales", () => {
    const canvas = { width: 1_200, height: 700 };

    expect(getMermaidViewerScale(tallDiagram, canvas, "fit")).toBeCloseTo(0.561, 2);
    // Fit-width used to magnify this diagram 1.87x, which blew 13px labels up
    // past 24px. Fit modes shrink only; magnifying is the zoom control's job.
    expect(getMermaidViewerScale(tallDiagram, canvas, "width")).toBe(1);
    expect(getMermaidViewerScale(tallDiagram, canvas, "actual")).toBe(1);
  });

  it("never magnifies a diagram that is smaller than the canvas", () => {
    const small = { width: 432, height: 240, aspectRatio: 1.8 };
    const canvas = { width: 1_338, height: 560 };

    expect(getMermaidViewerScale(small, canvas, "fit")).toBe(1);
    expect(getMermaidViewerScale(small, canvas, "width")).toBe(1);

    const viewport = getMermaidViewerViewport(small, canvas, "fit");
    expect(viewport.scale).toBe(1);
    // The viewBox covers the whole diagram plus margin instead of cropping it.
    expect(viewport.width).toBeGreaterThanOrEqual(small.width);
    expect(viewport.height).toBeGreaterThanOrEqual(small.height);
  });

  it("shows the reported regression diagram in full instead of a magnified slice", () => {
    // Measured from the failing note: mermaid produced 432.6 x 681.5, which is
    // 1.5px past PREVIEW_HEIGHT_LIMIT and therefore took the viewport path.
    const metrics = { width: 432.64, height: 681.5, aspectRatio: 432.64 / 681.5 };
    const canvas = { width: 1_338, height: 560 };

    expect(isLargeMermaidDiagram(metrics, canvas.width)).toBe(true);
    expect(getMermaidPreviewViewMode(metrics, canvas)).toBe("fit");

    const viewport = getMermaidViewerViewport(metrics, canvas, getMermaidPreviewViewMode(metrics, canvas));
    expect(viewport.scale).toBeLessThan(1);
    expect(viewport.height).toBeGreaterThanOrEqual(metrics.height);
    expect(13 * viewport.scale).toBeLessThan(13);
  });

  it("falls back to top-anchored fit-width when shrinking would be illegible", () => {
    const veryTall = { width: 142, height: 44_138, aspectRatio: 142 / 44_138 };
    const canvas = { width: 1_200, height: 700 };

    expect(getMermaidPreviewViewMode(veryTall, canvas)).toBe("width");
    expect(getMermaidViewerScale(veryTall, canvas, "width")).toBe(1);
  });

  it("keeps a 44,000px diagram inside a bounded fit-width viewBox", () => {
    const metrics = { width: 142, height: 44_138, aspectRatio: 142 / 44_138 };
    const viewport = getMermaidViewerViewport(metrics, { width: 1_200, height: 700 }, "width", 1, { x: 0, y: 0 });

    // Fit-width no longer magnifies the 142px-wide chart 8x; it renders at 100%
    // and the viewBox stays bounded by the canvas.
    expect(viewport.scale).toBe(1);
    expect(viewport.width).toBeCloseTo(1_200, 1);
    expect(viewport.height).toBeCloseTo(700, 1);
    expect(viewport.x).toBeLessThan(0);
    expect(viewport.y).toBe(0);
  });

  it("pans a tall diagram by moving the viewBox and clamps at both ends", () => {
    const metrics = { width: 600, height: 40_000, aspectRatio: 0.015 };
    const canvas = { width: 1_200, height: 700 };

    const middle = getMermaidViewerViewport(metrics, canvas, "width", 1, { x: 0, y: -20_000 });
    const end = getMermaidViewerViewport(metrics, canvas, "width", 1, { x: 0, y: -1_000_000 });

    expect(middle.y).toBeGreaterThan(0);
    expect(end.y + end.height).toBeCloseTo(metrics.height, 5);
  });
});
