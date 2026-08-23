import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, drawSelection } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { searchKeymap } from "@codemirror/search";
import { tags } from "@lezer/highlight";
import { shouldUsePlainTextNoteEditor } from "./markdownEditorPolicy";

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
  onMouseUp?: (event: ReactMouseEvent<HTMLElement>) => void;
};

export const MarkdownSourceEditor = forwardRef<MarkdownSourceEditorHandle, MarkdownSourceEditorProps>(
  function MarkdownSourceEditor({ value, ariaLabel, className, onChange, onSelectionChange, onMouseUp }, ref) {
    const hostRef = useRef<HTMLDivElement>(null);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const valueRef = useRef(value);
    const onChangeRef = useRef(onChange);
    const onSelectionRef = useRef(onSelectionChange);
    const [plainTextMode, setPlainTextMode] = useState(() => shouldUsePlainTextNoteEditor(value.length));
    valueRef.current = value;
    onChangeRef.current = onChange;
    onSelectionRef.current = onSelectionChange;

    useEffect(() => {
      if (shouldUsePlainTextNoteEditor(value.length)) setPlainTextMode(true);
    }, [value.length]);

    useImperativeHandle(ref, () => ({
      focus: () => {
        if (plainTextMode) textareaRef.current?.focus();
        else viewRef.current?.focus();
      },
      getText: () => plainTextMode
        ? textareaRef.current?.value ?? valueRef.current
        : viewRef.current?.state.doc.toString() ?? valueRef.current,
      getSelection: () => {
        if (plainTextMode) {
          const editor = textareaRef.current;
          if (!editor) return { text: "", from: 0, to: 0 };
          const from = editor.selectionStart;
          const to = editor.selectionEnd;
          return { text: editor.value.slice(from, to), from, to };
        }
        const view = viewRef.current;
        if (!view) return { text: "", from: 0, to: 0 };
        const range = view.state.selection.main;
        return { text: view.state.sliceDoc(range.from, range.to), from: range.from, to: range.to };
      },
      setSelection: (from, to = from) => {
        if (plainTextMode) {
          const editor = textareaRef.current;
          if (!editor) return;
          const nextFrom = Math.max(0, Math.min(from, editor.value.length));
          const nextTo = Math.max(nextFrom, Math.min(to, editor.value.length));
          editor.focus();
          editor.setSelectionRange(nextFrom, nextTo);
          return;
        }
        const view = viewRef.current;
        if (!view) return;
        const nextFrom = Math.max(0, Math.min(from, view.state.doc.length));
        const nextTo = Math.max(nextFrom, Math.min(to, view.state.doc.length));
        view.dispatch({ selection: { anchor: nextFrom, head: nextTo }, scrollIntoView: true });
      },
      scrollToLine: (line) => {
        if (plainTextMode) {
          const editor = textareaRef.current;
          if (!editor) return;
          const lines = editor.value.split("\n");
          const safeLine = Math.max(1, Math.min(line, lines.length));
          const offset = lines.slice(0, safeLine - 1).reduce((total, entry) => total + entry.length + 1, 0);
          editor.setSelectionRange(offset, offset);
          const lineHeight = Number.parseFloat(getComputedStyle(editor).lineHeight) || 24;
          editor.scrollTop = Math.max(0, (safeLine - 1) * lineHeight);
          return;
        }
        const view = viewRef.current;
        if (!view) return;
        const safeLine = Math.max(1, Math.min(line, view.state.doc.lines));
        view.dispatch({ effects: EditorView.scrollIntoView(view.state.doc.line(safeLine).from, { y: "start" }) });
      },
      getDom: () => plainTextMode ? textareaRef.current : viewRef.current?.dom ?? null,
    }), [plainTextMode]);

    useEffect(() => {
      const host = hostRef.current;
      if (plainTextMode || !host || viewRef.current) return;
      const state = EditorState.create({
        doc: valueRef.current,
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
      const view = new EditorView({ state, parent: host });
      viewRef.current = view;

      let frame: number | null = null;
      let disposed = false;
      let lastWidth = -1;
      let lastHeight = -1;
      const scheduleMeasure = () => {
        if (disposed || frame !== null) return;
        frame = window.requestAnimationFrame(() => {
          frame = null;
          const bounds = host.getBoundingClientRect();
          const width = Math.round(bounds.width);
          const height = Math.round(bounds.height);
          if (width === lastWidth && height === lastHeight) return;
          lastWidth = width;
          lastHeight = height;
          view.requestMeasure();
          if (valueRef.current && width > 0 && height > 0 && view.contentDOM.getBoundingClientRect().height <= 0) {
            setPlainTextMode(true);
          }
        });
      };
      scheduleMeasure();
      const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleMeasure);
      observer?.observe(host);
      void document.fonts?.ready.then(scheduleMeasure);

      return () => {
        disposed = true;
        observer?.disconnect();
        if (frame !== null) window.cancelAnimationFrame(frame);
        view.destroy();
        if (viewRef.current === view) viewRef.current = null;
      };
    }, [ariaLabel, plainTextMode]);

    useEffect(() => {
      if (plainTextMode) return;
      const view = viewRef.current;
      if (!view || value === view.state.doc.toString()) return;
      const current = view.state.doc.toString();
      valueRef.current = value;
      view.dispatch({ changes: { from: 0, to: current.length, insert: value } });
    }, [plainTextMode, value]);

    const handlePlainTextKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key !== "Tab") return;
      event.preventDefault();
      const editor = event.currentTarget;
      const from = editor.selectionStart;
      const to = editor.selectionEnd;
      const next = `${editor.value.slice(0, from)}  ${editor.value.slice(to)}`;
      valueRef.current = next;
      onChangeRef.current(next);
      window.requestAnimationFrame(() => {
        editor.setSelectionRange(from + 2, from + 2);
        onSelectionRef.current?.();
      });
    };

    if (plainTextMode) {
      return (
        <textarea
          ref={textareaRef}
          className={`notes-source-editor notes-source-editor-native${className ? ` ${className}` : ""}`}
          value={value}
          aria-label={ariaLabel}
          spellCheck={false}
          onChange={(event) => {
            valueRef.current = event.currentTarget.value;
            onChangeRef.current(event.currentTarget.value);
          }}
          onKeyDown={handlePlainTextKeyDown}
          onSelect={() => onSelectionRef.current?.()}
          onMouseUp={onMouseUp}
        />
      );
    }

    return <div ref={hostRef} className={`notes-source-editor-cm${className ? ` ${className}` : ""}`} aria-label={ariaLabel} onMouseUp={onMouseUp} />;
  },
);
