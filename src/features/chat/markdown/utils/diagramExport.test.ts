import { describe, expect, it } from "vitest";
import {
  base64ToBytes,
  bytesToBase64,
  decodeSvgDataUrl,
  diagramSaveOptions,
  rasterTargetSize,
  textToBase64,
} from "./diagramExport";

describe("decodeSvgDataUrl", () => {
  it("recovers percent-encoded SVG produced by the preview source", () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"><text>主机 (Host)</text></svg>';
    const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
    expect(decodeSvgDataUrl(url)).toBe(svg);
  });

  it("recovers base64-encoded SVG as well", () => {
    const svg = "<svg><rect /></svg>";
    expect(decodeSvgDataUrl(`data:image/svg+xml;base64,${textToBase64(svg)}`)).toBe(svg);
  });

  it("refuses bitmap sources so callers can tell there is no vector to export", () => {
    expect(decodeSvgDataUrl("data:image/png;base64,iVBORw0KGgo=")).toBeNull();
    expect(decodeSvgDataUrl("https://example.com/a.svg")).toBeNull();
    expect(decodeSvgDataUrl("")).toBeNull();
  });
});

describe("rasterTargetSize", () => {
  it("doubles small diagrams for crisp output on high-DPI screens", () => {
    expect(rasterTargetSize(400, 300)).toEqual({ width: 800, height: 600, ratio: 2 });
  });

  it("caps the long edge so extreme sequence diagrams still rasterize", () => {
    const size = rasterTargetSize(60_000, 200);
    expect(size).not.toBeNull();
    if (!size) return;
    expect(size.width).toBeLessThanOrEqual(8_192);
    expect(size.height).toBeLessThanOrEqual(8_192);
  });

  it("caps total pixels so the canvas does not silently return blank", () => {
    const size = rasterTargetSize(6_000, 6_000);
    expect(size).not.toBeNull();
    if (!size) return;
    expect(size.width * size.height).toBeLessThanOrEqual(16_777_216);
  });

  it("never upscales past the caps yet always leaves at least one pixel", () => {
    const size = rasterTargetSize(0.2, 0.2);
    expect(size).not.toBeNull();
    if (!size) return;
    expect(size.width).toBeGreaterThanOrEqual(1);
    expect(size.height).toBeGreaterThanOrEqual(1);
  });

  it("rejects degenerate dimensions instead of producing a 0-pixel canvas", () => {
    expect(rasterTargetSize(0, 100)).toBeNull();
    expect(rasterTargetSize(100, Number.NaN)).toBeNull();
    expect(rasterTargetSize(-5, 5)).toBeNull();
  });
});

describe("base64 round trip", () => {
  it("survives non-ASCII text, which naive btoa would reject", () => {
    const value = "主机 (Host) → 笔记管道";
    expect(new TextDecoder().decode(base64ToBytes(textToBase64(value)))).toBe(value);
  });

  it("chunks large byte arrays instead of blowing the call stack", () => {
    const bytes = new Uint8Array(200_000);
    for (let index = 0; index < bytes.length; index += 1) bytes[index] = index % 256;
    const restored = base64ToBytes(bytesToBase64(bytes));
    expect(restored.length).toBe(bytes.length);
    expect(restored[0]).toBe(bytes[0]);
    expect(restored[199_999]).toBe(bytes[199_999]);
  });
});

describe("diagramSaveOptions", () => {
  it("swaps the extension so PNG and SVG exports never collide", () => {
    expect(diagramSaveOptions("png", "mermaid-diagram.svg").defaultPath)
      .toBe("mermaid-diagram.png");
    expect(diagramSaveOptions("svg", "mermaid-diagram.svg").defaultPath)
      .toBe("mermaid-diagram.svg");
  });

  it("falls back to a usable name when the caller passes nothing", () => {
    expect(diagramSaveOptions("png", "   ").defaultPath).toBe("diagram.png");
  });

  it("declares a filter matching the chosen format", () => {
    expect(diagramSaveOptions("png", "a").filters[0].extensions).toEqual(["png"]);
    expect(diagramSaveOptions("svg", "a").filters[0].extensions).toEqual(["svg"]);
  });
});
