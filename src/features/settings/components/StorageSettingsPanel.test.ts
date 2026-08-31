import { describe, expect, it } from "vitest";
import { getStorageCategoryPresentation } from "./StorageSettingsPanel";

describe("getStorageCategoryPresentation", () => {
  it("renders the prompt-library storage category", () => {
    const presentation = getStorageCategoryPresentation("prompts");
    expect(presentation.Icon).toBeTypeOf("object");
    expect(presentation.color).toBe("var(--status-warning)");
    expect(presentation.translationKey).toBe("storage.category.prompts");
  });

  it("falls back safely for a category added by a newer backend", () => {
    const presentation = getStorageCategoryPresentation("future-category");
    expect(presentation.Icon).toBeTypeOf("object");
    expect(presentation.color).toBe("var(--color-text-secondary)");
    expect(presentation.translationKey).toBeUndefined();
  });
});
