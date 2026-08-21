import { describe, expect, it } from "vitest";
import {
  extractMermaidSvgMetrics,
  isLargeMermaidDiagram,
  sanitizeMermaidSvg,
} from "./mermaidSecurity";

describe("mermaidSecurity", () => {
  it("reads real diagram dimensions from a whitespace-separated viewBox", () => {
    expect(extractMermaidSvgMetrics('<svg viewBox="0\n0\n1920\n640"></svg>')).toEqual({
      width: 1920,
      height: 640,
      aspectRatio: 3,
    });
  });

  it("falls back to explicit width and height attributes", () => {
    expect(extractMermaidSvgMetrics('<svg width="800px" height="600px"></svg>')).toEqual({
      width: 800,
      height: 600,
      aspectRatio: 800 / 600,
    });
  });

  it("classifies oversized and extreme-aspect diagrams", () => {
    expect(isLargeMermaidDiagram({ width: 1900, height: 600, aspectRatio: 1900 / 600 })).toBe(true);
    expect(isLargeMermaidDiagram({ width: 600, height: 1700, aspectRatio: 600 / 1700 })).toBe(true);
    expect(isLargeMermaidDiagram({ width: 900, height: 500, aspectRatio: 1.8 })).toBe(false);
  });

  it("keeps a safe fallback contract when browser DOM APIs are unavailable", () => {
    const source = '<svg viewBox="0 0 720 360"><script>alert(1)</script></svg>';
    const result = sanitizeMermaidSvg(source);

    expect(result.svg).toBe(source);
    expect(result.metrics).toEqual({ width: 720, height: 360, aspectRatio: 2 });
  });
});
