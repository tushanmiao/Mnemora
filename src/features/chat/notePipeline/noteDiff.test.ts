import { describe, expect, it } from "vitest";
import { applySelectedNoteHunks, buildNoteDiff } from "./noteDiff";

describe("note patch diff", () => {
  it("splits independent line changes into selectable hunks", () => {
    const oldText = "# title\n\nkeep\n\nold\n\nend\n";
    const newText = "# title 2\n\nkeep\n\nnew\n\nend\n";
    const result = buildNoteDiff(oldText, newText);
    expect(result.hunks).toHaveLength(2);
    expect(result.hunks[0].oldText).toContain("# title");
    expect(result.hunks[1].newText).toContain("new");
  });

  it("applies only selected changes and keeps rejected changes intact", () => {
    const oldText = "a\nb\nc\nd\n";
    const newText = "a\nB\nc\nD\n";
    const diff = buildNoteDiff(oldText, newText);
    expect(applySelectedNoteHunks(oldText, newText, new Set([0]))).toBe("a\nB\nc\nd\n");
    expect(applySelectedNoteHunks(oldText, newText, new Set())).toBe(oldText);
    expect(applySelectedNoteHunks(oldText, newText, new Set(diff.hunks.map((hunk) => hunk.id)))).toBe(newText);
  });
});
