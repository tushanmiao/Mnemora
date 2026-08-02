import { describe, expect, it } from "vitest";
import {
  appendNoteReference,
  createNoteReference,
  formatNoteReferencesForModel,
  MAX_NOTE_REFERENCES_PER_MESSAGE,
} from "./noteReferences";

function input(index = 1) {
  return {
    noteId: `note-${index}`,
    noteTitle: `笔记 ${index}`,
    revisionHash: `revision-${index}`,
    startLine: index,
    endLine: index + 1,
    selectedText: `引用内容 ${index}`,
  };
}

describe("note references", () => {
  it("creates structured references and formats model context", () => {
    const reference = createNoteReference(input());
    expect(reference).not.toBeNull();
    expect(formatNoteReferencesForModel([reference!])).toContain("笔记 1");
    expect(formatNoteReferencesForModel([reference!])).toContain("第 1-2 行");
  });

  it("deduplicates and enforces the per-message limit", () => {
    const first = appendNoteReference([], input());
    expect(first.added).toBe(true);
    expect(appendNoteReference(first.references, input()).added).toBe(false);

    const full = Array.from({ length: MAX_NOTE_REFERENCES_PER_MESSAGE }, (_, index) => (
      createNoteReference(input(index + 1))!
    ));
    expect(appendNoteReference(full, input(20)).added).toBe(false);
  });
});
