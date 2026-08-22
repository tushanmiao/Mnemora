import { describe, expect, it } from "vitest";
import {
  getDefaultMermaidViewMode,
  getMermaidPreviewLayout,
  getMermaidViewerScale,
  isLargeMermaidDiagram,
} from "./mermaidLayout";

describe("mermaidLayout", () => {
  const tallDiagram = { width: 617, height: 1162, aspectRatio: 617 / 1162 };

  it("keeps a tall diagram readable instead of compressing it to a fixed height", () => {
    const layout = getMermaidPreviewLayout(tallDiagram, 868);

    expect(layout.minRenderWidth).toBe(617);
    expect(layout.maxRenderWidth).toBe(926);
    expect(layout.projectedWidth).toBe(868);
    expect(layout.projectedHeight).toBeGreaterThan(1_600);
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
});
