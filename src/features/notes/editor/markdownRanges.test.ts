import { describe, expect, it } from "vitest";
import { canonicalMarkdown, escapeTableCell, minimalTextChange, noteContentWithinLimit, noteMarkdownParser, noteOutline, serializeTable, tableAt, tableCells, utf16RangeToUtf8 } from "./markdownRanges";
import { revisionHash } from "../utils/notesWorkspace";
import type { LibraryNote } from "../../library/types";

describe("note Markdown source contract", () => {
  it("keeps whitespace and converts only BOM and line endings", () => {
    expect(canonicalMarkdown("\uFEFF  x  \r\n\r\n")).toBe("  x  \n\n");
  });
  it("maps Unicode boundaries without splitting surrogate pairs", () => {
    expect(utf16RangeToUtf8("中😀a", 1, 3)).toEqual({ byteStart: 3, byteEnd: 7 });
    expect(() => utf16RangeToUtf8("中😀a", 2, 3)).toThrow();
  });
  it("uses scalar and UTF8 limits, not UTF16 length", () => {
    expect(noteContentWithinLimit("😀".repeat(500000))).toBe(true);
    expect(noteContentWithinLimit("a".repeat(500001))).toBe(false);
  });
  it("includes all ATX and setext headings with unique source identities", () => {
    const text = "Title\n===\n\n```md\n# hidden\n```\n\n" + "# Same\n\n".repeat(100);
    const result = noteOutline(text, "test");
    expect(result).toHaveLength(101);
    expect(new Set(result.map((item) => item.id)).size).toBe(101);
  });
  it("hashes equal-length replacements differently", () => {
    const first = { content: "abc", updatedAt: 1 } as LibraryNote;
    const next = { content: "abd", updatedAt: 1 } as LibraryNote;
    expect(revisionHash(first)).toHaveLength(64);
    expect(revisionHash(first)).not.toBe(revisionHash(next));
  });
  it("preserves table escapes and never rewrites surrounding text", () => {
    const table = "| A | B |\n| :--- | ---: |\n| a\\|b | `x` |";
    const content = `Before\n\n${table}\n\nAfter`;
    const node = tableAt(content, 12)!;
    const rows = tableCells(content, node);
    expect(rows[1][0]).toBe("a\\|b");
    const next = serializeTable(rows, node.align);
    expect(next).toContain("a\\|b");
    const merged = content.slice(0, node.position!.start.offset) + next + content.slice(node.position!.end.offset);
    expect(merged.startsWith("Before\n\n")).toBe(true);
    expect(merged.endsWith("\n\nAfter")).toBe(true);
  });
  it("keeps an escaped pipe at the end of a cell", () => {
    const source = "| A | B |\n| --- | --- |\n| a\\| |  |";
    expect(tableCells(source, tableAt(source, 0)!)[1]).toEqual(["a\\|", ""]);
  });
  it("escapes consecutive pipes and even backslashes without changing existing escapes", () => {
    expect(escapeTableCell("||")).toBe("\\|\\|");
    expect(escapeTableCell("a\\|b")).toBe("a\\|b");
    expect(escapeTableCell("a\\\\|b")).toBe("a\\\\\\|b");
    expect(serializeTable([["A", "B"]], [null, "right"])).toBe("| A | B |\n| --- | ---: |");
  });
  it("makes minimal changes at whole Unicode boundaries", () => {
    expect(minimalTextChange("a😀z", "a😁z")).toEqual({ from: 1, to: 3, insert: "😁" });
  });
  it("recognizes math, highlights and allowed inline styles outside code", () => {
    const tree = noteMarkdownParser.parse("==important== <u>under</u> $x^2$ and $5 and $10 and `==code==`").toString();
    expect(tree).toContain("NoteHighlight"); expect(tree).toContain("Noteu");
    expect(tree.match(/NoteInlineMath/g)).toHaveLength(1);
  });
});
