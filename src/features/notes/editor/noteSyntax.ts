import type { MarkdownConfig, DelimiterType } from "@lezer/markdown";

const highlight: DelimiterType = { resolve: "NoteHighlight", mark: "NoteStyleMark" };
const tags = new Map<string, DelimiterType>(["u", "sup", "sub"].map((tag) => [tag, { resolve: `Note${tag}`, mark: "NoteStyleMark" }]));

export function isCurrencyMath(value: string, next: string) {
  return /^\s*\d/.test(value) && /\d/.test(next) && /\s$/.test(value);
}

export const noteSyntax: MarkdownConfig = {
  defineNodes: ["NoteHighlight", "NoteStyleMark", "Noteu", "Notesup", "Notesub", "NoteInlineMath", "NoteMathMark"],
  parseInline: [
    { name: "NoteHighlight", before: "Emphasis", parse(context, next, pos) {
      if (next !== 61 || context.char(pos + 1) !== 61 || context.char(pos + 2) === 61) return -1;
      return context.addDelimiter(highlight, pos, pos + 2, !/\s/.test(context.slice(pos + 2, pos + 3)), !/\s/.test(context.slice(pos - 1, pos)));
    } },
    { name: "NoteTags", before: "HTMLTag", parse(context, next, pos) {
      if (next !== 60) return -1;
      const match = /^<(\/)?(u|sup|sub)>/.exec(context.slice(pos, Math.min(context.end, pos + 6)));
      if (!match) return -1;
      return context.addDelimiter(tags.get(match[2])!, pos, pos + match[0].length, !match[1], !!match[1]);
    } },
    { name: "NoteMath", before: "Escape", parse(context, next, pos) {
      if (next !== 36) return -1;
      let length = 1;
      while (context.char(pos + length) === 36) length++;
      const end = Math.min(context.end, pos + 8000);
      for (let cursor = pos + length; cursor < end; cursor++) {
        if (context.char(cursor) === 10) return -1;
        if (context.char(cursor) === 92) { cursor++; continue; }
        if (context.char(cursor) !== 36) continue;
        let close = 1; while (context.char(cursor + close) === 36) close++;
        if (close !== length) { cursor += close - 1; continue; }
        const value = context.slice(pos + length, cursor);
        if (!value.trim() || isCurrencyMath(value, context.slice(cursor + close, cursor + close + 1))) return -1;
        return context.addElement(context.elt("NoteInlineMath", pos, cursor + close, [context.elt("NoteMathMark", pos, pos + length), context.elt("NoteMathMark", cursor, cursor + close)]));
      }
      return -1;
    } },
  ],
};
