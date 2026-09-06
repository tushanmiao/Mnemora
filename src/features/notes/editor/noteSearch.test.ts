import { describe, expect, it } from "vitest";
import { safeNoteSearchPattern } from "./noteSearch";

describe("bounded note search expressions", () => {
  it("accepts character classes and anchors without backtracking branches", () => {
    for (const value of ["[a-z]", "^中", "\\d\\d", "\\*", "^", "$", "a.b"]) expect(safeNoteSearchPattern(value)).toBe(true);
  });
  it("rejects catastrophic, malformed and unbounded expressions", () => {
    for (const value of ["(a+)+$", "a*a*", "(a|aa)", "a{1000}", "[abc", "\\1", "a".repeat(129)]) expect(safeNoteSearchPattern(value)).toBe(false);
  });
});
