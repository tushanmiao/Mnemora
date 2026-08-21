import {
  lazy,
  Suspense,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import {
  Bot,
  Copy,
  Eye,
  FileCode2,
  FileText,
  LoaderCircle,
  ListTree,
  MessageCircle,
  Quote,
  Save,
  Trash2,
  WandSparkles,
} from "lucide-react";
import {
  deleteLibraryNote,
  getLibraryNote,
  updateLibraryNote,
} from "../../library/api/library";
import type { LibraryNote } from "../../library/types";
import type { NoteReference } from "../../../types/chat";
import type {
  ActiveWorkNoteContext,
  WorkNoteSourceContext,
} from "../../workspace/types";
import {
  lineAtOffset,
  loadNotesLayout,
  OUTLINE_DEFAULT_WIDTH,
  OUTLINE_MAX_WIDTH,
  OUTLINE_MIN_WIDTH,
  persistNotesLayout,
  revisionHash,
} from "../utils/notesWorkspace";
import { extractMarkdownOutline, type MarkdownOutlineItem } from "../../chat/markdown/utils/outline";
import { PanelResizeHandle } from "../../layout/components/PanelResizeHandle";
import { NoteSourcesBar } from "./NoteSourcesBar";
import type { NoteSelectionMenu } from "./NoteEditor";
import "../styles/notes.css";
import "../styles/notes-workspace.css";

const MarkdownNotePreview = lazy(() => import("./MarkdownNotePreview"));
const MAX_SELECTION_CHARACTERS = 16_000;
const MAX_NOTE_SNAPSHOT_BYTES = 32 * 1024;
const textEncoder = new TextEncoder();

function noteSnapshot(content: string) {
  if (textEncoder.encode(content).byteLength <= MAX_NOTE_SNAPSHOT_BYTES) return content;
  let low = 0;
  let high = content.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (textEncoder.encode(content.slice(0, middle)).byteLength <= MAX_NOTE_SNAPSHOT_BYTES) {
      low = middle;
    } else {
      high = middle - 1;
    }
  }
  return `${content.slice(0, low).trimEnd()}\n\n[笔记快照已按 32 KB 上限截断]`;
}

type NoteWorkspaceProps = {
  noteId: string;
  source: WorkNoteSourceContext | null;
  chatOpen: boolean;
  chatBusy: boolean;
  refreshVersion: number;
  onUpdated: (note: LibraryNote) => void;
  onDeleted: () => void;
  onToggleChat: () => void;
  onAskSelection: (reference: NoteReference) => void;
  onEditSelection: (selection: {
    noteId: string;
    selectedText: string;
    sectionHeading: string;
  }) => void;
  onContextChange: (context: ActiveWorkNoteContext | null) => void;
  onOpenSourcePdf?: (source: WorkNoteSourceContext) => void;
};

function sameWorkNoteContext(
  left: ActiveWorkNoteContext | null,
  right: ActiveWorkNoteContext | null,
) {
  return left?.noteId === right?.noteId
    && left?.noteTitle === right?.noteTitle
    && left?.revisionHash === right?.revisionHash
    && left?.noteSnapshot === right?.noteSnapshot
    && left?.source?.sourcePdfId === right?.source?.sourcePdfId
    && left?.source?.sourcePdfTitle === right?.source?.sourcePdfTitle
    && left?.source?.sourcePageIndex === right?.source?.sourcePageIndex;
}

export function NoteWorkspace({
  noteId,
  source,
  chatOpen,
  chatBusy,
  refreshVersion,
  onUpdated,
  onDeleted,
  onToggleChat,
  onAskSelection,
  onEditSelection,
  onContextChange,
  onOpenSourcePdf,
}: NoteWorkspaceProps) {
  const previewRef = useRef<HTMLDivElement>(null);
  const sourceRef = useRef<HTMLTextAreaElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const lastContextRef = useRef<ActiveWorkNoteContext | null>(null);
  const loadRequestRef = useRef(0);
  const [note, setNote] = useState<LibraryNote | null>(null);
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [mode, setMode] = useState<"source" | "preview">("source");
  const [outlineLayout, setOutlineLayout] = useState(loadNotesLayout);
  const [selectionMenu, setSelectionMenu] = useState<NoteSelectionMenu | null>(null);
  const deferredContent = useDeferredValue(content);
  const outline = useMemo(
    () => extractMarkdownOutline(deferredContent, `note-${noteId}`),
    [deferredContent, noteId],
  );

  const loadNote = useCallback(async ({
    quiet = false,
    reset = false,
  }: {
    quiet?: boolean;
    reset?: boolean;
  } = {}) => {
    const requestId = ++loadRequestRef.current;
    if (!quiet) setLoading(true);
    if (reset) {
      setNote(null);
      setTitle("");
      setContent("");
      setSelectionMenu(null);
    }
    setError("");
    try {
      const next = await getLibraryNote(noteId);
      if (requestId !== loadRequestRef.current) return;
      setNote(next);
      setTitle(next.title);
      setContent(next.content);
      onUpdated(next);
    } catch (loadError) {
      if (requestId !== loadRequestRef.current) return;
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      if (requestId === loadRequestRef.current) setLoading(false);
    }
  }, [noteId, onUpdated]);

  useEffect(() => {
    void loadNote({ reset: true });
    return () => {
      loadRequestRef.current += 1;
    };
  }, [loadNote]);

  useEffect(() => {
    if (refreshVersion <= 0) return;
    void loadNote({ quiet: true });
  }, [loadNote, refreshVersion]);

  useEffect(() => {
    if (!note) {
      if (lastContextRef.current) {
        lastContextRef.current = null;
        onContextChange(null);
      }
      return;
    }
    const nextContext = {
      noteId: note.id,
      noteTitle: title.trim() || note.title,
      revisionHash: revisionHash({ ...note, title, content }),
      noteSnapshot: noteSnapshot(content),
      source,
    } satisfies ActiveWorkNoteContext;
    if (!sameWorkNoteContext(lastContextRef.current, nextContext)) {
      lastContextRef.current = nextContext;
      onContextChange(nextContext);
    }
  }, [content, note, onContextChange, source, title]);

  useEffect(() => () => {
    lastContextRef.current = null;
    onContextChange(null);
  }, [onContextChange]);

  const save = async () => {
    if (!title.trim() || saving) return;
    setSaving(true);
    setError("");
    try {
      const updated = await updateLibraryNote({ noteId, title, content });
      setNote(updated);
      setTitle(updated.title);
      setContent(updated.content);
      onUpdated(updated);
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : String(saveError));
    } finally {
      setSaving(false);
    }
  };

  const showSourceSelection = (
    event: ReactMouseEvent<HTMLTextAreaElement> | ReactKeyboardEvent<HTMLTextAreaElement>,
  ) => {
    const editor = event.currentTarget;
    const selectedText = editor.value.slice(editor.selectionStart, editor.selectionEnd).trim();
    if (!selectedText) {
      setSelectionMenu(null);
      return;
    }
    const rect = editor.getBoundingClientRect();
    const bodyRect = bodyRef.current?.getBoundingClientRect() ?? rect;
    const keyboardEvent = "key" in event;
    setSelectionMenu({
      left: keyboardEvent
        ? Math.max(18, rect.left - bodyRect.left + 18)
        : Math.min(bodyRect.width - 224, Math.max(12, event.clientX - bodyRect.left)),
      top: keyboardEvent
        ? 18
        : Math.min(bodyRect.height - 44, Math.max(12, event.clientY - bodyRect.top + 10)),
      text: selectedText.slice(0, MAX_SELECTION_CHARACTERS),
      startLine: lineAtOffset(editor.value, editor.selectionStart),
      endLine: lineAtOffset(editor.value, editor.selectionEnd),
    });
  };

  const showPreviewSelection = (event: ReactMouseEvent<HTMLDivElement>) => {
    const host = previewRef.current;
    const selection = window.getSelection();
    if (!host || !selection || selection.isCollapsed || !selection.anchorNode || !selection.focusNode
      || !host.contains(selection.anchorNode) || !host.contains(selection.focusNode)) {
      setSelectionMenu(null);
      return;
    }
    const text = selection.toString().trim();
    if (!text) return;
    const rect = bodyRef.current?.getBoundingClientRect() ?? host.getBoundingClientRect();
    setSelectionMenu({
      left: Math.min(rect.width - 224, Math.max(12, event.clientX - rect.left)),
      top: Math.min(rect.height - 44, Math.max(12, event.clientY - rect.top + 10)),
      text: text.slice(0, MAX_SELECTION_CHARACTERS),
    });
  };

  const clearSelection = () => {
    setSelectionMenu(null);
    window.getSelection()?.removeAllRanges();
  };

  const askSelection = () => {
    if (!note || !selectionMenu) return;
    onAskSelection({
      id: crypto.randomUUID(),
      noteId: note.id,
      noteTitle: title.trim() || note.title,
      revisionHash: revisionHash({ ...note, title, content }),
      startLine: selectionMenu.startLine,
      endLine: selectionMenu.endLine,
      selectedText: selectionMenu.text,
    });
    clearSelection();
  };

  const editSelection = async () => {
    if (!note || !selectionMenu || saving) return;
    let editNote = note;
    if (title !== note.title || content !== note.content) {
      setSaving(true);
      setError("");
      try {
        editNote = await updateLibraryNote({ noteId, title, content });
        setNote(editNote);
        setTitle(editNote.title);
        setContent(editNote.content);
        onUpdated(editNote);
      } catch (saveError) {
        setError(saveError instanceof Error ? saveError.message : String(saveError));
        setSaving(false);
        return;
      }
      setSaving(false);
    }
    const lines = content.split(/\r?\n/);
    const beforeSelection = selectionMenu.startLine
      ? lines.slice(0, selectionMenu.startLine)
      : lines;
    const sectionHeading = beforeSelection
      .reverse()
      .find((line) => /^##\s+/.test(line))
      ?.replace(/^##\s+/, "")
      .trim() ?? "";
    onEditSelection({ noteId: editNote.id, selectedText: selectionMenu.text, sectionHeading });
    clearSelection();
  };

  const jumpToOutline = (item: MarkdownOutlineItem) => {
    setSelectionMenu(null);
    if (mode === "preview") {
      const host = previewRef.current;
      const heading = host?.querySelector<HTMLElement>(`#${CSS.escape(item.id)}`);
      if (host && heading) {
        const hostRect = host.getBoundingClientRect();
        const headingRect = heading.getBoundingClientRect();
        const targetTop = host.scrollTop + headingRect.top - hostRect.top - 18;
        host.scrollTo({ top: Math.max(0, targetTop), behavior: "smooth" });
      }
      return;
    }
    const editor = sourceRef.current;
    if (!editor) return;
    const style = window.getComputedStyle(editor);
    const lineHeight = Number.parseFloat(style.lineHeight) || 26;
    const paddingTop = Number.parseFloat(style.paddingTop) || 0;
    const line = lineAtOffset(content, item.offset);
    editor.focus({ preventScroll: true });
    editor.setSelectionRange(item.offset, item.offset);
    editor.scrollTop = Math.max(0, (line - 1) * lineHeight + paddingTop - 18);
  };

  const previewOutlineWidth = (width: number) => {
    bodyRef.current?.style.setProperty("--note-workspace-outline-width", `${width}px`);
  };

  const commitOutlineWidth = (width: number) => {
    const next = { ...outlineLayout, outlineWidth: width };
    setOutlineLayout(next);
    persistNotesLayout(next);
  };

  const toggleOutline = () => {
    const next = { ...outlineLayout, outlineOpen: !outlineLayout.outlineOpen };
    setOutlineLayout(next);
    persistNotesLayout(next);
  };

  if (loading) {
    return <div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>正在打开笔记</span></div>;
  }
  if (!note || error && !note) {
    return <div className="work-library-state work-library-error" role="alert"><strong>笔记无法打开</strong><span>{error || "笔记不存在。"}</span></div>;
  }

  const dirty = title !== note.title || content !== note.content;

  return (
    <section className="note-workspace note-workspace-contextual" aria-label={note.title}>
      <header>
        <div className="note-workspace-origin">
          <FileText size={15} />
          {source && onOpenSourcePdf ? (
            <button type="button" title={`返回 ${source.sourcePdfTitle}`} onClick={() => onOpenSourcePdf(source)}>
              <span>{source.sourcePdfTitle}</span>
              {source.sourcePageIndex !== null ? <small>第 {source.sourcePageIndex + 1} 页</small> : null}
            </button>
          ) : <span title={note.itemTitle ?? "全局笔记"}>{note.itemTitle ?? "全局笔记"}</span>}
        </div>
        <div>
          <button type="button" title={mode === "source" ? "切换为预览" : "切换为 Markdown 源码"} onClick={() => { setSelectionMenu(null); setMode((current) => current === "source" ? "preview" : "source"); }}>
            {mode === "source" ? <Eye size={15} /> : <FileCode2 size={15} />}
            <span>{mode === "source" ? "预览" : "Markdown"}</span>
          </button>
          <button className={outlineLayout.outlineOpen ? "is-active" : ""} type="button" title={outlineLayout.outlineOpen ? "收起笔记大纲" : "展开笔记大纲"} aria-pressed={outlineLayout.outlineOpen} onClick={toggleOutline}>
            <ListTree size={15} /><span>大纲</span>
          </button>
          <button type="button" disabled={!dirty || saving || !title.trim()} onClick={() => void save()}>
            {saving ? <LoaderCircle className="is-spinning" size={15} /> : <Save size={15} />}
            <span>{saving ? "正在保存" : "保存"}</span>
          </button>
          <button className={chatOpen ? "is-active" : ""} type="button" title={chatOpen ? "收起当前笔记 Chat" : "打开当前笔记 Chat"} onClick={onToggleChat}>
            {chatBusy ? <LoaderCircle className="is-spinning" size={15} /> : chatOpen ? <Bot size={15} /> : <MessageCircle size={15} />}
            <span>{chatOpen ? "收起 Chat" : "笔记 Chat"}</span>
          </button>
          <button className="is-danger" type="button" onClick={() => {
            if (!window.confirm(`删除笔记“${note.title}”吗？`)) return;
            void deleteLibraryNote(note.id)
              .then((removed) => { if (removed) onDeleted(); })
              .catch((deleteError) => setError(deleteError instanceof Error ? deleteError.message : String(deleteError)));
          }}>
            <Trash2 size={15} /><span>删除</span>
          </button>
        </div>
      </header>
      {error ? <p className="note-workspace-error" role="alert">{error}</p> : null}
      <NoteSourcesBar noteId={note.id} />
      <div className="note-workspace-document">
        <input className="note-workspace-title" value={title} maxLength={500} aria-label="笔记标题" onChange={(event) => setTitle(event.target.value)} />
        <div
          ref={bodyRef}
          className="note-workspace-body"
          data-outline={outlineLayout.outlineOpen ? "open" : "closed"}
          style={{ "--note-workspace-outline-width": `${outlineLayout.outlineWidth}px` } as CSSProperties}
        >
          {outlineLayout.outlineOpen ? (
            <aside className="note-workspace-outline" aria-label="笔记大纲">
              <header><ListTree size={14} /><strong>大纲</strong><span>{outline.length}</span></header>
              <nav>
                {outline.length > 0 ? outline.map((item) => (
                  <button
                    type="button"
                    key={item.id}
                    title={item.title}
                    style={{ paddingInlineStart: `${12 + (item.level - 1) * 13}px` }}
                    onClick={() => jumpToOutline(item)}
                  >
                    {item.title}
                  </button>
                )) : <p>没有检测到标题。使用 “#” 开头的标题行即可建立大纲。</p>}
              </nav>
              <PanelResizeHandle
                edge="right"
                value={outlineLayout.outlineWidth}
                defaultValue={OUTLINE_DEFAULT_WIDTH}
                minValue={OUTLINE_MIN_WIDTH}
                maxValue={OUTLINE_MAX_WIDTH}
                label="调整笔记大纲宽度"
                onPreview={previewOutlineWidth}
                onCommit={commitOutlineWidth}
              />
            </aside>
          ) : null}
          <div className="note-workspace-surface">
            {mode === "source" ? (
              <textarea
                ref={sourceRef}
                className="note-workspace-content"
                value={content}
                maxLength={500_000}
                aria-label="Markdown 笔记正文"
                onMouseUp={showSourceSelection}
                onKeyUp={showSourceSelection}
                onScroll={() => setSelectionMenu(null)}
                onChange={(event) => setContent(event.target.value)}
              />
            ) : (
              <div ref={previewRef} className="note-workspace-preview" onMouseUp={showPreviewSelection} onScroll={() => setSelectionMenu(null)}>
                <Suspense fallback={<div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={20} /><span>正在加载预览</span></div>}>
                  <MarkdownNotePreview noteId={note.id} content={content} />
                </Suspense>
              </div>
            )}
          </div>
          {selectionMenu ? (
            <div className="notes-selection-menu note-workspace-selection-menu" style={{ left: selectionMenu.left, top: selectionMenu.top }} onMouseDown={(event) => event.preventDefault()}>
              <button type="button" onClick={() => { void navigator.clipboard?.writeText(selectionMenu.text); clearSelection(); }}><Copy size={13} />复制</button>
              <button type="button" onClick={askSelection}><Quote size={13} />引用提问</button>
              <button type="button" onClick={() => void editSelection()}><WandSparkles size={13} />AI 修改</button>
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}

export default NoteWorkspace;
