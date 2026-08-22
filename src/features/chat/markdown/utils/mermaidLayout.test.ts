import { describe, expect, it } from "vitest";
import {
  getDefaultMermaidViewMode,
  getMermaidPreviewLayout,
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
    expect(getMermaidViewerScale(tallDiagram, canvas, "width")).toBeCloseTo(1.867, 2);
    expect(getMermaidViewerScale(tallDiagram, canvas, "actual")).toBe(1);
  });

  it("keeps a 44,000px diagram inside a bounded fit-width viewBox", () => {
    const metrics = { width: 142, height: 44_138, aspectRatio: 142 / 44_138 };
    const viewport = getMermaidViewerViewport(metrics, { width: 1_200, height: 700 }, "width", 1, { x: 0, y: 0 });

    expect(viewport.scale).toBeCloseTo(8.113, 2);
    expect(viewport.width).toBeCloseTo(147.92, 1);
    expect(viewport.height).toBeCloseTo(86.29, 1);
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
