import { describe, expect, it, vi } from "vitest";
import {
  extractMermaidSvgMetrics,
  materializeMermaidFallbackPaint,
  measureMermaidViewerBudget,
  mermaidThemeConfig,
  normalizeMermaidSvgForXml,
  prepareMermaidSource,
  sanitizeMermaidSvg,
  stabilizeMermaidSvgPaint,
} from "./mermaidSecurity";

describe("mermaidSecurity", () => {
  it("normalizes Mermaid html labels before XML parsing", () => {
    const source = '<svg><foreignObject><div xmlns="http://www.w3.org/1999/xhtml"><p>第一行<br>第二行<br class="gap">第三行<br/></p><hr><img src="#asset"></div></foreignObject></svg>';

    expect(normalizeMermaidSvgForXml(source)).toBe(
      '<svg><foreignObject><div xmlns="http://www.w3.org/1999/xhtml"><p>第一行<br />第二行<br class="gap" />第三行<br/></p><hr /><img src="#asset" /></div></foreignObject></svg>',
    );
  });

  it("makes flowchart edges stroke-only without rewriting Mermaid's widths", () => {
    const createEdge = (classes: string[], inlineStyle = "") => {
      const attributes = new Map<string, string>(inlineStyle ? [["style", inlineStyle]] : []);
      const style = { setProperty: vi.fn() };
      return {
        attributes,
        style,
        classList: { contains: (name: string) => classes.includes(name) },
        getAttribute: (name: string) => attributes.get(name) ?? null,
        hasAttribute: (name: string) => attributes.has(name),
        setAttribute: (name: string, value: string) => attributes.set(name, value),
      };
    };
    const normal = createEdge(["flowchart-link", "edge-thickness-normal"]);
    const authorStyled = createEdge(["flowchart-link", "edge-thickness-normal"], "stroke-width:4px");
    const thick = createEdge(["flowchart-link", "edge-thickness-thick"]);
    const rootAttributes = new Map<string, string>();
    const root = {
      querySelectorAll: () => [normal, authorStyled, thick],
      setAttribute: (name: string, value: string) => rootAttributes.set(name, value),
    } as unknown as Element;

    expect(stabilizeMermaidSvgPaint(root)).toBe(3);
    expect(normal.attributes.get("fill")).toBe("none");
    expect(normal.attributes.get("stroke-width")).toBeUndefined();
    expect(normal.style.setProperty).toHaveBeenCalledWith("fill", "none", "important");
    expect(authorStyled.attributes.get("stroke-width")).toBeUndefined();
    expect(thick.attributes.get("stroke-width")).toBeUndefined();
    expect(rootAttributes.get("data-mnemora-edge-contract")).toBe("stable");
  });

  it("materializes a readable monochrome fallback when the embedded SVG stylesheet is unavailable", () => {
    const createElement = () => {
      const attributes = new Map<string, string>();
      return {
        attributes,
        setAttribute: (name: string, value: string) => attributes.set(name, value),
      };
    };
    const node = createElement();
    const label = createElement();
    const edge = createElement();
    const marker = createElement();
    const rootAttributes = new Map<string, string>();
    const root = {
      setAttribute: (name: string, value: string) => rootAttributes.set(name, value),
      querySelectorAll: (selector: string) => {
        if (selector === "text, tspan") return [label];
        if (selector.includes("g.node > rect")) return [node];
        if (selector.includes("path.flowchart-link")) return [edge];
        if (selector === "marker path, marker polygon") return [marker];
        if (selector.includes(".node text")) return [label];
        return [];
      },
    } as unknown as Element;

    materializeMermaidFallbackPaint(root, {
      canvas: "#ffffff",
      subtleCanvas: "#f6f7f8",
      alternateCanvas: "#eceff1",
      foreground: "#202427",
      mutedForeground: "#687276",
      border: "#d4d8dc",
      line: "#62686e",
      fontFamily: "Segoe UI",
      fontSize: "13px",
      dark: false,
    });

    expect(node.attributes).toMatchObject(new Map([
      ["fill", "#ffffff"],
      ["stroke", "#d4d8dc"],
      ["stroke-width", "1"],
    ]));
    expect(label.attributes.get("fill")).toBe("#202427");
    expect(label.attributes.get("text-anchor")).toBe("middle");
    expect(label.attributes.get("font-size")).toBe("13px");
    expect(edge.attributes.get("fill")).toBe("none");
    expect(edge.attributes.get("stroke")).toBe("#62686e");
    expect(marker.attributes.get("fill")).toBe("#62686e");
    expect(rootAttributes.get("data-mnemora-paint-fallback")).toBe("materialized");
  });

  it("removes executable directives and click handlers before rendering", () => {
    const source = `%%{init: {'theme': 'dark'}}%%
flowchart TD
  A[Line\\nTwo] --> B
  click A "https://example.com"`;

    expect(prepareMermaidSource(source)).toBe(`flowchart TD
  A[Line<br/>Two] --> B`);
  });

  it("preserves only a validated sequence number color override", () => {
    const source = `%%{init: {'themeVariables': {'sequenceNumberColor': '#abc'}}}%%
sequenceDiagram
  A->>B: hello`;

    expect(prepareMermaidSource(source)).toContain('"sequenceNumberColor":"#abc"');
    expect(prepareMermaidSource(source)).toContain("sequenceDiagram");
  });

  it("rejects attempts to lower Mermaid's security level", () => {
    expect(() => prepareMermaidSource(`%%{init: {'securityLevel': 'loose'}}%%
flowchart LR
  A-->B`)).toThrow("安全级别");
  });

  it("uses the Codex-style monochrome classic path without WebView2 SVG filters", () => {
    const previousGetComputedStyle = globalThis.getComputedStyle;
    try {
      globalThis.getComputedStyle = (() => ({ getPropertyValue: () => "" })) as unknown as typeof getComputedStyle;
      const host = {
        closest: () => null,
        getAttribute: () => null,
      } as unknown as HTMLElement;
      const config = mermaidThemeConfig(host, "flowchart LR\nA-->B");

      expect(config.theme).toBe("neutral");
      expect(config.look).toBe("classic");
      expect(config.htmlLabels).toBe(false);
      expect(config.flowchart).toMatchObject({ curve: "linear", htmlLabels: false });
      expect(config.themeVariables).toMatchObject({
        useGradient: false,
        dropShadow: "none",
        primaryColor: "#ffffff",
        secondaryColor: "#ffffff",
        tertiaryColor: "#ffffff",
        nodeBkg: "#ffffff",
        actorBkg: "#ffffff",
        noteBkgColor: "#ffffff",
        clusterBkg: "#ffffff",
        attributeBackgroundColorOdd: "#ffffff",
        attributeBackgroundColorEven: "#ffffff",
      });
      expect(config.themeVariables.primaryBorderColor).toBe(config.themeVariables.secondaryBorderColor);
      expect(config.themeVariables.primaryBorderColor).toBe(config.themeVariables.tertiaryBorderColor);
      expect(config.themeVariables.lineColor).toBe(config.themeVariables.arrowheadColor);
      expect(new Set([
        config.themeVariables.pie1,
        config.themeVariables.pie2,
        config.themeVariables.pie3,
        config.themeVariables.pie4,
        config.themeVariables.pie5,
        config.themeVariables.pie6,
      ]).size).toBeGreaterThan(1);
    } finally {
      globalThis.getComputedStyle = previousGetComputedStyle;
    }
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

  it("keeps presentation sizing out of the SVG security sanitizer", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const previousSerializer = globalThis.XMLSerializer;

    try {
      const setAttribute = vi.fn();
      const setProperty = vi.fn();
      globalThis.DOMParser = class {
        parseFromString() {
          return {
            querySelector: () => null,
            documentElement: {
              tagName: "svg",
              outerHTML: '<svg viewBox="0 0 617 1162"></svg>',
              getAttribute: (name: string) => name === "viewBox" ? "0 0 617 1162" : null,
              querySelectorAll: () => [],
              setAttribute,
              removeAttribute: vi.fn(),
              style: { removeProperty: vi.fn(), setProperty },
            },
          };
        }
      } as unknown as typeof DOMParser;
      globalThis.document = {} as Document;
      globalThis.XMLSerializer = class { serializeToString() { return '<svg viewBox="0 0 617 1162"></svg>'; } } as unknown as typeof XMLSerializer;

      const result = sanitizeMermaidSvg('<svg viewBox="0 0 617 1162"></svg>');

      expect(result.svg).toContain('viewBox="0 0 617 1162"');
      expect(setAttribute).not.toHaveBeenCalledWith("width", expect.anything());
      expect(setProperty).not.toHaveBeenCalledWith("max-width", expect.anything());
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

  it("rejects pathological intrinsic dimensions without inventing a cropped viewBox", () => {
    const metrics = measureMermaidViewerBudget('<svg viewBox="0 0 142 44138"><path /></svg>');

    expect(metrics.height).toBe(44_138);
    expect(metrics.viewerSafe).toBe(false);
  });
});
