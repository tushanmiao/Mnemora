import { describe, expect, it, vi } from "vitest";
import {
  extractMermaidSvgMetrics,
  measureMermaidViewerBudget,
  normalizeMermaidSvgForXml,
  sanitizeMermaidSvg,
} from "./mermaidSecurity";

describe("mermaidSecurity", () => {
  it("normalizes Mermaid html labels before XML parsing", () => {
    const source = '<svg><foreignObject><div xmlns="http://www.w3.org/1999/xhtml"><p>第一行<br>第二行<br class="gap">第三行<br/></p><hr><img src="#asset"></div></foreignObject></svg>';

    expect(normalizeMermaidSvgForXml(source)).toBe(
      '<svg><foreignObject><div xmlns="http://www.w3.org/1999/xhtml"><p>第一行<br />第二行<br class="gap" />第三行<br/></p><hr /><img src="#asset" /></div></foreignObject></svg>',
    );
  });

  it("passes normalized HTML labels to the strict XML sanitizer", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const previousSerializer = globalThis.XMLSerializer;
    let parsedInput = "";
    const root = {
      tagName: "svg",
      outerHTML: '<svg viewBox="0 0 320 180"></svg>',
      getAttribute: (name: string) => name === "viewBox" ? "0 0 320 180" : null,
      querySelectorAll: () => [],
      setAttribute: vi.fn(),
      removeAttribute: vi.fn(),
      style: { removeProperty: vi.fn(), setProperty: vi.fn() },
    };

    try {
      globalThis.DOMParser = class {
        parseFromString(input: string) {
          parsedInput = input;
          return { documentElement: root, querySelector: () => null };
        }
      } as unknown as typeof DOMParser;
      globalThis.document = {} as Document;
      globalThis.XMLSerializer = class { serializeToString() { return root.outerHTML; } } as unknown as typeof XMLSerializer;

      sanitizeMermaidSvg('<svg><foreignObject><p>一<br>二</p></foreignObject></svg>');

      expect(parsedInput).toContain("<br />");
      expect(parsedInput).not.toContain("<br>");
    } finally {
      globalThis.DOMParser = previousDomParser;
      globalThis.document = previousDocument;
      globalThis.XMLSerializer = previousSerializer;
    }
  });

  it("reads real diagram dimensions from a whitespace-separated viewBox", () => {
    expect(extractMermaidSvgMetrics('<svg viewBox="0\n0\n1920\n640"></svg>')).toMatchObject({
      width: 1920,
      height: 640,
      aspectRatio: 3,
    });
  });

  it("retains the viewBox origin used by Mermaid labels and markers", () => {
    expect(extractMermaidSvgMetrics('<svg viewBox="-24 -12 640 360"></svg>')).toMatchObject({
      x: -24,
      y: -12,
      width: 640,
      height: 360,
    });
  });

  it("falls back to explicit width and height attributes", () => {
    expect(extractMermaidSvgMetrics('<svg width="800px" height="600px"></svg>')).toMatchObject({
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
      // The cap must be Mermaid's measured width, not the container width;
      // 100% here let narrow diagrams stretch and magnify their labels.
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
      expect(result.metrics).toMatchObject({ width: 800, height: 600, aspectRatio: 800 / 600 });
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
    expect(result.metrics).toMatchObject({ width: 720, height: 360, aspectRatio: 2, viewerSafe: true });
  });

  it("marks oversized interactive viewer payloads as unsafe", () => {
    const oversized = `<svg viewBox="0 0 142 44138">${"<foreignObject></foreignObject>".repeat(801)}</svg>`;
    const metrics = measureMermaidViewerBudget(oversized);

    expect(metrics.foreignObjectCount).toBe(801);
    expect(metrics.viewerSafe).toBe(false);
  });
});
