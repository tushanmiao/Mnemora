import { describe, expect, it } from "vitest";

import { safeMarkdownImageUrlTransform } from "../../chat/utils/htmlSecurity";
import { createSafeNoteMarkdownUrlTransform } from "./noteMarkdownUrls";

describe("note Markdown URL security", () => {
  const transform = createSafeNoteMarkdownUrlTransform(
    "http://asset.localhost/C%3A/AppData/library/notes/note-1",
  );

  it("resolves relative images inside the note directory", () => {
    expect(transform("attachments/chart one.png", "src")).toBe(
      "http://asset.localhost/C%3A/AppData/library/notes/note-1/attachments/chart%20one.png",
    );
  });

  it("rejects traversal and absolute local paths", () => {
    expect(transform("../other/secret.png", "src")).toBe("");
    expect(transform("C:\\secret.png", "src")).toBe("");
    expect(transform("//example.com/secret.png", "src")).toBe("");
  });

  it("does not widen the shared chat transform", () => {
    expect(safeMarkdownImageUrlTransform("attachments/chart.png")).toBe("");
  });
});
