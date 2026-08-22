import { describe, expect, it } from "vitest";
import { localNoteSourceExtensions } from "./localNoteSource";

describe("localNoteSource", () => {
  it("exposes the requested document and image formats", () => {
    const extensions = localNoteSourceExtensions();
    for (const extension of ["md", "txt", "docx", "pdf", "png", "jpg", "webp", "gif"]) {
      expect(extensions).toContain(extension);
    }
  });
});
