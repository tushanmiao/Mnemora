import { describe, expect, it, vi } from "vitest";
import {
  mountMermaidSvg,
  readMermaidIntrinsicSize,
  syncMermaidOverflow,
} from "./mermaidDom";

describe("mermaidDom", () => {
  it("mounts one real SVG and preserves Mermaid's original viewBox", () => {
    const styles: Record<string, string> = {};
    const svg = {
      getAttribute: (name: string) => name === "viewBox" ? "-12 -8 920 480" : null,
      setAttribute: vi.fn(),
      style: styles,
    } as unknown as SVGSVGElement;
    const template = {
      innerHTML: "",
      content: { querySelector: () => svg },
    };
    const replaceChildren = vi.fn();
    const host = {
      ownerDocument: { createElement: () => template },
      replaceChildren,
    } as unknown as HTMLElement;

    expect(mountMermaidSvg(host, '<svg viewBox="-12 -8 920 480"></svg>')).toBe(svg);
    expect(styles.width).toBe("920px");
    expect(styles.height).toBe("auto");
    expect(styles.maxWidth).toBe("100%");
    expect(svg.setAttribute).not.toHaveBeenCalledWith("viewBox", expect.anything());
    expect(replaceChildren).toHaveBeenCalledWith(svg);
  });

  it("shows the open-diagram affordance only when natural width is reduced", () => {
    const toggleAttribute = vi.fn();
    const block = { toggleAttribute } as unknown as HTMLElement;
    const host = {
      ownerDocument: {
        defaultView: {
          getComputedStyle: () => ({
            borderLeftWidth: "1px",
            borderRightWidth: "1px",
            paddingLeft: "16px",
            paddingRight: "16px",
          }),
        },
      },
      getBoundingClientRect: () => ({ width: 800 }),
      clientWidth: 800,
    } as unknown as HTMLElement;
    const svg = {
      getAttribute: (name: string) => name === "viewBox" ? "0 0 1200 600" : null,
    } as unknown as SVGSVGElement;

    expect(syncMermaidOverflow(block, host, svg)).toBe(true);
    expect(toggleAttribute).toHaveBeenCalledWith("data-mermaid-overflow", true);
  });

  it("reads explicit dimensions only when a viewBox is unavailable", () => {
    const svg = {
      getAttribute: (name: string) => ({ width: "640px", height: "360px" })[name] ?? null,
    } as unknown as SVGSVGElement;

    expect(readMermaidIntrinsicSize(svg)).toEqual({ x: 0, y: 0, width: 640, height: 360 });
  });
});
