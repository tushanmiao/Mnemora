import { StateEffect, StateField, type EditorState, type Range } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { createRoot, type Root as ReactRoot } from "react-dom/client";
import { minimalTextChange, parseNoteMarkdown } from "./markdownRanges";
import { NoteTableEditor } from "./NoteTableEditor";
import { NoteBlockEditor } from "./NoteBlockEditor";
import { ImageViewerProvider } from "../../chat/image-viewer/ImageViewerContext";
import { undo, redo, isolateHistory } from "@codemirror/commands";
import { getNoteEditSession } from "../runtime/noteEditSession";
import katex from "katex";
import { I18nProvider } from "../../../i18n/I18nProvider";
import type { InterfaceLanguage } from "../../../types/appSettings";
import { noteText } from "./noteText";

type Block = { id: string; from: number; to: number; source: string; type: string };
type BlockState = { blocks: Block[]; decorations: DecorationSet };
const reparseBlocks = StateEffect.define<null>();

class NoteBlockWidget extends WidgetType {
  private root: ReactRoot | null = null;
  constructor(readonly block: Block, readonly noteId: string, readonly getBlocks: (view: EditorView) => Block[], readonly directoryPath?: string | null, readonly language: InterfaceLanguage = "zh") { super(); }
  eq(other: NoteBlockWidget) { return this.block.source === other.block.source && this.block.from === other.block.from && this.block.type === other.block.type; }
  toDOM(view: EditorView) {
    const dom = document.createElement("div");
    dom.className = "note-live-block";
    dom.contentEditable = "false";
    this.root = createRoot(dom);
    const measure = new ResizeObserver(() => { if (dom.isConnected) view.requestMeasure(); });
    measure.observe(dom);
    (dom as HTMLElement & { noteMeasure?: ResizeObserver }).noteMeasure = measure;
    this.render(dom, view);
    return dom;
  }
  updateDOM(dom: HTMLElement, view: EditorView) {
    if (dom.dataset.noteBlockId !== this.block.id) return false;
    const previous = (dom as HTMLElement & { noteRoot?: ReactRoot }).noteRoot;
    if (!previous) return false;
    this.root = previous;
    this.render(dom, view);
    return true;
  }
  private render(dom: HTMLElement, view: EditorView) {
    dom.dataset.noteBlockId = this.block.id;
    dom.dataset.noteBlockFrom = String(this.block.from);
    (dom as HTMLElement & { noteRoot?: ReactRoot }).noteRoot = this.root!;
    const locate = () => {
      if (!dom.isConnected || view.state.readOnly) return null;
      const from = view.posAtDOM(dom);
      const block = this.getBlocks(view).find((candidate) => candidate.from === from);
      return block?.source === this.block.source ? block : null;
    };
    const source = () => {
      const block = locate();
      if (!block) return;
      view.dispatch({ selection: { anchor: block.from + Math.min(1, block.to - block.from) }, scrollIntoView: true }); view.focus();
    };
    const change = (text: string, structural = false) => {
      const block = locate();
      if (!block) return;
      const delta = minimalTextChange(block.source, text);
      view.dispatch({ changes: { from: block.from + delta.from, to: block.from + delta.to, insert: delta.insert },
        annotations: structural ? isolateHistory.of("full") : [], userEvent: "input.block" });
    };
    const session = getNoteEditSession(this.noteId);
    const save = () => { void session.save().catch(() => undefined); };
    this.root!.render(<I18nProvider language={this.language}><ImageViewerProvider>{this.block.type === "table" ? <NoteTableEditor source={this.block.source} onChange={change} onSource={source}
      onUndo={() => undo(view)} onRedo={() => redo(view)} onSave={save} onComposition={(active) => session.composition(active)} /> :
      <NoteBlockEditor noteId={this.noteId} source={this.block.source} directoryPath={this.directoryPath}
        onChange={change} onSource={source} onUndo={() => undo(view)} onRedo={() => redo(view)} onSave={save} />}</ImageViewerProvider></I18nProvider>);
    requestAnimationFrame(() => { if (dom.isConnected) view.requestMeasure(); });
  }
  destroy(dom: HTMLElement) { (dom as HTMLElement & { noteMeasure?: ResizeObserver }).noteMeasure?.disconnect(); const root = this.root; queueMicrotask(() => root?.unmount()); this.root = null; }
  ignoreEvent() { return true; }
}

class TaskWidget extends WidgetType {
  constructor(readonly checked: boolean, readonly language: InterfaceLanguage) { super(); }
  eq(other: TaskWidget) { return this.checked === other.checked; }
  toDOM(view: EditorView) {
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox"; checkbox.checked = this.checked; checkbox.setAttribute("aria-label", noteText(this.language, "切换任务状态"));
    checkbox.onchange = () => {
      if (view.state.readOnly) return;
      const from = view.posAtDOM(checkbox);
      if (/^\[[ xX]\]$/.test(view.state.sliceDoc(from, from + 3))) {
        view.dispatch({ changes: { from, to: from + 3, insert: this.checked ? "[ ]" : "[x]" }, userEvent: "input.task" });
      }
    };
    return checkbox;
  }
  ignoreEvent() { return true; }
}

class InlineMathWidget extends WidgetType {
  constructor(readonly source: string) { super(); }
  eq(other: InlineMathWidget) { return this.source === other.source; }
  toDOM(view: EditorView) {
    const dom = document.createElement("span"); dom.className = "note-inline-math";
    const delimiter = /^\$+/.exec(this.source)![0].length;
    try {
      dom.innerHTML = katex.renderToString(this.source.slice(delimiter, -delimiter), { displayMode: false, trust: false, maxExpand: 1000, maxSize: 20, throwOnError: false });
    } catch { dom.textContent = this.source; }
    dom.ondblclick = () => {
      if (view.state.readOnly) return;
      const pos = view.posAtDOM(dom);
      view.dispatch({ selection: { anchor: pos + delimiter } }); view.focus();
    };
    return dom;
  }
}

export function livePreview(noteId: string, directoryPath?: string | null, language: InterfaceLanguage = "zh") {
  let field: StateField<BlockState>;
  const build = (state: EditorState, cached?: Block[], previous: Block[] = []) => {
    const nodes = cached ? [] : parseNoteMarkdown(state.doc.toString()).children.map((node) =>
      node.type === "paragraph" && node.children.length === 1 && ["image", "inlineMath"].includes(node.children[0].type)
        ? { ...node.children[0], position: node.position } : node);
    const blocks = cached ?? nodes.filter((node) =>
      ["code", "math", "inlineMath", "image", "table", "html", "yaml", "thematicBreak"].includes(node.type)
      && node.position?.start.offset !== undefined && node.position.end.offset !== undefined,
    ).map((node) => ({ id: previous.find((block) => block.from === node.position!.start.offset && block.type === node.type)?.id ?? crypto.randomUUID(), from: node.position!.start.offset!, to: node.position!.end.offset!,
      source: state.sliceDoc(node.position!.start.offset!, node.position!.end.offset!), type: node.type }));
    const decorations: Range<Decoration>[] = [];
    let heavy = 0;
    for (const block of blocks) {
      const active = state.selection.ranges.some((range) => range.empty && range.head > block.from && range.head < block.to);
      if (active || block.source.length > 32000 || heavy++ >= 20) continue;
      if (block.type === "code" && /^\s*(`{3,}|~{3,})/.test(block.source)) {
        const fence = block.source.match(/^\s*(`{3,}|~{3,})/)![1];
        if (!new RegExp(`\\n[ \\t]{0,3}${fence[0]}{${fence.length},}[ \\t]*$`).test(block.source)) continue;
      }
      decorations.push(Decoration.replace({ block: true, widget: new NoteBlockWidget(block, noteId, (view) => view.state.field(field).blocks, directoryPath, language) }).range(block.from, block.to));
    }
    return { blocks, decorations: Decoration.set(decorations, true) };
  };
  field = StateField.define<BlockState>({
    create: (state) => build(state),
    update: (value, transaction) => transaction.effects.some((effect) => effect.is(reparseBlocks)) ? build(transaction.state, undefined, value.blocks)
      : transaction.docChanged ? build(transaction.state, value.blocks.map((block) => {
        const from = transaction.changes.mapPos(block.from, -1), to = transaction.changes.mapPos(block.to, 1);
        return { ...block, from, to, source: transaction.state.sliceDoc(from, to) };
      }).filter((block) => block.to > block.from))
      : transaction.selection ? build(transaction.state, value.blocks) : value,
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
  const inline = ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    timer: ReturnType<typeof setTimeout> | null = null;
    constructor(view: EditorView) { this.decorations = this.build(view); }
    update(update: ViewUpdate) {
      if (update.view.composing) this.decorations = this.decorations.map(update.changes);
      else if (update.docChanged || update.selectionSet || update.viewportChanged || syntaxTree(update.startState) !== syntaxTree(update.state)) this.decorations = this.build(update.view);
      if (update.docChanged) {
        if (this.timer) clearTimeout(this.timer);
        this.timer = setTimeout(() => {
          this.timer = null;
          if (!update.view.composing) update.view.dispatch({ effects: reparseBlocks.of(null) });
        }, 160);
      }
    }
    destroy() { if (this.timer) clearTimeout(this.timer); }
    build(view: EditorView) {
      const ranges: Range<Decoration>[] = [];
      const state = view.state;
      for (const visible of view.visibleRanges) syntaxTree(state).iterate({ from: visible.from, to: visible.to, enter(node) {
        if (["FencedCode", "CodeBlock", "Table", "HTMLBlock"].includes(node.name)) return false;
        const heading = /^ATXHeading([1-6])$|^SetextHeading([12])$/.exec(node.name);
        if (heading) ranges.push(Decoration.line({ class: `note-live-heading note-live-h${heading[1] ?? heading[2]}` }).range(state.doc.lineAt(node.from).from));
        const styles: Record<string, string> = { StrongEmphasis: "note-live-strong", Emphasis: "note-live-em", Strikethrough: "note-live-strike", InlineCode: "note-live-code", Link: "note-live-link", Blockquote: "note-live-quote", NoteHighlight: "note-live-highlight", Noteu: "note-live-underline", Notesup: "note-live-sup", Notesub: "note-live-sub" };
        if (styles[node.name]) ranges.push(Decoration.mark({ class: styles[node.name] }).range(node.from, node.to));
        if (["HeaderMark", "EmphasisMark", "StrikethroughMark", "CodeMark", "QuoteMark", "NoteStyleMark"].includes(node.name)) {
          const parent = node.node.parent;
          const active = parent && state.selection.ranges.some((range) => range.empty && range.head >= parent.from && range.head <= parent.to);
          if (!active && !view.composing && !state.sliceDoc(node.from, node.to).includes("\n")) ranges.push(Decoration.replace({}).range(node.from, node.to));
        }
        if (node.name === "TaskMarker") {
          ranges.push(Decoration.replace({ widget: new TaskWidget(state.sliceDoc(node.from, node.to).toLowerCase() === "[x]", language) }).range(node.from, node.to));
        }
        if (node.name === "NoteInlineMath") {
          const active = state.selection.ranges.some((range) => range.head >= node.from && range.head <= node.to);
          if (!active && !view.composing) ranges.push(Decoration.replace({ widget: new InlineMathWidget(state.sliceDoc(node.from, node.to)) }).range(node.from, node.to));
          return false;
        }
        if (node.name === "Link") {
          const active = state.selection.ranges.some((range) => range.empty && range.head >= node.from && range.head <= node.to);
          if (!active && !view.composing) for (const child of node.node.getChildren("LinkMark").concat(node.node.getChildren("URL"))) {
            ranges.push(Decoration.replace({}).range(child.from, child.to));
          }
        }
      }});
      return Decoration.set(ranges, true);
    }
  }, { decorations: (plugin) => plugin.decorations });
  return [field, inline];
}
