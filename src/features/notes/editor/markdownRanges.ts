import { parser, GFM } from "@lezer/markdown";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkFrontmatter from "remark-frontmatter";
import type { Root, RootContent, Table, TableCell } from "mdast";
import { headingId, type MarkdownOutlineItem } from "../../chat/markdown/utils/outline";
import { noteSyntax } from "./noteSyntax";

export const noteMarkdownParser = parser.configure([GFM, noteSyntax]);
const processor = unified().use(remarkParse).use(remarkGfm).use(remarkMath).use(remarkFrontmatter);
export function parseNoteMarkdown(content: string): Root { return processor.parse(content); }
export function plainNodeText(node: { type?: string; value?: string; children?: unknown[] }): string {
  return node.value ?? (node.children?.map((child) => plainNodeText(child as typeof node)).join("") ?? "");
}
export function noteOutline(content: string, scope: string): MarkdownOutlineItem[] {
  const result: MarkdownOutlineItem[] = [];
  const titles = new Map<string, string>();
  noteMarkdownParser.parse(content).iterate({ enter(node) {
    if (node.name === "FencedCode" || node.name === "CodeBlock") return false;
    const heading = /^(?:ATX|Setext)Heading([1-6])$/.exec(node.name);
    if (!heading) return;
    const raw = content.slice(node.from, node.to);
    let title = titles.get(raw);
    if (title === undefined) {
      const parsed = parseNoteMarkdown(raw).children[0];
      title = parsed ? plainNodeText(parsed) : raw;
      titles.set(raw, title);
    }
    result.push({ id: headingId(scope, node.from), level: Number(heading[1]), title, offset: node.from });
    return false;
  }});
  return result;
}
export function nodeAt(content: string, position: number, type?: RootContent["type"]) {
  return parseNoteMarkdown(content).children.find((node) => (!type || node.type === type)
    && node.position && node.position.start.offset! <= position && node.position.end.offset! >= position);
}
export function tableAt(content: string, position: number) {
  return nodeAt(content, position, "table") as Table | undefined;
}
export function utf16RangeToUtf8(content: string, from: number, to: number) {
  const valid = (offset: number) => Number.isInteger(offset) && offset >= 0 && offset <= content.length
    && !(offset > 0 && offset < content.length && /[\uD800-\uDBFF]/.test(content[offset - 1]) && /[\uDC00-\uDFFF]/.test(content[offset]));
  if (!valid(from) || !valid(to) || from > to) throw new Error("NOTE_RANGE_STALE");
  const encoder = new TextEncoder();
  const start = encoder.encode(content.slice(0, from)).length;
  return { byteStart: start, byteEnd: start + encoder.encode(content.slice(from, to)).length };
}
export function noteContentWithinLimit(content: string) {
  return content.length <= 1_000_000 && new TextEncoder().encode(content).length <= 2 * 1024 * 1024
    && (content.length <= 500_000 || Array.from(content).length <= 500_000);
}
export function canonicalMarkdown(content: string) { return content.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n"); }

export function serializeTable(rows: string[][], alignment: Table["align"]) {
  if (!rows.length || !rows[0].length) throw new Error("NOTE_CONTENT_INVALID: Empty table");
  const row = (cells: string[]) => `| ${cells.map(escapeTableCell).join(" | ")} |`;
  return [row(rows[0]), row(rows[0].map((_, index) => {
    const align = alignment?.[index];
    return align ? ({ left: ":---", center: ":---:", right: "---:" })[align] : "---";
  })), ...rows.slice(1).map(row)].join("\n");
}
export function escapeTableCell(value: string) {
  let slashes = 0, result = "";
  for (const character of value.replace(/\r\n?|\n/g, "<br>")) {
    if (character === "|" && slashes % 2 === 0) result += "\\";
    result += character;
    slashes = character === "\\" ? slashes + 1 : 0;
  }
  return result;
}

/** Smallest UTF-16 change, never splitting a surrogate pair. */
export function minimalTextChange(before: string, after: string) {
  let from = 0, end = before.length, nextEnd = after.length;
  while (from < end && from < nextEnd && before[from] === after[from]) from++;
  if (from > 0 && /[\uD800-\uDBFF]/.test(before[from - 1])) from--;
  while (end > from && nextEnd > from && before[end - 1] === after[nextEnd - 1]) { end--; nextEnd--; }
  if (end < before.length && /[\uDC00-\uDFFF]/.test(before[end])) { end++; nextEnd++; }
  return { from, to: end, insert: after.slice(from, nextEnd) };
}
export function tableCells(content: string, table: Table) {
  return table.children.map((row) => row.children.map((cell) => {
    const range = tableCellRange(content, cell);
    return content.slice(range.from, range.to);
  }));
}
export function tableCellRange(content: string, cell: TableCell) {
  const first = cell.children[0]?.position?.start.offset;
  const last = cell.children[cell.children.length - 1]?.position?.end.offset;
  if (first !== undefined && last !== undefined) return { from: first, to: last };
  let from = cell.position!.start.offset!;
  if (content[from] === "|") from++;
  while (from < cell.position!.end.offset! && /[ \t]/.test(content[from])) from++;
  return { from, to: from };
}
