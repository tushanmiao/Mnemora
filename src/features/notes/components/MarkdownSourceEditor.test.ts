import { describe, expect, it } from "vitest";
import { MARKDOWN_RICH_EDITOR_MAX_CHARS, shouldUsePlainTextNoteEditor } from "./markdownEditorPolicy";

describe("MarkdownSourceEditor", () => {
  it("keeps CodeMirror for ordinary notes", () => {
    expect(shouldUsePlainTextNoteEditor(MARKDOWN_RICH_EDITOR_MAX_CHARS)).toBe(false);
  });

  it("uses the resilient native editor for large generated notes", () => {
    expect(shouldUsePlainTextNoteEditor(MARKDOWN_RICH_EDITOR_MAX_CHARS + 1)).toBe(true);
  });

  it("falls back when the rich editor cannot produce a visible paint surface", () => {
    expect(shouldUsePlainTextNoteEditor(128, true)).toBe(true);
  });
});
