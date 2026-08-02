import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import {
  ArrowLeft,
  Bot,
  Check,
  Copy,
  Eye,
  FileCode2,
  FilePlus2,
  FileText,
  Folder,
  LoaderCircle,
  PanelRightOpen,
  Quote,
  Search,
  Trash2,
} from "lucide-react";
import {
  createLibraryNote,
  deleteLibraryNote,
  getLibraryNote,
  chooseLibraryMarkdownFiles,
  importLibraryMarkdownNotes,
  listLibraryNotes,
  updateLibraryNote,
} from "../../library/api/library";
import type { LibraryNote, LibraryNoteSummary } from "../../library/types";
import type { NoteReference } from "../../../types/chat";
import "../styles/notes-workspace.css";

const MarkdownNotePreview = lazy(() => import("./MarkdownNotePreview"));
const AUTOSAVE_DELAY_MS = 700;
const MAX_SELECTION_CHARACTERS = 16_000;
const NOTE_GROUPS_STORAGE_KEY = "mnemora.notes.groups.v1";
const CUSTOM_GROUPS_STORAGE_KEY = "mnemora.notes.custom-groups.v1";

type NotesWorkspaceProps = {
  chatOpen: boolean;
  chatBusy: boolean;
  onToggleChat: () => void;
  onAskSelection: (reference: NoteReference) => void;
  onBack: () => void;
};

type SelectionMenu = {
  left: number;
  top: number;
  text: string;
  startLine?: number;
  endLine?: number;
};

function revisionHash(note: LibraryNote) {
  return `${note.updatedAt.toString(36)}-${note.content.length.toString(36)}`;
}

function lineAtOffset(content: string, offset: number) {
  return content.slice(0, offset).split("\n").length;
}

function noteStats(content: string) {
  const characters = Array.from(content).length;
  const words = content.trim() ? content.trim().split(/\s+/).filter(Boolean).length : 0;
  const readingMinutes = characters === 0 ? 0 : Math.max(1, Math.ceil(characters / 400));
  return { characters, words, readingMinutes };
}

export default function NotesWorkspace({
  chatOpen,
  chatBusy,
  onToggleChat,
  onAskSelection,
  onBack,
}: NotesWorkspaceProps) {
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const saveTimerRef = useRef<number | null>(null);
  const savedTimerRef = useRef<number | null>(null);
  const saveChainRef = useRef<Promise<void>>(Promise.resolve());
  const activeNoteRef = useRef<LibraryNote | null>(null);
  const titleRef = useRef("");
  const contentRef = useRef("");
  const mountedRef = useRef(true);
  const [notes, setNotes] = useState<LibraryNoteSummary[]>([]);
  const [activeNote, setActiveNote] = useState<LibraryNote | null>(null);
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"source" | "preview">("source");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [selectionMenu, setSelectionMenu] = useState<SelectionMenu | null>(null);
  const [noteGroups, setNoteGroups] = useState<Record<string, string>>({});
  const [customGroups, setCustomGroups] = useState<string[]>([]);
  activeNoteRef.current = activeNote;
  titleRef.current = title;
  contentRef.current = content;

  useEffect(() => {
    try {
      const parsed = JSON.parse(window.localStorage.getItem(NOTE_GROUPS_STORAGE_KEY) ?? "{}");
      if (!parsed || typeof parsed !== "object") return;
      const groups: Record<string, string> = {};
      for (const [id, value] of Object.entries(parsed)) {
        if (typeof value === "string") groups[id] = value;
      }
      setNoteGroups(groups);
      setCustomGroups(Array.from(new Set(Object.values(groups).filter((value): value is string => Boolean(value && value !== "未分类")))));
      const savedGroups = JSON.parse(window.localStorage.getItem(CUSTOM_GROUPS_STORAGE_KEY) ?? "[]");
      if (Array.isArray(savedGroups)) {
        setCustomGroups((current) => Array.from(new Set([...current, ...savedGroups.filter((value): value is string => typeof value === "string" && value.trim().length > 0)])));
      }
    } catch {
      // 损坏的本地分组数据不应阻止笔记页面打开。
    }
  }, []);

  const persistNoteGroups = useCallback((next: Record<string, string>) => {
    setNoteGroups(next);
    try {
      window.localStorage.setItem(NOTE_GROUPS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // 本地存储不可用时仍保留当前会话内的分组状态。
    }
  }, []);

  const createGroup = useCallback(() => {
    const name = window.prompt("请输入分组名称")?.trim();
    if (!name) return;
    setCustomGroups((current) => {
      const next = current.includes(name) ? current : [...current, name];
      try { window.localStorage.setItem(CUSTOM_GROUPS_STORAGE_KEY, JSON.stringify(next)); } catch { /* 忽略不可用的本地存储 */ }
      return next;
    });
  }, []);

  const assignGroup = useCallback((noteId: string, group: string) => {
    const next = { ...noteGroups };
    if (!group || group === "未分类") delete next[noteId];
    else next[noteId] = group;
    persistNoteGroups(next);
  }, [noteGroups, persistNoteGroups]);

  const clearSaveTimer = useCallback(() => {
    if (saveTimerRef.current === null) return;
    window.clearTimeout(saveTimerRef.current);
    saveTimerRef.current = null;
  }, []);

  const queueSave = useCallback((note: LibraryNote, nextTitle: string, nextContent: string) => {
    const normalizedTitle = nextTitle.trim() || "未命名笔记";
    if (mountedRef.current) {
      setSaving(true);
      setSaved(false);
      setError("");
    }
    const operation = saveChainRef.current
      .catch(() => undefined)
      .then(async () => {
        const updated = await updateLibraryNote({
          noteId: note.id,
          title: normalizedTitle,
          content: nextContent,
        });
        if (!mountedRef.current || activeNoteRef.current?.id !== updated.id) return;

        setActiveNote(updated);
        setNotes((current) => current.map((item) => item.id === updated.id ? {
          ...item,
          title: updated.title,
          contentPreview: updated.content.slice(0, 600),
          contentChars: Array.from(updated.content).length,
          updatedAt: updated.updatedAt,
        } : item).sort((a, b) => b.updatedAt - a.updatedAt));

        // 只规范化当前仍未继续编辑的标题，避免旧请求覆盖保存期间的新输入。
        if (titleRef.current === nextTitle) setTitle(updated.title);
        if (titleRef.current === nextTitle && contentRef.current === nextContent) {
          setSaved(true);
          if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
          savedTimerRef.current = window.setTimeout(() => setSaved(false), 1_200);
        }
      })
      .catch((saveError) => {
        if (mountedRef.current) {
          setError(saveError instanceof Error ? saveError.message : String(saveError));
        }
      })
      .finally(() => {
        if (mountedRef.current && saveChainRef.current === operation) setSaving(false);
      });
    saveChainRef.current = operation;
    return operation;
  }, []);

  const flushActiveDraft = useCallback(() => {
    clearSaveTimer();
    const note = activeNoteRef.current;
    if (!note || (titleRef.current === note.title && contentRef.current === note.content)) {
      return saveChainRef.current.catch(() => undefined);
    }
    return queueSave(note, titleRef.current, contentRef.current);
  }, [clearSaveTimer, queueSave]);

  const loadNotes = useCallback(async (preferredId?: string) => {
    setLoading(true);
    setError("");
    try {
      const next = (await listLibraryNotes()).filter((note) => note.itemId === null);
      setNotes(next);
      const targetId = preferredId;
      if (!targetId) {
        setActiveNote(null);
        setTitle("");
        setContent("");
        return;
      }
      const note = await getLibraryNote(targetId);
      setActiveNote(note);
      setTitle(note.title);
      setContent(note.content);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      setLoading(false);
    }
  }, [activeNote?.id]);

  const importNotes = useCallback(async () => {
    const paths = await chooseLibraryMarkdownFiles();
    if (paths.length === 0) return;
    setLoading(true);
    setError("");
    try {
      const result = await importLibraryMarkdownNotes(paths);
      if (result.failed.length > 0) {
        setError(`已导入 ${result.imported.length} 篇，${result.failed.length} 篇失败：${result.failed[0].error}`);
      }
      await loadNotes();
    } catch (importError) {
      setError(importError instanceof Error ? importError.message : String(importError));
    } finally {
      setLoading(false);
    }
  }, [loadNotes]);

  useEffect(() => {
    // Strict Mode 可能会先执行一次清理，再重新挂载；重新进入页面时恢复保存状态。
    mountedRef.current = true;
    void loadNotes();
    return () => {
      mountedRef.current = false;
      clearSaveTimer();
      if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
      const note = activeNoteRef.current;
      if (note && (titleRef.current !== note.title || contentRef.current !== note.content)) {
        void queueSave(note, titleRef.current, contentRef.current);
      }
    };
  }, [clearSaveTimer, queueSave]);

  useEffect(() => {
    if (!activeNote || (title === activeNote.title && content === activeNote.content)) return;
    clearSaveTimer();
    saveTimerRef.current = window.setTimeout(() => {
      saveTimerRef.current = null;
      void queueSave(activeNote, title, content);
    }, AUTOSAVE_DELAY_MS);
    return clearSaveTimer;
  }, [activeNote, clearSaveTimer, content, queueSave, title]);

  const createNote = async () => {
    if (saving) return;
    setError("");
    try {
      await flushActiveDraft();
      const note = await createLibraryNote({
        itemId: null,
        title: "未命名笔记",
        content: "# 未命名笔记\n\n",
      });
      await loadNotes(note.id);
      setMode("source");
      window.setTimeout(() => editorRef.current?.focus(), 0);
    } catch (createError) {
      setError(createError instanceof Error ? createError.message : String(createError));
    }
  };

  const openNote = async (noteId: string) => {
    if (activeNote?.id === noteId || saving) return;
    await flushActiveDraft();
    setSelectionMenu(null);
    setLoading(true);
    try {
      const note = await getLibraryNote(noteId);
      setActiveNote(note);
      setTitle(note.title);
      setContent(note.content);
    } catch (openError) {
      setError(openError instanceof Error ? openError.message : String(openError));
    } finally {
      setLoading(false);
    }
  };

  const removeNote = async () => {
    if (!activeNote || !window.confirm(`删除笔记“${activeNote.title}”吗？`)) return;
    try {
      clearSaveTimer();
      await saveChainRef.current.catch(() => undefined);
      await deleteLibraryNote(activeNote.id);
      const nextGroups = { ...noteGroups };
      delete nextGroups[activeNote.id];
      persistNoteGroups(nextGroups);
      setActiveNote(null);
      setTitle("");
      setContent("");
      const next = (await listLibraryNotes()).filter((note) => note.itemId === null);
      setNotes(next);
    } catch (deleteError) {
      setError(deleteError instanceof Error ? deleteError.message : String(deleteError));
    }
  };

  const filteredNotes = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return notes;
    return notes.filter((note) => [note.title, note.contentPreview]
      .some((value) => value.toLocaleLowerCase().includes(normalized)));
  }, [notes, query]);
  const groupedNotes = useMemo(() => {
    const groups = new Map<string, LibraryNoteSummary[]>();
    for (const note of filteredNotes) {
      const group = noteGroups[note.id] || note.itemTitle?.trim() || "未分类";
      groups.set(group, [...(groups.get(group) ?? []), note]);
    }
    return Array.from(groups.entries());
  }, [filteredNotes, noteGroups]);
  const stats = useMemo(() => noteStats(content), [content]);

  const showSourceSelection = (event: ReactMouseEvent<HTMLTextAreaElement>) => {
    const editor = event.currentTarget;
    const selectedText = editor.value.slice(editor.selectionStart, editor.selectionEnd).trim();
    if (!selectedText) {
      setSelectionMenu(null);
      return;
    }
    const rect = editor.getBoundingClientRect();
    setSelectionMenu({
      left: Math.min(rect.width - 170, Math.max(12, event.clientX - rect.left)),
      top: Math.min(rect.height - 42, Math.max(12, event.clientY - rect.top + 10)),
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
    const hostRect = host.getBoundingClientRect();
    setSelectionMenu({
      left: Math.min(hostRect.width - 170, Math.max(12, event.clientX - hostRect.left)),
      top: Math.min(hostRect.height - 42, Math.max(12, event.clientY - hostRect.top + 10)),
      text: text.slice(0, MAX_SELECTION_CHARACTERS),
    });
  };

  const askSelection = () => {
    if (!activeNote || !selectionMenu) return;
    onAskSelection({
      id: crypto.randomUUID(),
      noteId: activeNote.id,
      noteTitle: title.trim() || activeNote.title,
      revisionHash: revisionHash({ ...activeNote, title, content }),
      startLine: selectionMenu.startLine,
      endLine: selectionMenu.endLine,
      selectedText: selectionMenu.text,
    });
    setSelectionMenu(null);
    window.getSelection()?.removeAllRanges();
  };

  if (!activeNote) {
    return (
      <section className="notes-browser" aria-label="Markdown 笔记库">
        <header className="notes-browser-toolbar">
          <button type="button" className="notes-back-button" onClick={onBack}>
            <ArrowLeft size={16} />
            <span>返回 Chat</span>
          </button>
          <strong>笔记</strong>
          <button type="button" className="notes-create-button" onClick={() => void createNote()}>
            <FilePlus2 size={16} />
            <span>新建 Markdown 笔记</span>
          </button>
          <button type="button" className="notes-back-button" onClick={() => void importNotes()}>
            <FileText size={15} />
            <span>导入 Markdown</span>
          </button>
          <button type="button" className="notes-back-button" onClick={createGroup}>
            <Folder size={15} />
            <span>新建分组</span>
          </button>
        </header>
        <div className="notes-browser-tools">
          <label className="notes-search">
            <Search size={14} />
            <input value={query} placeholder="搜索笔记" onChange={(event) => setQuery(event.target.value)} />
          </label>
          <span className="notes-browser-count">{filteredNotes.length} 篇笔记</span>
        </div>
        {error ? <div className="notes-error" role="alert">{error}</div> : null}
        {loading ? (
          <div className="notes-empty" role="status"><LoaderCircle className="is-spinning" size={24} />正在加载笔记</div>
        ) : groupedNotes.length === 0 ? (
          <div className="notes-empty"><FilePlus2 size={32} /><strong>还没有 Markdown 笔记</strong><button type="button" onClick={() => void createNote()}>新建笔记</button></div>
        ) : (
          <div className="notes-browser-list" role="list">
            {groupedNotes.map(([group, groupNotes]) => (
              <section className="notes-browser-group" key={group}>
                <header><Folder size={15} /><strong>{group}</strong><span>{groupNotes.length}</span></header>
                {groupNotes.map((note) => (
                  <div className="notes-browser-item" role="listitem" key={note.id}>
                    <button type="button" className="notes-browser-item-open" onClick={() => void openNote(note.id)}>
                    <FileText size={17} />
                    <span><strong>{note.title}</strong><small>{note.contentPreview || "空白笔记"}</small></span>
                    </button>
                    <select value={noteGroups[note.id] || "未分类"} aria-label={`${note.title} 所属分组`} onChange={(event) => assignGroup(note.id, event.target.value)}>
                      <option value="未分类">未分类</option>
                      {customGroups.map((group) => <option value={group} key={group}>{group}</option>)}
                    </select>
                    <time>{new Intl.DateTimeFormat("zh-CN", { month: "2-digit", day: "2-digit" }).format(note.updatedAt)}</time>
                  </div>
                ))}
              </section>
            ))}
          </div>
        )}
      </section>
    );
  }

  return (
    <section className="notes-workspace" aria-label="笔记工作区">
      <main className="notes-editor-pane">
        <header className="notes-toolbar">
          <button type="button" className="notes-back-button" title="返回笔记列表" aria-label="返回笔记列表" onClick={() => { void flushActiveDraft().then(() => { setActiveNote(null); setTitle(""); setContent(""); setSelectionMenu(null); }); }}>
            <ArrowLeft size={16} />
            <span>笔记</span>
          </button>
          <div className="notes-title-wrap">
            <FileCode2 size={16} />
            <input
              value={title}
              disabled={!activeNote}
              aria-label="笔记标题"
              onChange={(event) => setTitle(event.target.value)}
            />
          </div>
          <div className="notes-toolbar-actions">
            <button
              className="notes-mode-toggle"
              type="button"
              disabled={!activeNote}
              title={mode === "source" ? "切换为预览" : "切换为 Markdown 源码"}
              aria-label={mode === "source" ? "切换为预览" : "切换为 Markdown 源码"}
              onClick={() => { setSelectionMenu(null); setMode((current) => current === "source" ? "preview" : "source"); }}
            >
              {mode === "source" ? <Eye size={16} /> : <FileCode2 size={16} />}
              <span>{mode === "source" ? "预览" : "Markdown"}</span>
            </button>
            <button type="button" disabled={!activeNote} title="删除笔记" aria-label="删除笔记" onClick={() => void removeNote()}>
              <Trash2 size={16} />
            </button>
            <button
              className={chatOpen ? "is-active" : ""}
              type="button"
              title={chatOpen ? "收起 AI" : "打开 AI"}
              aria-label={chatOpen ? "收起 AI" : "打开 AI"}
              onClick={onToggleChat}
            >
              {chatBusy ? <LoaderCircle className="is-spinning" size={16} /> : chatOpen ? <Bot size={16} /> : <PanelRightOpen size={16} />}
            </button>
          </div>
        </header>

        {error ? <div className="notes-error" role="alert">{error}</div> : null}
        {loading ? (
          <div className="notes-empty" role="status"><LoaderCircle className="is-spinning" size={24} />正在加载笔记</div>
        ) : (
          <div className="notes-document-host">
            {mode === "source" ? (
              <textarea
                ref={editorRef}
                className="notes-source-editor"
                value={content}
                spellCheck={false}
                aria-label="Markdown 源码编辑器"
                onChange={(event) => setContent(event.target.value)}
                onMouseUp={showSourceSelection}
                onKeyUp={() => setSelectionMenu(null)}
              />
            ) : (
              <div ref={previewRef} className="notes-preview-host" onMouseUp={showPreviewSelection}>
                <Suspense fallback={<div className="notes-empty"><LoaderCircle className="is-spinning" size={20} />正在加载预览</div>}>
                  <MarkdownNotePreview noteId={activeNote.id} content={content} />
                </Suspense>
              </div>
            )}
            {selectionMenu ? (
              <div
                className="notes-selection-menu"
                style={{ left: selectionMenu.left, top: selectionMenu.top }}
                onMouseDown={(event) => event.preventDefault()}
              >
                <button type="button" onClick={() => { void navigator.clipboard?.writeText(selectionMenu.text); setSelectionMenu(null); }}><Copy size={13} />复制</button>
                <button type="button" onClick={askSelection}><Quote size={13} />引用提问</button>
              </div>
            ) : null}
          </div>
        )}
        <footer className="notes-statusbar">
          <span>{mode === "source" ? "Markdown 源码" : "渲染预览"}</span>
          <span>{saving ? "自动保存中" : saved ? <><Check size={12} />已保存</> : "自动保存"}</span>
          <span>{stats.words} 词</span>
          <span>{stats.characters} 字符</span>
          <span>约 {stats.readingMinutes} 分钟</span>
        </footer>
      </main>
    </section>
  );
}
