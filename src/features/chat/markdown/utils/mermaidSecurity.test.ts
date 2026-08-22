import { describe, expect, it, vi } from "vitest";
import {
  extractMermaidSvgMetrics,
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

  it("preserves intrinsic SVG dimensions for browser layout", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const previousSerializer = globalThis.XMLSerializer;

    try {
      globalThis.DOMParser = class {
        parseFromString() {
          return {
            documentElement: {
              tagName: "svg",
              outerHTML: '<svg viewBox="0 0 617 1162"></svg>',
              querySelectorAll: () => [],
              setAttribute: vi.fn(),
              removeAttribute: vi.fn(),
              style: { removeProperty: vi.fn() },
            },
          };
        }
      } as unknown as typeof DOMParser;
      globalThis.document = {} as Document;
      globalThis.XMLSerializer = class {
        serializeToString(root: { setAttribute: ReturnType<typeof vi.fn> }) {
          const attributes = Object.fromEntries(root.setAttribute.mock.calls);
          return `<svg width="${attributes.width}" height="${attributes.height}"></svg>`;
        }
      } as unknown as typeof XMLSerializer;

      const result = sanitizeMermaidSvg('<svg viewBox="0 0 617 1162"></svg>');

      expect(result.svg).toContain('width="617"');
      expect(result.svg).toContain('height="1162"');
    } finally {
      globalThis.DOMParser = previousDomParser;
      globalThis.document = previousDocument;
      globalThis.XMLSerializer = previousSerializer;
    }
  });

  it("keeps a safe fallback contract when browser DOM APIs are unavailable", () => {
    const source = '<svg viewBox="0 0 720 360"><script>alert(1)</script></svg>';
    const result = sanitizeMermaidSvg(source);

    expect(result.svg).toBe(source);
    expect(result.metrics).toEqual({ width: 720, height: 360, aspectRatio: 2 });
  });
});
