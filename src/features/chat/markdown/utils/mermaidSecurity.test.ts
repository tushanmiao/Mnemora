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

  it("sanitizes root event attributes and external SVG/CSS resources", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const previousSerializer = globalThis.XMLSerializer;
    const attributes = new Map<string, string>([
      ["viewBox", "0 0 640 360"],
      ["onclick", "alert(1)"],
      ["style", "background:url(https://evil.invalid/a.png)"],
    ]);
    const styleElement = { tagName: "style", textContent: "@import url(https://evil.invalid/a.css); .x{fill:url(https://evil.invalid/a.png)}", remove: vi.fn() };
    const root = {
      tagName: "svg",
      outerHTML: "<svg></svg>",
      get attributes() {
        return [...attributes].map(([name, value]) => ({ name, value }));
      },
      getAttribute: (name: string) => attributes.get(name) ?? null,
      querySelectorAll: () => [styleElement],
      setAttribute: (name: string, value: string) => attributes.set(name, value),
      removeAttribute: (name: string) => attributes.delete(name),
      style: { removeProperty: vi.fn(), setProperty: vi.fn() },
    };
    try {
      globalThis.DOMParser = class { parseFromString() { return { documentElement: root, querySelector: () => null }; } } as unknown as typeof DOMParser;
      globalThis.document = {} as Document;
      globalThis.XMLSerializer = class { serializeToString() { return JSON.stringify({ attributes: [...attributes], css: styleElement.textContent }); } } as unknown as typeof XMLSerializer;

      const result = sanitizeMermaidSvg("<svg />");
      expect(result.svg).not.toContain("onclick");
      expect(result.svg).not.toContain("evil.invalid");
    } finally {
      globalThis.DOMParser = previousDomParser;
      globalThis.document = previousDocument;
      globalThis.XMLSerializer = previousSerializer;
    }
  });

  it("makes SVG width adaptive while preserving its intrinsic max width", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const previousSerializer = globalThis.XMLSerializer;

    try {
      globalThis.DOMParser = class {
        parseFromString() {
          return {
            querySelector: () => null,
            documentElement: {
              tagName: "svg",
              outerHTML: '<svg viewBox="0 0 617 1162"></svg>',
              getAttribute: (name: string) => name === "viewBox" ? "0 0 617 1162" : null,
              querySelectorAll: () => [],
              setAttribute: vi.fn(),
              removeAttribute: vi.fn(),
              style: { removeProperty: vi.fn(), setProperty: vi.fn() },
            },
          };
        }
      } as unknown as typeof DOMParser;
      globalThis.document = {} as Document;
      globalThis.XMLSerializer = class {
        serializeToString(root: {
          setAttribute: ReturnType<typeof vi.fn>;
          style: { setProperty: ReturnType<typeof vi.fn> };
        }) {
          const attributes = Object.fromEntries(root.setAttribute.mock.calls);
          const maxWidth = root.style.setProperty.mock.calls.find((call: unknown[]) => call[0] === "max-width")?.[1];
          return `<svg width="${attributes.width}" max-width="${maxWidth}"></svg>`;
        }
      } as unknown as typeof XMLSerializer;

      const result = sanitizeMermaidSvg('<svg viewBox="0 0 617 1162"></svg>');

      expect(result.svg).toContain('width="100%"');
      expect(result.svg).toContain('max-width="617px"');
    } finally {
      globalThis.DOMParser = previousDomParser;
      globalThis.document = previousDocument;
      globalThis.XMLSerializer = previousSerializer;
    }
  });

  it("adds a viewBox from explicit dimensions and preserves aspect ratio", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const previousSerializer = globalThis.XMLSerializer;
    const attributes = new Map<string, string>([["width", "800px"], ["height", "600px"]]);

    try {
      globalThis.DOMParser = class {
        parseFromString() {
          const root = {
            tagName: "svg",
            get outerHTML() {
              const serialized = [...attributes].map(([name, value]) => `${name}="${value}"`).join(" ");
              return `<svg ${serialized}></svg>`;
            },
            getAttribute: (name: string) => attributes.get(name) ?? null,
            querySelectorAll: () => [],
            setAttribute: (name: string, value: string) => attributes.set(name, value),
            removeAttribute: (name: string) => attributes.delete(name),
            style: { removeProperty: vi.fn(), setProperty: vi.fn() },
          };
          return { documentElement: root, querySelector: () => null };
        }
      } as unknown as typeof DOMParser;
      globalThis.document = {} as Document;
      globalThis.XMLSerializer = class {
        serializeToString() {
          return `<svg viewBox="${attributes.get("viewBox")}" preserveAspectRatio="${attributes.get("preserveAspectRatio")}"></svg>`;
        }
      } as unknown as typeof XMLSerializer;

      const result = sanitizeMermaidSvg('<svg width="800px" height="600px"></svg>');

      expect(result.svg).toContain('viewBox="0 0 800 600"');
      expect(result.svg).toContain('preserveAspectRatio="xMidYMin meet"');
      expect(result.metrics).toEqual({ width: 800, height: 600, aspectRatio: 800 / 600 });
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
