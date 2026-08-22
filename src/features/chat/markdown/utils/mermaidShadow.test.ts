import { describe, expect, it, vi } from "vitest";
import { renderMermaidSvgInShadowHost, updateMermaidSvgViewport } from "./mermaidShadow";

describe("mermaidShadow", () => {
  it("keeps intrinsic dimensions without forcing a host aspect-ratio height", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const root = {
      tagName: "svg",
      getAttribute: (name: string) => name === "viewBox" ? "0 0 617 1162" : null,
    };
    const styleProperties = new Map<string, string>();
    const shadow = { replaceChildren: vi.fn(), append: vi.fn() };
    const host = {
      shadowRoot: null,
      attachShadow: vi.fn(() => shadow),
      style: {
        aspectRatio: "",
        setProperty: (name: string, value: string) => styleProperties.set(name, value),
      },
    } as unknown as HTMLElement;

    try {
      globalThis.DOMParser = class {
        parseFromString() {
          return { documentElement: root, querySelector: () => null };
        }
      } as unknown as typeof DOMParser;
      globalThis.document = {
        createElement: () => ({ style: { textContent: "" } }),
        importNode: (value: unknown) => value,
      } as unknown as Document;

      renderMermaidSvgInShadowHost('<svg viewBox="0 0 617 1162"></svg>', host);

      expect(host.style.aspectRatio).toBe("");
      expect(styleProperties.get("--mermaid-intrinsic-width")).toBe("617px");
      expect(styleProperties.get("--mermaid-intrinsic-height")).toBe("1162px");
      expect(styleProperties.get("--mermaid-aspect-ratio")).toBe("617 / 1162");
      expect(shadow.append).toHaveBeenCalledWith(expect.anything(), root);
    } finally {
      globalThis.DOMParser = previousDomParser;
      globalThis.document = previousDocument;
    }
  });

  it("uses a bounded viewBox for the interactive viewer", () => {
    const previousDomParser = globalThis.DOMParser;
    const previousDocument = globalThis.document;
    const attributes = new Map<string, string>([["viewBox", "0 0 142 44138"]]);
    const root = {
      tagName: "svg",
      getAttribute: (name: string) => attributes.get(name) ?? null,
      setAttribute: (name: string, value: string) => attributes.set(name, value),
    };
    const shadow = { replaceChildren: vi.fn(), append: vi.fn() };
    const host = {
      shadowRoot: null,
      attachShadow: vi.fn(() => shadow),
      style: { setProperty: vi.fn() },
    } as unknown as HTMLElement;

    try {
      globalThis.DOMParser = class {
        parseFromString() {
          return { documentElement: root, querySelector: () => null };
        }
      } as unknown as typeof DOMParser;
      globalThis.document = {
        createElement: () => ({ style: { textContent: "" } }),
        importNode: (value: unknown) => value,
      } as unknown as Document;

      renderMermaidSvgInShadowHost(
        '<svg viewBox="0 0 142 44138"></svg>',
        host,
        { x: 0, y: 200, width: 142, height: 175 },
      );

      expect(attributes.get("viewBox")).toBe("0 200 142 175");
      expect(attributes.get("width")).toBe("100%");
      expect(attributes.get("height")).toBe("100%");
    } finally {
      globalThis.DOMParser = previousDomParser;
      globalThis.document = previousDocument;
    }
  });

  it("updates viewer navigation without rebuilding the SVG tree", () => {
    const setAttribute = vi.fn();
    const host = {
      shadowRoot: { querySelector: () => ({ setAttribute }) },
    } as unknown as HTMLElement;

    expect(updateMermaidSvgViewport(host, { x: 0, y: 800, width: 600, height: 350 })).toBe(true);
    expect(setAttribute).toHaveBeenCalledWith("viewBox", "0 800 600 350");
    expect(setAttribute).toHaveBeenCalledWith("width", "100%");
    expect(setAttribute).toHaveBeenCalledWith("height", "100%");
  });
});
