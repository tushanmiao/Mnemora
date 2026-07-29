import { describe, expect, it } from "vitest";
import {
  appendLiteratureReference,
  createLiteratureReference,
  formatLiteratureReferencesForModel,
  MAX_LITERATURE_REFERENCE_TEXT_BYTES,
  normalizeLinkedLibraryItemIds,
} from "./literatureReferences";

function reference(index = 1) {
  return {
    id: `reference-${index}`,
    libraryItemId: `item-${index}`,
    title: `Paper ${index}`,
    pageIndex: index - 1,
    kind: "selection" as const,
    text: `Evidence ${index}`,
  };
}

describe("literature references", () => {
  it("limits linked documents and removes duplicate or invalid IDs", () => {
    const ids = Array.from({ length: 14 }, (_, index) => `item-${index}`);
    expect(normalizeLinkedLibraryItemIds([ids[0], ids[0], "bad id", ...ids.slice(1)]))
      .toEqual(ids.slice(0, 12));
  });

  it("truncates one reference to the UTF-8 byte budget", () => {
    const result = createLiteratureReference({
      ...reference(),
      text: "文".repeat(20_000),
    });
    expect(result).not.toBeNull();
    expect(new TextEncoder().encode(result?.text).byteLength)
      .toBeLessThanOrEqual(MAX_LITERATURE_REFERENCE_TEXT_BYTES);
  });

  it("deduplicates a pending reference and formats source instructions", () => {
    const first = appendLiteratureReference([], reference());
    const duplicate = appendLiteratureReference(first.references, {
      ...reference(),
      id: "another-reference-id",
    });
    expect(first.added).toBe(true);
    expect(duplicate.added).toBe(false);
    expect(formatLiteratureReferencesForModel(first.references)).toContain("【文献标题，第 N 页】");
    expect(formatLiteratureReferencesForModel(first.references)).toContain("Paper 1");
    expect(formatLiteratureReferencesForModel(first.references)).toContain("第 1 页");
  });
});
