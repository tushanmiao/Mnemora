import { describe, expect, it } from "vitest";
import {
  buildStorageDonutLabels,
  buildStorageDonutSegments,
  donutSegmentDash,
  getStorageCategoryPresentation,
} from "./StorageSettingsPanel";

function slice(id: string, share: number) {
  return { id, bytes: Math.round(share * 1000), share, color: `var(--chart-series-1)` };
}

describe("getStorageCategoryPresentation", () => {
  it("renders the prompt-library storage category", () => {
    const presentation = getStorageCategoryPresentation("prompts");
    expect(presentation.Icon).toBeTypeOf("object");
    expect(presentation.translationKey).toBe("storage.category.prompts");
  });

  it("falls back safely for a category added by a newer backend", () => {
    const presentation = getStorageCategoryPresentation("future-category");
    expect(presentation.Icon).toBeTypeOf("object");
    expect(presentation.translationKey).toBeUndefined();
  });

  it("builds contiguous interactive donut segments and omits zero-byte slices", () => {
    const segments = buildStorageDonutSegments([
      slice("english", 0.75),
      slice("library", 0.25),
      { ...slice("skills", 0), share: 0 },
    ]);

    expect(segments).toHaveLength(2);
    expect(segments[0].offset).toBe(0);
    expect(segments[1].offset).toBe(0.75);
  });
});

describe("donutSegmentDash", () => {
  it("carves a gap out of segments wide enough to spare it", () => {
    const [visible] = donutSegmentDash(0.5).split(" ").map(Number);
    expect(visible).toBeLessThan(0.5);
    expect(visible).toBeGreaterThan(0.49);
  });

  it("keeps a sliver visible instead of letting the gap eat it", () => {
    const [visible] = donutSegmentDash(0.002).split(" ").map(Number);
    expect(visible).toBe(0.002);
  });

  it("never emits a negative dash remainder", () => {
    const [, remainder] = donutSegmentDash(1).split(" ").map(Number);
    expect(remainder).toBeGreaterThanOrEqual(0);
  });
});

describe("buildStorageDonutLabels", () => {
  it("labels only the segments big enough to read", () => {
    const segments = buildStorageDonutSegments([
      slice("english", 0.9),
      slice("library", 0.08),
      slice("skills", 0.02),
    ]);

    expect(buildStorageDonutLabels(segments).map((label) => label.id)).toEqual([
      "english",
      "library",
    ]);
  });

  it("anchors text outward so it never runs back over the ring", () => {
    // 两段各半：第一段中点在 3 点方向（右半圆），第二段在 9 点方向（左半圆）。
    const segments = buildStorageDonutSegments([slice("a", 0.5), slice("b", 0.5)]);
    const [right, left] = buildStorageDonutLabels(segments);

    expect(right.anchor).toBe("start");
    expect(left.anchor).toBe("end");
    expect(right.x).toBeGreaterThan(left.x);
  });

  it("puts the leader line between the ring edge and the text", () => {
    const segments = buildStorageDonutSegments([slice("only", 1)]);
    const [label] = buildStorageDonutLabels(segments);
    const center = { x: 144, y: 106 };
    const distance = (x: number, y: number) => Math.hypot(x - center.x, y - center.y);

    // 起点贴环外沿（radius 62 + band/2 = 76），终点在标注半径 86 上。
    expect(distance(label.line.x1, label.line.y1)).toBeCloseTo(76, 5);
    expect(distance(label.line.x2, label.line.y2)).toBeCloseTo(86, 5);
  });

  it("carries the share so the label needs no second lookup", () => {
    const segments = buildStorageDonutSegments([slice("english", 0.42), slice("library", 0.58)]);
    expect(buildStorageDonutLabels(segments).map((label) => label.share)).toEqual([0.42, 0.58]);
  });
});
