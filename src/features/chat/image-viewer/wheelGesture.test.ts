import { describe, expect, it } from "vitest";
import {
  clampZoom,
  MAX_ZOOM,
  MIN_ZOOM,
  normalizeWheelDelta,
  resolveWheelGesture,
} from "./wheelGesture";

describe("normalizeWheelDelta", () => {
  it("passes pixel deltas through unchanged", () => {
    expect(normalizeWheelDelta(-120, 0)).toBe(-120);
  });

  it("scales line and page modes so Firefox does not crawl", () => {
    // 行模式下 deltaY 是「几行」，不换算的话滚一下只动 3 像素。
    expect(normalizeWheelDelta(3, 1)).toBe(48);
    expect(normalizeWheelDelta(1, 2)).toBe(400);
  });

  it("treats non-finite deltas as no movement", () => {
    expect(normalizeWheelDelta(Number.NaN)).toBe(0);
    expect(normalizeWheelDelta(Number.POSITIVE_INFINITY)).toBe(0);
  });
});

describe("clampZoom", () => {
  it("keeps zoom inside the shared bounds", () => {
    expect(clampZoom(0.1)).toBe(MIN_ZOOM);
    expect(clampZoom(99)).toBe(MAX_ZOOM);
    expect(clampZoom(1.5)).toBe(1.5);
  });
});

describe("resolveWheelGesture", () => {
  it("pans on a bare wheel so scrolling still works", () => {
    expect(resolveWheelGesture({ deltaX: 8, deltaY: -120 }, 1)).toEqual({
      kind: "pan",
      dx: 8,
      dy: -120,
    });
  });

  it("zooms in when Ctrl is held and the wheel moves up", () => {
    const outcome = resolveWheelGesture({ deltaX: 0, deltaY: -100, ctrlKey: true }, 1);
    expect(outcome.kind).toBe("zoom");
    if (outcome.kind !== "zoom") return;
    expect(outcome.zoom).toBeGreaterThan(1);
  });

  it("zooms out when Ctrl is held and the wheel moves down", () => {
    const outcome = resolveWheelGesture({ deltaX: 0, deltaY: 100, ctrlKey: true }, 1);
    expect(outcome.kind).toBe("zoom");
    if (outcome.kind !== "zoom") return;
    expect(outcome.zoom).toBeLessThan(1);
  });

  it("steps proportionally so each notch feels the same at any scale", () => {
    const fromHalf = resolveWheelGesture({ deltaX: 0, deltaY: -100, ctrlKey: true }, 0.8);
    const fromDouble = resolveWheelGesture({ deltaX: 0, deltaY: -100, ctrlKey: true }, 2);
    if (fromHalf.kind !== "zoom" || fromDouble.kind !== "zoom") throw new Error("expected zoom");
    expect(fromHalf.zoom / 0.8).toBeCloseTo(fromDouble.zoom / 2, 5);
  });

  it("treats Cmd like Ctrl so macOS is not left without a zoom key", () => {
    expect(resolveWheelGesture({ deltaX: 0, deltaY: -100, metaKey: true }, 1).kind).toBe("zoom");
  });

  it("ignores the gesture at the zoom bounds instead of swallowing the scroll", () => {
    // 贴在上界还返回 zoom，调用方就会 preventDefault，用户会以为面板卡死。
    expect(resolveWheelGesture({ deltaX: 0, deltaY: -100, ctrlKey: true }, MAX_ZOOM)).toEqual({
      kind: "ignore",
    });
    expect(resolveWheelGesture({ deltaX: 0, deltaY: 100, ctrlKey: true }, MIN_ZOOM)).toEqual({
      kind: "ignore",
    });
  });

  it("ignores empty wheel events in both modes", () => {
    expect(resolveWheelGesture({ deltaX: 0, deltaY: 0 }, 1)).toEqual({ kind: "ignore" });
    expect(resolveWheelGesture({ deltaX: 0, deltaY: 0, ctrlKey: true }, 1)).toEqual({
      kind: "ignore",
    });
  });

  it("normalizes line-mode deltas before deciding to zoom", () => {
    // 行模式 + Ctrl：不换算的话步长小到几乎看不出缩放。
    const lineMode = resolveWheelGesture({ deltaX: 0, deltaY: -3, deltaMode: 1, ctrlKey: true }, 1);
    const pixelMode = resolveWheelGesture({ deltaX: 0, deltaY: -48, ctrlKey: true }, 1);
    if (lineMode.kind !== "zoom" || pixelMode.kind !== "zoom") throw new Error("expected zoom");
    expect(lineMode.zoom).toBeCloseTo(pixelMode.zoom, 10);
  });
});
