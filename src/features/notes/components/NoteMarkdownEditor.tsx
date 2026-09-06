import { useNoteText } from "../editor/noteText";
import { forwardRef, lazy, Suspense, useEffect, useImperativeHandle, useMemo, useRef, useState, type MouseEvent, type RefObject } from "react";
import { Annotation, Compartment, EditorState, StateEffect, StateField, Transaction, type EditorStateConfig } from "@codemirror/state";
import { EditorView, drawSelection, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
import { defaultKeymap, history, historyField, historyKeymap, indentWithTab, undo, redo, undoDepth, redoDepth } from "@codemirror/commands";
import { markdown, markdownKeymap } from "@codemirror/lang-markdown";
import { GFM } from "@lezer/markdown";
import { foldGutter, foldKeymap, syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
import { openSearchPanel, search, searchKeymap, gotoLine } from "@codemirror/search";
import { Bold, Italic, Strikethrough, Code, List, ListOrdered, ListTodo, Quote, Undo2, Redo2, Search, History, Save, ImagePlus, Download, Focus, WrapText, Eye, Code2, PencilLine, MoreHorizontal } from "lucide-react";
import type { MarkdownSourceEditorHandle } from "./MarkdownSourceEditor";
import { noteEditingApi, type NoteEditorMode } from "../api/noteEditing";
import { useNoteEditSession } from "../runtime/noteEditSession";
import { livePreview } from "../editor/livePreview";
import { canFormat, formatActive, formatCommand, inProtectedCode, moveSection, replaceRange, selectedHeadingLevel, setHeading, type FormatId } from "../editor/commands";
import { NoteInsertPanel } from "../editor/NoteInsertPanel";
import { canonicalMarkdown, minimalTextChange, noteContentWithinLimit } from "../editor/markdownRanges";
import { createNoteSearchPanel } from "../editor/noteSearch";
import { noteSyntax } from "../editor/noteSyntax";
import { useI18n } from "../../../i18n/I18nProvider";
import { htmlClipboardToMarkdown, imageBase64 } from "../editor/clipboard";
import { NoteHistoryPanel, downloadNoteText } from "./NoteHistoryPanel";
import { NoteRecoveryBar } from "./NoteRecoveryBar";
import { useNoteEditorPreferences } from "../runtime/noteEditorPreferences";
import "../styles/note-editor.css";

const Preview = lazy(() => import("./MarkdownNotePreview"));
const externalUpdate = Annotation.define<boolean>();
const addImageAnchor = StateEffect.define<{ id: string; from: number; to: number }>();
const dropImageAnchor = StateEffect.define<string>();
const imageAnchors = StateField.define<Map<string, { from: number; to: number }>>({
  create: () => new Map(), update(value, transaction) {
    const next = new Map([...value].map(([id, range]) => [id, { from: transaction.changes.mapPos(range.from, 1), to: transaction.changes.mapPos(range.to, 1) }]));
    for (const effect of transaction.effects) {
      if (effect.is(addImageAnchor)) next.set(effect.value.id, effect.value);
      if (effect.is(dropImageAnchor)) next.delete(effect.value);
    }
    return next;
  },
});

type Props = {
  noteId: string; value: string; directoryPath?: string | null; mode: NoteEditorMode;
  onModeChange: (mode: NoteEditorMode) => void;
  onChange: (content: string) => void; onSelectionChange?: () => void;
  onMouseUp?: (event: MouseEvent<HTMLElement>) => void;
  onPreviewMouseUp?: (event: MouseEvent<HTMLDivElement>) => void;
  previewRef?: RefObject<HTMLDivElement | null>;
};

export const NoteMarkdownEditor = forwardRef<MarkdownSourceEditorHandle, Props>(function NoteMarkdownEditor(props, ref) {
  const nt = useNoteText();
  const { language } = useI18n();
  const textRef = useRef(nt); textRef.current = nt;
  const { noteId, directoryPath, mode, value } = props;
  const preferences = useNoteEditorPreferences();
  const preferencesRef = useRef(preferences); preferencesRef.current = preferences;
  const sessionState = useNoteEditSession(noteId);
  const { session } = sessionState;
  const host = useRef<HTMLDivElement>(null), native = useRef<HTMLTextAreaElement>(null), fileInput = useRef<HTMLInputElement>(null);
  const viewRef = useRef<EditorView | null>(null), retained = useRef<EditorState | null>(null);
  const scroll = useRef(0), propsRef = useRef(props);
  propsRef.current = props;
  const [version, repaintToolbar] = useState(0), [failure, setFailure] = useState(false);
  const [readSearch, setReadSearch] = useState(false);
  const [blockFocused, setBlockFocused] = useState(false);
  const [insertKind, setInsertKind] = useState<"link" | "table" | "mermaid" | null>(null);
  const [error, setError] = useState(""), [historyOpen, setHistoryOpen] = useState(false), [assetsPending, setAssetsPending] = useState(0);
  const [focus, setFocus] = useState(preferences.focusMode), [wrap, setWrap] = useState(preferences.wordWrap);
  const preferenceCompartment = useMemo(() => new Compartment(), []);
  const liveCompartment = useMemo(() => new Compartment(), []), wrapCompartment = useMemo(() => new Compartment(), []);
  const large = useMemo(() => new TextEncoder().encode(value).length > 512 * 1024, [value]);
  const effectiveMode = mode === "live" && (large || preferences.renderPolicy === "sourceOnly") ? "source" : mode;
  const reading = effectiveMode === "read" && !readSearch;
  const readonlyCompartment = useMemo(() => new Compartment(), []);
  useEffect(() => { session?.configure(preferences.autosaveEnabled, preferences.autosaveDelayMs); setFocus(preferences.focusMode); setWrap(preferences.wordWrap); }, [session, preferences]);
  const run = (action: () => Promise<unknown>) => { void action().catch((error: unknown) => setError(String(error))); };
  const nativeTransaction = (transaction: Transaction) => {
    retained.current = transaction.state;
    propsRef.current.onChange(transaction.state.doc.toString());
    requestAnimationFrame(() => {
      const selected = transaction.state.selection.main;
      native.current?.setSelectionRange(selected.from, selected.to);
    });
  };

  const insertImages = async (files: File[]) => {
    const view = viewRef.current;
    if (!view || view.state.readOnly || !session || mode === "read") return;
    if (files.length > 10) throw new Error(nt("每次最多插入 10 张图片。"));
    const id = crypto.randomUUID(), selected = view.state.selection.main;
    const expectedText = view.state.sliceDoc(selected.from, selected.to);
    view.dispatch({ effects: addImageAnchor.of({ id, from: selected.from, to: selected.to }) });
    setAssetsPending((count) => count + 1);
    session.pendingAssets++;
    try {
      const markdown: string[] = [];
      for (const file of files) {
        const asset = await noteEditingApi.stageImage(noteId, session.sessionId, file.name, await imageBase64(file));
        markdown.push(`![${file.name.replace(/[\[\]\\\r\n]/g, "")}](${asset.relativePath})`);
      }
      if (viewRef.current !== view || view.state.readOnly) throw new Error(nt("图片已保留，编辑视图已变化，请重新插入。"));
      const range = view.state.field(imageAnchors).get(id);
      if (!range || view.state.sliceDoc(range.from, range.to) !== expectedText) throw new Error(nt("图片已保留，但插入位置已被修改，请重新插入。"));
      replaceRange(view, range.from, range.to, markdown.join("\n"));
      await session.checkpoint();
    } finally {
      if (viewRef.current === view) view.dispatch({ effects: dropImageAnchor.of(id) });
      setAssetsPending((count) => count - 1);
      session.pendingAssets--;
      void session.load();
    }
  };
  const imageInsertRef = useRef(insertImages); imageInsertRef.current = insertImages;

  useImperativeHandle(ref, () => ({
    focus: () => failure ? native.current?.focus() : viewRef.current?.focus(),
    getText: () => viewRef.current?.state.doc.toString() ?? propsRef.current.value,
    getSelection: () => {
      const view = viewRef.current;
      const focused = document.activeElement;
      if ((focused instanceof HTMLInputElement || focused instanceof HTMLTextAreaElement) && host.current?.contains(focused) && focused.dataset.noteSourceFrom !== undefined) {
        const blockFrom = Number(focused.closest<HTMLElement>("[data-note-block-from]")?.dataset.noteBlockFrom);
        const cellFrom = Number(focused.dataset.noteSourceFrom);
        const from = blockFrom + cellFrom + (focused.selectionStart ?? 0), to = blockFrom + cellFrom + (focused.selectionEnd ?? 0);
        const text = focused.value.slice(focused.selectionStart ?? 0, focused.selectionEnd ?? 0);
        if (view && view.state.sliceDoc(from, to) === text) return { from, to, text };
        // A staged/IME or escaped selection has no exact source coordinate.
        return { from: 0, to: 0, text: "" };
      }
      const selectedDom = window.getSelection();
      if (selectedDom && !selectedDom.isCollapsed && focused?.closest(".note-live-block") && !(focused instanceof HTMLInputElement || focused instanceof HTMLTextAreaElement)) return { from: 0, to: 0, text: "" };
      const from = view?.state.selection.main.from ?? native.current?.selectionStart ?? 0;
      const to = view?.state.selection.main.to ?? native.current?.selectionEnd ?? from;
      return { from, to, text: (view?.state.doc.toString() ?? propsRef.current.value).slice(from, to) };
    },
    setSelection: (from, to = from) => {
      const view = viewRef.current;
      if (view) view.dispatch({ selection: { anchor: Math.min(from, view.state.doc.length), head: Math.min(to, view.state.doc.length) }, scrollIntoView: true });
      else native.current?.setSelectionRange(from, to);
    },
    scrollToLine: (line) => {
      const view = viewRef.current;
      if (view) view.dispatch({ effects: EditorView.scrollIntoView(view.state.doc.line(Math.max(1, Math.min(line, view.state.doc.lines))).from, { y: "start" }) });
    },
    getDom: () => viewRef.current?.dom ?? native.current,
  }), [failure]);

  useEffect(() => {
    if (reading || failure || !host.current) return;
    const parent = host.current;
    const currentText = canonicalMarkdown(propsRef.current.value);
    const savedState = retained.current;
    const config: EditorStateConfig = { doc: currentText, extensions: [
      history(), drawSelection(), highlightActiveLine(), imageAnchors,
      markdown({ extensions: [GFM, noteSyntax] }), syntaxHighlighting(defaultHighlightStyle), foldGutter(),
      liveCompartment.of([]), wrapCompartment.of(EditorView.lineWrapping),
      preferenceCompartment.of([]),
      readonlyCompartment.of(EditorState.readOnly.of(effectiveMode === "read")),
      search({ top: true, createPanel: (view) => createNoteSearchPanel(view, (value) => textRef.current(value)) }),
      keymap.of([
        { key: "Mod-b", run: formatCommand("bold") }, { key: "Mod-i", run: formatCommand("italic") },
        { key: "Mod-k", run: (view) => { if (!canFormat(view.state)) return false; setInsertKind("link"); return true; } }, { key: "Shift-Enter", run: formatCommand("hardBreak") },
        { key: "Mod-s", run: () => { run(() => session!.save()); return true; } },
        { key: "Mod-/", run: () => { if (!session?.isComposing && !session?.pendingAssets) propsRef.current.onModeChange(propsRef.current.mode === "live" ? "source" : "live"); return true; } },
        { key: "Mod-Alt-ArrowUp", run: moveSection(-1) }, { key: "Mod-Alt-ArrowDown", run: moveSection(1) },
        ...markdownKeymap, indentWithTab, ...defaultKeymap, ...historyKeymap, ...searchKeymap, ...foldKeymap,
      ]),
      EditorState.transactionFilter.of((transaction) => {
        if (transaction.startState.readOnly && transaction.docChanged && !transaction.annotation(externalUpdate)) return [];
        if (transaction.docChanged && !noteContentWithinLimit(transaction.newDoc.toString())) { queueMicrotask(() => setError(nt("正文超过 500,000 字符或 2 MiB，插入已取消。"))); return []; }
        return transaction;
      }),
      EditorView.updateListener.of((update) => {
        if (update.docChanged && !update.transactions.some((transaction) => transaction.annotation(externalUpdate))) {
          propsRef.current.onChange(update.state.doc.toString());
          if (preferencesRef.current.typewriterMode && !update.view.composing) requestAnimationFrame(() => {
            if (viewRef.current === update.view) update.view.dispatch({ effects: EditorView.scrollIntoView(update.view.state.selection.main.head, { y: "center" }) });
          });
        }
        if (update.selectionSet || update.docChanged) { repaintToolbar((count) => count + 1); propsRef.current.onSelectionChange?.(); }
      }),
      EditorView.domEventHandlers({
        compositionstart: () => { session?.composition(true); }, compositionend: () => { session?.composition(false); },
        paste: (event, view) => {
          if (view.state.readOnly) { event.preventDefault(); return true; }
          if ((event.target as HTMLElement).closest(".note-live-block")) return false;
          if (inProtectedCode(view.state)) return false;
          const html = event.clipboardData?.getData("text/html");
          if (html) { event.preventDefault(); try { view.dispatch(view.state.replaceSelection(htmlClipboardToMarkdown(html))); } catch (error) { setError(String(error)); } return true; }
          const files = Array.from(event.clipboardData?.files ?? []).filter((file) => file.type.startsWith("image/"));
          if (files.length) { event.preventDefault(); run(() => imageInsertRef.current(files)); return true; }
          return false;
        },
        drop: (event, view) => {
          if (view.state.readOnly) { event.preventDefault(); return true; }
          const files = Array.from(event.dataTransfer?.files ?? []).filter((file) => file.type.startsWith("image/"));
          if (!files.length) return false;
          event.preventDefault();
          const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
          if (pos !== null) view.dispatch({ selection: { anchor: pos } });
          run(() => imageInsertRef.current(files)); return true;
        },
      }),
      EditorView.contentAttributes.of({ "aria-label": nt("Markdown 笔记正文"), spellcheck: "false" }),
    ] };
    let state = (savedState && savedState.doc.toString() !== currentText
      ? savedState.update({ changes: minimalTextChange(savedState.doc.toString(), currentText), annotations: [externalUpdate.of(true), Transaction.addToHistory.of(false)] }).state : savedState)
      ?? EditorState.create(config);
    if (!savedState && session?.editorStateJson?.doc === currentText) {
      // Recreate extensions with this host's listeners while retaining the
      // shared note's undo branches and source selection.
      state = EditorState.fromJSON(session.editorStateJson, config, { history: historyField });
    }
    const view = new EditorView({ state, parent }); viewRef.current = view;
    repaintToolbar((count) => count + 1);
    view.scrollDOM.scrollTop = scroll.current || session?.editorScrollTop || 0;
    let attempts = 0;
    const measure = () => {
      view.requestMeasure();
      if (parent.clientWidth > 0 && parent.clientHeight > 0 && view.contentDOM.getBoundingClientRect().height <= 0 && ++attempts >= 3) setFailure(true);
    };
    const observer = new ResizeObserver(measure); observer.observe(parent);
    const timers = [200, 500, 1000].map((delay) => setTimeout(measure, delay));
    return () => {
      timers.forEach(clearTimeout); observer.disconnect(); retained.current = view.state;
      if (view.composing) session?.composition(false);
      if (session) {
        const json = view.state.toJSON({ history: historyField });
        session.editorStateJson = JSON.stringify(json).length < 32 * 1024 * 1024 ? json : null;
        session.editorScrollTop = view.scrollDOM.scrollTop;
      }
      scroll.current = view.scrollDOM.scrollTop; view.destroy(); viewRef.current = null;
    };
  }, [reading, failure, noteId, session, liveCompartment, wrapCompartment, preferenceCompartment, readonlyCompartment]);

  useEffect(() => {
    if (!failure || !native.current || !retained.current) return;
    const selection = retained.current.selection.main;
    native.current.setSelectionRange(selection.from, selection.to);
    native.current.scrollTop = scroll.current;
    void session?.checkpoint().catch(() => undefined);
  }, [failure, reading, session]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({ effects: [liveCompartment.reconfigure(effectiveMode === "live" ? livePreview(noteId, directoryPath, language) : preferences.lineNumbers ? lineNumbers() : []),
      wrapCompartment.reconfigure(wrap ? EditorView.lineWrapping : []),
      readonlyCompartment.reconfigure(EditorState.readOnly.of(effectiveMode === "read")),
      preferenceCompartment.reconfigure([EditorState.tabSize.of(preferences.tabSize), EditorView.contentAttributes.of({ spellcheck: String(preferences.spellcheck) })])] });
    if (readSearch && effectiveMode === "read") openSearchPanel(view);
  }, [effectiveMode, reading, readSearch, failure, noteId, directoryPath, language, liveCompartment, wrapCompartment, wrap, preferenceCompartment, preferences, readonlyCompartment]);

  useEffect(() => {
    const view = viewRef.current, next = canonicalMarkdown(value);
    if (view && !view.composing && next !== view.state.doc.toString()) view.dispatch({ changes: minimalTextChange(view.state.doc.toString(), next), annotations: externalUpdate.of(true) });
  }, [value, effectiveMode]);
  const view = viewRef.current;
  void version;
  const writable = effectiveMode !== "read" && !failure && !blockFocused && !session?.isComposing;
  const command = (id: FormatId) => {
    if (["link", "table", "mermaid"].includes(id)) { setInsertKind(id as "link" | "table" | "mermaid"); return; }
    if (viewRef.current) formatCommand(id)(viewRef.current);
  };

  return <div className={`note-markdown-editor${focus ? " is-focused-writing" : ""}`} data-mode={effectiveMode} onFocusCapture={(event) => {
    const element = event.target as HTMLElement;
    if (element.closest(".note-editor-content")) setBlockFocused(!!element.closest(".note-live-block"));
  }}>
    <div className="note-format-toolbar" role="toolbar" aria-label={nt("Markdown 格式")}>
      <div className="note-editor-modes" role="group" aria-label={nt("编辑模式")}>{([
        ["live", nt("实时编辑"), PencilLine], ["source", nt("源码"), Code2], ["read", nt("阅读"), Eye],
      ] as const).map(([id, label, Icon]) => <button type="button" key={id} title={label} aria-label={label} aria-pressed={effectiveMode === id} disabled={session?.isComposing || assetsPending > 0} onClick={() => { if (!viewRef.current?.composing && !session?.isComposing && !assetsPending) { setReadSearch(false); setBlockFocused(false); props.onModeChange(id); } }}><Icon size={15} /><span>{label}</span></button>)}</div>
      <select aria-label={nt("段落样式")} disabled={!writable} value={view ? selectedHeadingLevel(view.state) : 0} onChange={(event) => { if (viewRef.current) setHeading(Number(event.target.value))(viewRef.current); }}>
        <option value="mixed" disabled>{nt("混合段落")}</option><option value="0">{nt("正文")}</option>{[1, 2, 3, 4, 5, 6].map((level) => <option key={level} value={level}>H{level}</option>)}
      </select>
      <div className="note-format-primary">{([
        ["bold", nt("粗体"), Bold], ["italic", nt("斜体"), Italic], ["strike", nt("删除线"), Strikethrough], ["inlineCode", nt("行内代码"), Code],
        ["bullet", nt("无序列表"), List], ["ordered", nt("有序列表"), ListOrdered], ["task", nt("任务列表"), ListTodo], ["quote", nt("引用"), Quote],
      ] as const).map(([id, label, Icon]) => <button type="button" key={id} title={label} aria-label={label} disabled={!writable || !!view && !canFormat(view.state) && !(id === "inlineCode" && formatActive(view.state, id))} aria-pressed={!!view && formatActive(view.state, id)} onMouseDown={(event) => event.preventDefault()} onClick={() => command(id)}><Icon size={15} /></button>)}</div>
      <select aria-label={nt("插入 Markdown")} disabled={!writable} value="" onChange={(event) => command(event.target.value as FormatId)}>
        <option value="" disabled>{nt("插入")}</option>{([
          ["link", nt("链接")], ["table", nt("表格")], ["code", nt("代码块")], ["math", nt("公式")], ["mermaid", "Mermaid"], ["footnote", nt("脚注")], ["rule", nt("分隔线")], ["callout", nt("提示块")], ["highlight", nt("高亮")], ["underline", nt("下划线")], ["sup", nt("上标")], ["sub", nt("下标")],
        ] as const).map(([id, label]) => <option value={id} key={id}>{label}</option>)}
      </select>
      <button type="button" aria-label={nt("插入图片")} title={nt("插入图片")} disabled={!writable || assetsPending > 0} onClick={() => fileInput.current?.click()}><ImagePlus size={15} /></button>
      <button type="button" aria-label={nt("撤销")} title={nt("撤销")} disabled={!writable || !view || !undoDepth(view.state)} onClick={() => view && undo(view)}><Undo2 size={15} /></button>
      <button type="button" aria-label={nt("重做")} title={nt("重做")} disabled={!writable || !view || !redoDepth(view.state)} onClick={() => view && redo(view)}><Redo2 size={15} /></button>
      <button type="button" aria-label={nt("查找替换")} title={nt("查找替换")} onClick={() => { const current = viewRef.current; if (current) openSearchPanel(current); else setReadSearch(true); }}><Search size={15} /></button>
      <button type="button" aria-label={nt("版本历史")} title={nt("版本历史")} onClick={() => setHistoryOpen((open) => !open)}><History size={15} /></button>
      <button type="button" aria-label={nt("保存笔记")} title={nt("保存笔记")} disabled={assetsPending > 0 || !session} onClick={() => run(() => session!.save())}><Save size={15} /></button>
      <details className="note-editor-more"><summary aria-label={nt("更多操作")} title={nt("更多操作")}><MoreHorizontal size={16} /></summary><div>
        <button type="button" onClick={() => { setFocus(!focus); }}><Focus size={15} />{nt("专注模式")}</button>
        <button type="button" onClick={() => setWrap(!wrap)}><WrapText size={15} />{nt("自动换行")}</button>
        <button type="button" onClick={() => view && gotoLine(view)}>{nt("跳转行")}</button>
        <button type="button" disabled={!writable} onClick={() => view && moveSection(-1)(view)}>{nt("上移章节")}</button>
        <button type="button" disabled={!writable} onClick={() => view && moveSection(1)(view)}>{nt("下移章节")}</button>
        <button type="button" onClick={() => downloadNoteText(sessionState.title, value)}><Download size={15} />{nt("导出当前 Markdown")}</button>
        <button type="button" onClick={() => run(async () => { const module = await import("../editor/exportNote"); await module.exportNoteBundle(noteId, sessionState.title, value); })}><Download size={15} />{nt("导出 Markdown 与附件")}</button>
        <button type="button" onClick={() => run(async () => { const module = await import("../editor/exportNote"); await module.exportNoteHtml(noteId, sessionState.title, value, host.current ?? window.document.body); })}><Download size={15} />{nt("导出 HTML")}</button>
      </div></details>
      <input ref={fileInput} type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple hidden onChange={(event) => { const files = Array.from(event.target.files ?? []); event.target.value = ""; run(() => insertImages(files)); }} />
    </div>
    {insertKind && effectiveMode !== "read" ? <NoteInsertPanel key={insertKind} kind={insertKind} onClose={() => { setInsertKind(null); viewRef.current?.focus(); }} onInsert={(id, argument) => {
      if (viewRef.current) formatCommand(id, argument)(viewRef.current);
    }} /> : null}
    {session ? <NoteRecoveryBar session={session} state={sessionState} onRestoreMode={() => { setReadSearch(false); props.onModeChange("live"); }} /> : null}
    {error ? <div className="note-editor-error" role="alert">{error}<button type="button" onClick={() => setError("")}>{nt("关闭")}</button></div> : null}
    {large && mode === "live" ? <div className="note-editor-notice">{nt("大文档 · 源码模式")}</div> : null}
    {failure ? <div className="note-editor-notice">{nt("编辑器绘制失败 · 纯文本恢复模式")} <button type="button" onClick={() => { if (!session?.isComposing) setFailure(false); }}>{nt("重试编辑器")}</button></div> : null}
    {historyOpen && session ? <NoteHistoryPanel session={session} onRestoreMode={() => { setReadSearch(false); props.onModeChange("live"); }} onClose={() => setHistoryOpen(false)} /> : null}
    <div className="note-editor-content">
      {reading ? <div ref={props.previewRef} className="note-editor-reading" onMouseUp={props.onPreviewMouseUp}>
        <Suspense fallback={<pre>{value}</pre>}><Preview noteId={noteId} content={value} directoryPath={directoryPath} /></Suspense>
      </div> : failure ? <textarea ref={native} className="notes-source-editor-native" aria-label={nt("Markdown 纯文本恢复编辑器")} readOnly={effectiveMode === "read"} value={value}
        onCompositionStart={() => session?.composition(true)} onCompositionEnd={() => session?.composition(false)}
        onChange={(event) => {
          if (!noteContentWithinLimit(event.target.value)) { setError(nt("正文超过 500,000 字符或 2 MiB，插入已取消。")); return; }
          const state = retained.current;
          if (state) retained.current = state.update({ changes: minimalTextChange(state.doc.toString(), event.target.value),
            selection: { anchor: event.target.selectionStart, head: event.target.selectionEnd }, userEvent: "input.type" }).state;
          props.onChange(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || session?.isComposing || !(event.ctrlKey || event.metaKey)) return;
          if (event.key.toLowerCase() === "s") { event.preventDefault(); run(() => session!.save()); }
          if (!retained.current || effectiveMode === "read") return;
          if (["z", "y"].includes(event.key.toLowerCase())) {
            event.preventDefault();
            (event.shiftKey || event.key.toLowerCase() === "y" ? redo : undo)({ state: retained.current, dispatch: nativeTransaction });
          }
        }}
        onSelect={props.onSelectionChange} onMouseUp={props.onMouseUp} /> : <div ref={host} className="note-editor-cm" onMouseUp={props.onMouseUp} />}
    </div>
    <div className="note-editor-save-state" role="status">
      {sessionState.phase === "loading" ? nt("正在加载") : assetsPending ? nt("图片保留中") : sessionState.phase === "saving" ? nt("正在保存") : sessionState.phase === "conflict" ? nt("存在冲突") : sessionState.phase === "error" ? nt("保存失败") : session?.dirty ? sessionState.checkpointGeneration >= sessionState.generation ? nt("草稿已保留") : nt("有未保存修改") : nt("已保存")}
    </div>
  </div>;
});
