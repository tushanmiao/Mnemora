import { EditorSelection, Transaction, type EditorState } from "@codemirror/state";
import { type Command, type EditorView } from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { isolateHistory } from "@codemirror/commands";
import { noteOutline } from "./markdownRanges";

export type FormatId = "bold" | "italic" | "strike" | "inlineCode" | "highlight" | "underline" | "sup" | "sub" | "bullet" | "ordered" | "task" | "quote" | "link" | "code" | "math" | "mermaid" | "table" | "rule" | "footnote" | "callout" | "hardBreak";
const wrappers: Partial<Record<FormatId, [string, string, string?]>> = {
  bold: ["**", "**", "StrongEmphasis"], italic: ["*", "*", "Emphasis"], strike: ["~~", "~~", "Strikethrough"],
  highlight: ["==", "==", "NoteHighlight"], underline: ["<u>", "</u>", "Noteu"], sup: ["<sup>", "</sup>", "Notesup"], sub: ["<sub>", "</sub>", "Notesub"],
};
export function inProtectedCode(state: EditorState) {
  let node = syntaxTree(state).resolveInner(state.selection.main.head, -1);
  while (node.parent) {
    if (["FencedCode", "CodeBlock", "InlineCode", "NoteInlineMath"].includes(node.name)) return true;
    node = node.parent;
  }
  return false;
}
export function canFormat(state: EditorState) {
  if (state.readOnly || state.selection.ranges.length !== 1 || inProtectedCode(state)) return false;
  const range = state.selection.main;
  let protectedRange = false;
  if (!range.empty) syntaxTree(state).iterate({ from: range.from, to: range.to, enter(node) {
    if (["FencedCode", "CodeBlock", "InlineCode", "NoteInlineMath"].includes(node.name) && node.from < range.to && node.to > range.from) protectedRange = true;
  } });
  return !protectedRange;
}
export function formatActive(state: EditorState, id: FormatId) {
  const name = id === "inlineCode" ? "InlineCode" : wrappers[id]?.[2];
  let node = syntaxTree(state).resolveInner(state.selection.main.head, -1);
  while (node.parent) { if (node.name === name) return true; node = node.parent; }
  return false;
}
export function selectedHeadingLevel(state: EditorState): number | "mixed" {
  const { from, to, empty } = state.selection.main;
  const start = state.doc.lineAt(from).number, end = state.doc.lineAt(empty ? to : to - 1).number;
  const levels = new Set<number>();
  for (let line = start; line <= end; line++) {
    let node = syntaxTree(state).resolveInner(state.doc.line(line).from + Math.min(1, state.doc.line(line).length), 1);
    while (node.parent && !/^(ATX|Setext)Heading/.test(node.name)) node = node.parent;
    levels.add(Number(/Heading([1-6])$/.exec(node.name)?.[1] ?? 0));
  }
  return levels.size === 1 ? [...levels][0] : "mixed";
}
export function replaceRange(view: EditorView, from: number, to: number, insert: string, anchor = from + insert.length, head = anchor) {
  if (view.state.readOnly) return false;
  view.dispatch({ changes: { from, to, insert }, selection: EditorSelection.single(anchor, head), scrollIntoView: true,
    annotations: [Transaction.userEvent.of("input.format"), isolateHistory.of("full")] });
  view.focus();
  return true;
}
export function setHeading(level: number): Command {
  return (view) => {
    if (!canFormat(view.state) || !Number.isInteger(level) || level < 0 || level > 6) return false;
    const range = view.state.selection.main;
    const start = view.state.doc.lineAt(range.from), end = view.state.doc.lineAt(range.empty ? range.to : range.to - 1);
    const text = view.state.sliceDoc(start.from, end.to);
    let to = end.to;
    const next = end.number < view.state.doc.lines ? view.state.doc.line(end.number + 1) : null;
    let node = syntaxTree(view.state).resolveInner(range.from, 1);
    while (node.parent && !/^SetextHeading/.test(node.name)) node = node.parent;
    if (next && /^SetextHeading/.test(node.name) && node.to === next.to) to = next.to;
    const insert = text.split("\n").map((line) => line.replace(/^( {0,3})(?:#{1,6}\s+)?/, `$1${level ? `${"#".repeat(level)} ` : ""}`)).join("\n");
    return replaceRange(view, start.from, to, insert);
  };
}
export function formatCommand(id: FormatId, argument?: string): Command {
  return (view) => {
    if (id === "inlineCode" && !view.state.readOnly && view.state.selection.ranges.length === 1) {
      let node = syntaxTree(view.state).resolveInner(view.state.selection.main.head, -1);
      while (node.parent && node.name !== "InlineCode") node = node.parent;
      if (node.name === "InlineCode") {
        const marks = node.getChildren("CodeMark");
        if (marks.length === 2) return replaceRange(view, node.from, node.to, view.state.sliceDoc(marks[0].to, marks[1].from));
      }
    }
    if (!canFormat(view.state)) return false;
    const { state } = view;
    const { from, to } = state.selection.main;
    const selected = state.sliceDoc(from, to);
    const wrapper = wrappers[id];
    if (wrapper) {
      const [open, close, nodeName] = wrapper;
      let node = syntaxTree(state).resolveInner(from, 1);
      while (node.parent && node.name !== nodeName) node = node.parent;
      if (node.name === nodeName && node.from <= from && node.to >= to) {
        const inner = state.sliceDoc(node.from + open.length, node.to - close.length);
        return replaceRange(view, node.from, node.to, inner, node.from, node.from + inner.length);
      }
      if (state.sliceDoc(Math.max(0, from - open.length), from) === open && state.sliceDoc(to, to + close.length) === close) {
        return replaceRange(view, from - open.length, to + close.length, selected, from - open.length, to - open.length);
      }
      if (selected.includes("\n\n")) return false;
      return replaceRange(view, from, to, `${open}${selected}${close}`, from + open.length, to + open.length);
    }
    if (id === "inlineCode") {
      const length = Math.max(0, ...[...selected.matchAll(/`+/g)].map((match) => match[0].length)) + 1;
      const fence = "`".repeat(length), padding = /^`|`$|^ .* $/.test(selected) ? " " : "";
      return replaceRange(view, from, to, `${fence}${padding}${selected}${padding}${fence}`, from + length + padding.length, to + length + padding.length);
    }
    if (["bullet", "ordered", "task", "quote"].includes(id)) {
      const first = state.doc.lineAt(from), last = state.doc.lineAt(to === from ? to : to - 1);
      const lines = state.sliceDoc(first.from, last.to).split("\n");
      const pattern = id === "quote" ? /^(\s*)> ?/ : /^(\s*)(?:[-+*]|\d+[.)])\s+(?:\[[ xX]\]\s+)?/;
      const targetPattern = id === "quote" ? /^(\s*)> ?/ : id === "ordered" ? /^(\s*)\d+[.)]\s+/ : id === "task" ? /^(\s*)[-+*]\s+\[[ xX]\]\s+/ : /^(\s*)[-+*]\s+(?!\[[ xX]\])/;
      const all = lines.every((line) => targetPattern.test(line));
      const numbers = new Map<number, number>();
      const insert = lines.map((line) => {
        if (all) return line.replace(pattern, "$1");
        const indent = /^\s*/.exec(line)![0].length;
        const originalNumber = /^\s*(\d+)[.)]\s+/.exec(line)?.[1];
        const number = originalNumber ? Number(originalNumber) : (numbers.get(indent) ?? 0) + 1;
        numbers.set(indent, number);
        const checked = /^\s*[-+*]\s+\[([xX ])\]/.exec(line)?.[1] ?? " ";
        const prefix = id === "quote" ? "> " : id === "ordered" ? `${number}. ` : id === "task" ? `- [${checked}] ` : "- ";
        const plain = id === "quote" ? line : line.replace(pattern, "$1");
        return plain.replace(/^(\s*)/, `$1${prefix}`);
      }).join("\n");
      return replaceRange(view, first.from, last.to, insert);
    }
    if (id === "link") {
      const target = argument ?? "https://";
      if (!/^(https?:\/\/|mailto:|#|attachments\/)/i.test(target)) return false;
      const insert = `[${selected || "链接"}](<${target.replace(/[<>\r\n]/g, "")}>)`;
      return replaceRange(view, from, to, insert, from + 1, from + 1 + (selected || "链接").length);
    }
    if (id === "hardBreak") return replaceRange(view, from, to, "  \n");
    if (id === "footnote") {
      let index = 1;
      while (state.doc.toString().includes(`[^${index}]`)) index++;
      const ref = `[^${index}]`;
      view.dispatch({ changes: [{ from, to, insert: ref }, { from: state.doc.length, insert: `\n\n${ref}: ${selected}\n` }],
        annotations: isolateHistory.of("full") });
      view.focus(); return true;
    }
    const fence = "`".repeat(Math.max(3, ...[...selected.matchAll(/`+/g)].map((match) => match[0].length + 1)));
    let table = "| 项目 | 内容 |\n| --- | --- |\n|  |  |";
    if (id === "table" && argument) {
      const [rows, columns] = argument.split("x").map(Number);
      if (![rows, columns].every(Number.isInteger) || rows < 1 || columns < 1 || rows > 200 || columns > 30 || rows * columns > 2000) return false;
      const row = `| ${Array<string>(columns).fill("").join(" | ")} |`;
      table = [row, `| ${Array<string>(columns).fill("---").join(" | ")} |`, ...Array<string>(rows - 1).fill(row)].join("\n");
    }
    const templates: Partial<Record<FormatId, string>> = {
      code: `${fence}${argument ?? "text"}\n${selected}\n${fence}`,
      math: `$$\n${selected || "E = mc^2"}\n$$`,
      mermaid: `${fence}mermaid\n${argument || selected || "flowchart LR\n  A[问题] --> B[证据] --> C[结论]"}\n${fence}`,
      table, rule: "---", callout: `> [!NOTE]\n> ${selected}`,
    };
    const insert = templates[id];
    return insert === undefined ? false : replaceRange(view, from, to, `\n\n${insert}\n\n`);
  };
}
export function moveSection(direction: -1 | 1): Command {
  return (view) => {
    if (view.state.readOnly) return false;
    const content = view.state.doc.toString(), head = view.state.selection.main.head;
    const outline = noteOutline(content, "move");
    const current = [...outline].reverse().find((heading) => heading.offset <= head);
    if (!current) return false;
    const siblings = outline.filter((heading) => heading.level <= current.level);
    const index = siblings.indexOf(current), other = siblings[index + direction];
    if (!other || other.level !== current.level) return false;
    const start = direction < 0 ? other.offset : current.offset;
    const middle = direction < 0 ? current.offset : other.offset;
    const end = siblings[index + (direction < 0 ? 1 : 2)]?.offset ?? content.length;
    const left = content.slice(start, middle), right = content.slice(middle, end);
    return replaceRange(view, start, end, `${right}${right.endsWith("\n") ? "" : "\n"}${left}`, direction < 0 ? start : start + right.length);
  };
}
