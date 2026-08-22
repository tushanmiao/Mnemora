import { describe, expect, it, vi } from "vitest";
import { renderMermaidSvgInShadowHost } from "./mermaidShadow";

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
});
