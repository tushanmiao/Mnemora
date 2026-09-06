// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { EditorState, Transaction, type TransactionSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";
import { history, undo } from "@codemirror/commands";
import { GFM } from "@lezer/markdown";
import { formatCommand, moveSection, setHeading } from "./commands";
import { htmlClipboardToMarkdown } from "./clipboard";
const views: EditorView[] = [];
function editor(doc: string, from = 0, to = from, readOnly = false) {
  const view = { state: EditorState.create({ doc, selection: { anchor: from, head: to }, extensions: [markdown({ extensions: GFM }), history(), EditorState.readOnly.of(readOnly)] }),
    dispatch: (...specs: (TransactionSpec | Transaction)[]) => { Object.assign(view, { state: specs[0] instanceof Transaction ? specs[0].state : view.state.update(...specs).state }); }, focus() {}, destroy() {},
  } as EditorView;
  views.push(view); return view;
}
afterEach(() => { views.splice(0).forEach((view) => view.destroy()); });
describe("Markdown transactions", () => {
  it("toggles emphasis in a single undo step", () => {
    const view = editor("Before text After", 7, 11);
    expect(formatCommand("bold")(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("Before **text** After");
    undo(view); expect(view.state.doc.toString()).toBe("Before text After");
  });
  it("chooses an inline delimiter longer than the code", () => {
    const view = editor("a`b", 0, 3); formatCommand("inlineCode")(view);
    expect(view.state.doc.toString()).toBe("``a`b``");
  });
  it("rejects formatting inside fences and read-only documents", () => {
    expect(formatCommand("bold")(editor("```js\nconst x=1\n```", 10))).toBe(false);
    expect(formatCommand("bold")(editor("read", 0, 4, true))).toBe(false);
  });
  it("converts setext only on explicit heading action", () => {
    const view = editor("Title\n===\n\nNext", 2); setHeading(2)(view);
    expect(view.state.doc.toString()).toBe("## Title\n\nNext");
  });
  it("keeps a thematic break following a heading", () => {
    const view = editor("# Title\n---\n\nNext", 2); setHeading(2)(view);
    expect(view.state.doc.toString()).toBe("## Title\n---\n\nNext");
  });
  it("rejects a format range that crosses code even if its caret ends in prose", () => {
    expect(formatCommand("bold")(editor("prose\n\n```js\nx\n```\n\nend", 0, 23))).toBe(false);
  });
  it("removes existing inline-code delimiters", () => {
    const view = editor("a `code` b", 5); expect(formatCommand("inlineCode")(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("a code b");
  });
  it("moves child sections with their parent", () => {
    const view = editor("# A\n\na\n\n## Child\n\nc\n\n# B\n\nb\n", 2);
    expect(moveSection(1)(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("# B\n\nb\n# A\n\na\n\n## Child\n\nc\n\n");
  });
  it("converts structured HTML without scripts or executable links", () => {
    const markdown = htmlClipboardToMarkdown('<h2>Title</h2><script>alert(1)</script><p><a href="javascript:alert(1)">Link</a></p><table><tr><td colspan="2">Merged</td></tr></table>');
    expect(markdown).toContain("## Title"); expect(markdown).toContain('colspan="2"');
    expect(markdown).not.toContain("javascript:"); expect(markdown).not.toContain("script");
  });
});
