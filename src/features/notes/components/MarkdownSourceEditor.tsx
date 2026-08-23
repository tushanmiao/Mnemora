import { forwardRef, useEffect, useImperativeHandle, useRef, type MouseEvent as ReactMouseEvent } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, drawSelection } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { searchKeymap } from "@codemirror/search";
import { tags } from "@lezer/highlight";

const markdownHighlightStyle = HighlightStyle.define([
  { tag: tags.heading, color: "var(--color-accent)", fontWeight: "700" },
  { tag: [tags.link, tags.url], color: "var(--color-accent)", textDecoration: "underline" },
  { tag: [tags.strong, tags.keyword], color: "var(--color-text)", fontWeight: "700" },
  { tag: tags.emphasis, color: "var(--color-text-secondary)", fontStyle: "italic" },
  { tag: [tags.quote, tags.comment, tags.meta], color: "var(--color-muted)" },
  { tag: [tags.monospace, tags.string], color: "var(--color-text-secondary)" },
  { tag: [tags.list, tags.punctuation, tags.separator], color: "var(--color-muted)" },
]);

export type MarkdownSourceEditorHandle = {
  focus: () => void;
  getText: () => string;
  getSelection: () => { text: string; from: number; to: number };
  setSelection: (from: number, to?: number) => void;
  scrollToLine: (line: number) => void;
  getDom: () => HTMLElement | null;
};

type MarkdownSourceEditorProps = {
  value: string;
  ariaLabel: string;
  className?: string;
  onChange: (value: string) => void;
  onSelectionChange?: () => void;
  onMouseUp?: (event: ReactMouseEvent<HTMLDivElement>) => void;
};

export const MarkdownSourceEditor = forwardRef<MarkdownSourceEditorHandle, MarkdownSourceEditorProps>(
  function MarkdownSourceEditor({ value, ariaLabel, className, onChange, onSelectionChange, onMouseUp }, ref) {
    const hostRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const valueRef = useRef(value);
    const onChangeRef = useRef(onChange);
    const onSelectionRef = useRef(onSelectionChange);
    onChangeRef.current = onChange;
    onSelectionRef.current = onSelectionChange;

    useImperativeHandle(ref, () => ({
      focus: () => viewRef.current?.focus(),
      getText: () => viewRef.current?.state.doc.toString() ?? valueRef.current,
      getSelection: () => {
        const view = viewRef.current;
        if (!view) return { text: "", from: 0, to: 0 };
        const range = view.state.selection.main;
        return { text: view.state.sliceDoc(range.from, range.to), from: range.from, to: range.to };
      },
      setSelection: (from, to = from) => {
        const view = viewRef.current;
        if (!view) return;
        const nextFrom = Math.max(0, Math.min(from, view.state.doc.length));
        const nextTo = Math.max(nextFrom, Math.min(to, view.state.doc.length));
        view.dispatch({ selection: { anchor: nextFrom, head: nextTo }, scrollIntoView: true });
      },
      scrollToLine: (line) => {
        const view = viewRef.current;
        if (!view) return;
        const safeLine = Math.max(1, Math.min(line, view.state.doc.lines));
        view.dispatch({ effects: EditorView.scrollIntoView(view.state.doc.line(safeLine).from, { y: "start" }) });
      },
      getDom: () => viewRef.current?.dom ?? null,
    }), []);

    useEffect(() => {
      if (!hostRef.current || viewRef.current) return;
      valueRef.current = value;
      const state = EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          drawSelection(),
          highlightActiveLine(),
          keymap.of([indentWithTab, ...defaultKeymap, ...historyKeymap, ...searchKeymap]),
          markdown(),
          syntaxHighlighting(markdownHighlightStyle, { fallback: true }),
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              const next = update.state.doc.toString();
              valueRef.current = next;
              onChangeRef.current(next);
            }
            if (update.selectionSet) onSelectionRef.current?.();
          }),
          EditorView.contentAttributes.of({ "aria-label": ariaLabel, spellcheck: "false" }),
          EditorView.theme({
            "&": { height: "100%", color: "var(--color-text)", backgroundColor: "var(--color-surface-layer)" },
            ".cm-scroller": { overflow: "auto", fontFamily: "var(--note-font-family)", fontSize: "var(--note-font-size)", lineHeight: "var(--note-line-height)" },
            ".cm-content": { minHeight: "100%", padding: "28px clamp(24px, 4vw, 64px) 60px", color: "var(--color-text)", caretColor: "var(--color-accent)" },
            ".cm-line": { color: "var(--color-text)" },
            ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-accent)" },
            "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": { backgroundColor: "var(--color-accent-soft)" },
            ".cm-gutters": { color: "var(--color-muted)", backgroundColor: "var(--color-surface-layer)", border: "none" },
            ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "color-mix(in srgb, var(--color-accent-soft) 42%, transparent)" },
            ".cm-focused": { outline: "none" },
          }),
        ],
      });
      viewRef.current = new EditorView({ state, parent: hostRef.current });
      return () => {
        viewRef.current?.destroy();
        viewRef.current = null;
      };
      // The editor is intentionally initialized once per mounted note.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    useEffect(() => {
      const view = viewRef.current;
      if (!view || value === valueRef.current) return;
      const current = view.state.doc.toString();
      if (current === value) {
        valueRef.current = value;
        return;
      }
      valueRef.current = value;
      view.dispatch({ changes: { from: 0, to: current.length, insert: value } });
    }, [value]);

    return <div ref={hostRef} className={`notes-source-editor-cm${className ? ` ${className}` : ""}`} aria-label={ariaLabel} onMouseUp={onMouseUp} />;
  },
);
