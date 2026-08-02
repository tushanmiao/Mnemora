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
  FolderPlus,
  Inbox,
  LoaderCircle,
  ListTree,
  MoreHorizontal,
  NotebookText,
  PanelRightOpen,
  Quote,
  Search,
  Trash2,
} from "lucide-react";
import {
  createLibraryNote,
  createLibraryNoteGroup,
  deleteLibraryNote,
  deleteLibraryNoteGroup,
  getLibraryNote,
  chooseLibraryMarkdownFiles,
  importLibraryMarkdownNotes,
  isLibraryRuntime,
  listLibraryNoteGroups,
  listLibraryNotes,
  setLibraryNoteGroup,
  updateLibraryNote,
} from "../../library/api/library";
import type { LibraryNote, LibraryNoteGroup, LibraryNoteSummary } from "../../library/types";
import type { NoteReference } from "../../../types/chat";
import {
  extractMarkdownOutline,
  type MarkdownOutlineItem,
} from "../../chat/markdown/utils/outline";
import { PanelResizeHandle } from "../../layout/components/PanelResizeHandle";
import "../styles/notes-workspace.css";

const MarkdownNotePreview = lazy(() => import("./MarkdownNotePreview"));
const AUTOSAVE_DELAY_MS = 700;
const MAX_SELECTION_CHARACTERS = 16_000;
/** v4 之前分组存放在 localStorage；首次进入时一次性迁入 SQLite 后移除。 */
const LEGACY_NOTE_GROUPS_STORAGE_KEY = "mnemora.notes.groups.v1";
const LEGACY_CUSTOM_GROUPS_STORAGE_KEY = "mnemora.notes.custom-groups.v1";
/** 大纲栏宽度与开关的本地持久化。 */
const NOTES_LAYOUT_STORAGE_KEY = "mnemora.notes.layout.v1";
const OUTLINE_DEFAULT_WIDTH = 232;
const OUTLINE_MIN_WIDTH = 168;
const OUTLINE_MAX_WIDTH = 440;

type NotesLayout = { outlineWidth: number; outlineOpen: boolean };

function loadNotesLayout(): NotesLayout {
  const fallback: NotesLayout = { outlineWidth: OUTLINE_DEFAULT_WIDTH, outlineOpen: true };
  try {
    const parsed: unknown = JSON.parse(
      window.localStorage.getItem(NOTES_LAYOUT_STORAGE_KEY) ?? "{}",
    );
    if (!parsed || typeof parsed !== "object") return fallback;
    const candidate = parsed as Partial<NotesLayout>;
    const width = typeof candidate.outlineWidth === "number" && Number.isFinite(candidate.outlineWidth)
      ? Math.min(Math.max(candidate.outlineWidth, OUTLINE_MIN_WIDTH), OUTLINE_MAX_WIDTH)
      : OUTLINE_DEFAULT_WIDTH;
    return { outlineWidth: width, outlineOpen: candidate.outlineOpen !== false };
  } catch {
    return fallback;
  }
}

function persistNotesLayout(layout: NotesLayout) {
  try {
    window.localStorage.setItem(NOTES_LAYOUT_STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // 本地存储不可用时布局仅在当前会话内生效。
  }
}

/**
 * 会话内记住最后打开的笔记。组件可能被 Suspense 或路由切换卸载重挂，
 * 重挂后凭此恢复编辑现场；用户主动返回列表时会同步清空，不会误恢复。
 */
let lastOpenNoteId: string | null = null;

type NotesWorkspaceProps = {
  chatOpen: boolean;
  chatBusy: boolean;
  userDisplayName: string;
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

type GroupFilter = "all" | "unfiled" | { group: string };

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

function formatNoteSize(characters: number) {
  if (characters >= 10_000) return `${(characters / 10_000).toFixed(1)} 万字`;
  return `${characters} 字`;
}

const NOTE_TIME_FORMATTER = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
});

/** 旧版 localStorage 分组一次性迁入 SQLite；成败都不阻塞页面。 */
async function migrateLegacyGroups() {
  let rawGroups: string | null = null;
  let rawAssignments: string | null = null;
  try {
    rawGroups = window.localStorage.getItem(LEGACY_CUSTOM_GROUPS_STORAGE_KEY);
    rawAssignments = window.localStorage.getItem(LEGACY_NOTE_GROUPS_STORAGE_KEY);
  } catch {
    return;
  }
  if (!rawGroups && !rawAssignments) return;
  const names = new Set<string>();
  const assignments: Array<[string, string]> = [];
  try {
    const parsed: unknown = JSON.parse(rawGroups ?? "[]");
    if (Array.isArray(parsed)) {
      for (const value of parsed) {
        if (typeof value === "string" && value.trim()) names.add(value.trim());
      }
    }
  } catch { /* 忽略损坏的旧数据 */ }
  try {
    const parsed: unknown = JSON.parse(rawAssignments ?? "{}");
    if (parsed && typeof parsed === "object") {
      for (const [noteId, group] of Object.entries(parsed)) {
        if (typeof group === "string" && group.trim() && group !== "未分类") {
          names.add(group.trim());
          assignments.push([noteId, group.trim()]);
        }
      }
    }
  } catch { /* 忽略损坏的旧数据 */ }
  for (const name of names) {
    try { await createLibraryNoteGroup(name); } catch { /* 已存在则跳过 */ }
  }
  for (const [noteId, group] of assignments) {
    try { await setLibraryNoteGroup(noteId, group); } catch { /* 笔记可能已删除 */ }
  }
  try {
    window.localStorage.removeItem(LEGACY_CUSTOM_GROUPS_STORAGE_KEY);
    window.localStorage.removeItem(LEGACY_NOTE_GROUPS_STORAGE_KEY);
  } catch { /* 移除失败下次会重试，迁移本身幂等 */ }
}

export default function NotesWorkspace({
  chatOpen,
  chatBusy,
  userDisplayName,
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
  const [groups, setGroups] = useState<LibraryNoteGroup[]>([]);
  const [groupFilter, setGroupFilter] = useState<GroupFilter>("all");
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
  /** 列表行三点菜单当前打开的笔记 ID。 */
  const [rowMenu, setRowMenu] = useState<string | null>(null);
  const workspaceRef = useRef<HTMLElement>(null);
  const [notesLayout, setNotesLayout] = useState<NotesLayout>(loadNotesLayout);
  activeNoteRef.current = activeNote;
  titleRef.current = title;
  contentRef.current = content;

  // 拖动期间只写 CSS 变量，松手才提交 state + 持久化，避免每个 move 都重渲染。
  const previewOutlineWidth = useCallback((width: number) => {
    workspaceRef.current?.style.setProperty("--notes-outline-width", `${width}px`);
  }, []);
  const commitOutlineWidth = useCallback((width: number) => {
    setNotesLayout((current) => {
      const next = { ...current, outlineWidth: width };
      persistNotesLayout(next);
      return next;
    });
  }, []);
  const toggleOutline = useCallback(() => {
    setNotesLayout((current) => {
      const next = { ...current, outlineOpen: !current.outlineOpen };
      persistNotesLayout(next);
      return next;
    });
  }, []);

  useEffect(() => {
    lastOpenNoteId = activeNote?.id ?? null;
  }, [activeNote?.id]);

  const creatorName = userDisplayName.trim() || "Mnemora 用户";

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

  const refreshGroups = useCallback(async () => {
    const next = await listLibraryNoteGroups();
    if (mountedRef.current) setGroups(next);
  }, []);

  const loadNotes = useCallback(async (preferredId?: string, quiet = false) => {
    setLoading(true);
    setError("");
    try {
      const [nextNotes] = await Promise.all([
        listLibraryNotes().then((all) => all.filter((note) => note.itemId === null)),
        refreshGroups(),
      ]);
      setNotes(nextNotes);
      if (!preferredId) {
        setActiveNote(null);
        setTitle("");
        setContent("");
        return;
      }
      try {
        const note = await getLibraryNote(preferredId);
        setActiveNote(note);
        setTitle(note.title);
        setContent(note.content);
      } catch (openError) {
        // 恢复场景（如笔记刚被删除）静默回到列表；主动打开的失败仍然提示。
        setActiveNote(null);
        setTitle("");
        setContent("");
        if (!quiet) throw openError;
      }
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      setLoading(false);
    }
  }, [refreshGroups]);

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
    void (async () => {
      if (isLibraryRuntime()) await migrateLegacyGroups();
      await loadNotes(lastOpenNoteId ?? undefined, true);
    })();
    return () => {
      mountedRef.current = false;
      clearSaveTimer();
      if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
      const note = activeNoteRef.current;
      if (note && (titleRef.current !== note.title || contentRef.current !== note.content)) {
        void queueSave(note, titleRef.current, contentRef.current);
      }
    };
    // 仅挂载时执行一次；loadNotes 的依赖都是稳定引用。
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
        groupName: typeof groupFilter === "object" ? groupFilter.group : null,
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

  const closeNote = () => {
    void flushActiveDraft().then(() => {
      setActiveNote(null);
      setTitle("");
      setContent("");
      setSelectionMenu(null);
    });
  };

  const removeNote = async () => {
    if (!activeNote || !window.confirm(`删除笔记“${activeNote.title}”吗？`)) return;
    try {
      clearSaveTimer();
      await saveChainRef.current.catch(() => undefined);
      await deleteLibraryNote(activeNote.id);
      setActiveNote(null);
      setTitle("");
      setContent("");
      await loadNotes();
    } catch (deleteError) {
      setError(deleteError instanceof Error ? deleteError.message : String(deleteError));
    }
  };

  // 点击行菜单外的任意位置收起；菜单和三点按钮自身通过 stopPropagation 幸免。
  useEffect(() => {
    if (!rowMenu) return;
    const hide = () => setRowMenu(null);
    document.addEventListener("mousedown", hide);
    return () => document.removeEventListener("mousedown", hide);
  }, [rowMenu]);

  /** 列表行内删除：不经过编辑器，删除后刷新列表与分组计数。 */
  const removeNoteFromList = async (note: LibraryNoteSummary) => {
    setRowMenu(null);
    if (!window.confirm(`删除笔记“${note.title}”吗？`)) return;
    setError("");
    try {
      await deleteLibraryNote(note.id);
      await loadNotes();
    } catch (deleteError) {
      setError(deleteError instanceof Error ? deleteError.message : String(deleteError));
    }
  };

  const createGroup = async () => {
    const name = window.prompt("请输入分组名称")?.trim();
    if (!name) return;
    try {
      await createLibraryNoteGroup(name);
      await refreshGroups();
      setGroupFilter({ group: name });
    } catch (groupError) {
      setError(groupError instanceof Error ? groupError.message : String(groupError));
    }
  };

  const removeGroup = async (name: string) => {
    if (!window.confirm(`删除分组“${name}”吗？组内笔记会回到未分类。`)) return;
    try {
      await deleteLibraryNoteGroup(name);
      setGroupFilter((current) => (
        typeof current === "object" && current.group === name ? "all" : current
      ));
      await loadNotes();
    } catch (groupError) {
      setError(groupError instanceof Error ? groupError.message : String(groupError));
    }
  };

  const assignGroup = async (noteId: string, groupName: string | null) => {
    try {
      const updated = await setLibraryNoteGroup(noteId, groupName);
      setNotes((current) => current.map((item) => (
        item.id === updated.id ? { ...item, groupName: updated.groupName } : item
      )));
      await refreshGroups();
    } catch (groupError) {
      setError(groupError instanceof Error ? groupError.message : String(groupError));
    }
  };

  const filteredNotes = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return notes.filter((note) => {
      if (groupFilter === "unfiled" && note.groupName) return false;
      if (typeof groupFilter === "object" && note.groupName !== groupFilter.group) return false;
      if (!normalized) return true;
      return [note.title, note.contentPreview]
        .some((value) => value.toLocaleLowerCase().includes(normalized));
    });
  }, [notes, query, groupFilter]);
  const unfiledCount = useMemo(
    () => notes.filter((note) => !note.groupName).length,
    [notes],
  );
  const stats = useMemo(() => noteStats(content), [content]);

  // 大纲随输入实时更新；用 deferred 值避免大文档下每次击键都同步重算。
  const deferredContent = useDeferredValue(content);
  const outline = useMemo<MarkdownOutlineItem[]>(
    () => (activeNote ? extractMarkdownOutline(deferredContent, `note-${activeNote.id}`) : []),
    [activeNote, deferredContent],
  );

  const jumpToOutlineItem = (item: MarkdownOutlineItem) => {
    if (mode === "preview") {
      document.getElementById(item.id)?.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
    const editor = editorRef.current;
    if (!editor) return;
    const style = window.getComputedStyle(editor);
    const lineHeight = Number.parseFloat(style.lineHeight) || 24;
    const paddingTop = Number.parseFloat(style.paddingTop) || 0;
    const line = lineAtOffset(content, item.offset);
    editor.focus({ preventScroll: true });
    editor.setSelectionRange(item.offset, item.offset);
    editor.scrollTop = Math.max(0, (line - 1) * lineHeight + paddingTop - 16);
  };

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
        </header>
        <div className="notes-browser-body">
          <aside className="notes-groups-nav" aria-label="笔记分组">
            <header>
              <strong>分组</strong>
              <button type="button" title="新建分组" aria-label="新建分组" onClick={() => void createGroup()}>
                <FolderPlus size={15} />
              </button>
            </header>
            <nav>
              <button
                type="button"
                className={groupFilter === "all" ? "notes-group-item is-active" : "notes-group-item"}
                onClick={() => setGroupFilter("all")}
              >
                <NotebookText size={15} />
                <span>全部笔记</span>
                <small>{notes.length}</small>
              </button>
              <button
                type="button"
                className={groupFilter === "unfiled" ? "notes-group-item is-active" : "notes-group-item"}
                onClick={() => setGroupFilter("unfiled")}
              >
                <Inbox size={15} />
                <span>未分类</span>
                <small>{unfiledCount}</small>
              </button>
              {groups.map((group) => (
                <div
                  className={typeof groupFilter === "object" && groupFilter.group === group.name
                    ? "notes-group-row is-active"
                    : "notes-group-row"}
                  key={group.name}
                >
                  <button
                    type="button"
                    className="notes-group-item"
                    onClick={() => setGroupFilter({ group: group.name })}
                  >
                    <Folder size={15} />
                    <span>{group.name}</span>
                    <small>{group.noteCount}</small>
                  </button>
                  <button
                    type="button"
                    className="notes-group-remove"
                    title={`删除分组 ${group.name}`}
                    aria-label={`删除分组 ${group.name}`}
                    onClick={() => void removeGroup(group.name)}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              ))}
            </nav>
          </aside>
          <div className="notes-browser-main">
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
            ) : filteredNotes.length === 0 ? (
              <div className="notes-empty">
                <FilePlus2 size={32} />
                <strong>{notes.length === 0 ? "还没有 Markdown 笔记" : "该分组下暂无笔记"}</strong>
                <button type="button" onClick={() => void createNote()}>新建笔记</button>
              </div>
            ) : (
              <div className="notes-table" role="table" aria-label="笔记列表">
                <div className="notes-table-head" role="row">
                  <span role="columnheader">标题</span>
                  <span role="columnheader">创建者</span>
                  <span role="columnheader">最后修改</span>
                  <span role="columnheader">分组</span>
                  <span role="columnheader">大小</span>
                  <span role="columnheader" aria-label="操作" />
                </div>
                <div className="notes-table-body" role="rowgroup">
                  {filteredNotes.map((note) => (
                    <div className="notes-table-row" role="row" key={note.id}>
                      <button
                        type="button"
                        className="notes-table-title"
                        role="cell"
                        title={note.contentPreview || "空白笔记"}
                        onClick={() => void openNote(note.id)}
                      >
                        <FileText size={16} />
                        <span>{note.title}</span>
                      </button>
                      <span role="cell" className="notes-table-muted" title={creatorName}>{creatorName}</span>
                      <span role="cell" className="notes-table-muted">
                        {NOTE_TIME_FORMATTER.format(note.updatedAt)}
                      </span>
                      <span role="cell">
                        <select
                          value={note.groupName ?? ""}
                          aria-label={`${note.title} 所属分组`}
                          onChange={(event) => void assignGroup(note.id, event.target.value || null)}
                        >
                          <option value="">未分类</option>
                          {groups.map((group) => (
                            <option value={group.name} key={group.name}>{group.name}</option>
                          ))}
                        </select>
                      </span>
                      <span role="cell" className="notes-table-muted">{formatNoteSize(note.contentChars)}</span>
                      <span role="cell" className="notes-table-actions">
                        <button
                          type="button"
                          className="notes-table-more"
                          title="更多操作"
                          aria-label={`${note.title} 更多操作`}
                          aria-expanded={rowMenu === note.id}
                          onMouseDown={(event) => event.stopPropagation()}
                          onClick={() => setRowMenu((current) => (current === note.id ? null : note.id))}
                        >
                          <MoreHorizontal size={15} />
                        </button>
                        {rowMenu === note.id ? (
                          <div
                            className="notes-row-menu"
                            role="menu"
                            onMouseDown={(event) => event.stopPropagation()}
                          >
                            <button
                              type="button"
                              role="menuitem"
                              className="notes-row-menu-danger"
                              onClick={() => void removeNoteFromList(note)}
                            >
                              <Trash2 size={14} />
                              <span>删除笔记</span>
                            </button>
                          </div>
                        ) : null}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      </section>
    );
  }

  return (
    <section
      className="notes-workspace"
      aria-label="笔记工作区"
      ref={workspaceRef}
      data-outline={notesLayout.outlineOpen ? "open" : "closed"}
      style={{ "--notes-outline-width": `${notesLayout.outlineWidth}px` } as CSSProperties}
    >
      {notesLayout.outlineOpen ? (
        <aside className="notes-outline-pane" aria-label="笔记大纲">
          <header>
            <ListTree size={14} />
            <strong>大纲</strong>
            <span>{outline.length}</span>
          </header>
          <div>
            {outline.length === 0 ? (
              <p>没有检测到标题。使用 “#” 开头的标题行会出现在这里。</p>
            ) : (
              outline.map((item) => (
                <button
                  type="button"
                  key={item.id}
                  style={{ paddingLeft: `${10 + (item.level - 1) * 13}px` }}
                  title={item.title}
                  onClick={() => jumpToOutlineItem(item)}
                >
                  {item.title}
                </button>
              ))
            )}
          </div>
          <PanelResizeHandle
            edge="right"
            value={notesLayout.outlineWidth}
            defaultValue={OUTLINE_DEFAULT_WIDTH}
            minValue={OUTLINE_MIN_WIDTH}
            maxValue={OUTLINE_MAX_WIDTH}
            label="调整大纲宽度"
            onPreview={previewOutlineWidth}
            onCommit={commitOutlineWidth}
          />
        </aside>
      ) : null}
      <main className="notes-editor-pane">
        <header className="notes-toolbar">
          <button type="button" className="notes-back-button" title="返回笔记列表" aria-label="返回笔记列表" onClick={closeNote}>
            <ArrowLeft size={16} />
            <span>笔记</span>
          </button>
          <div className="notes-title-wrap">
            <FileCode2 size={16} />
            <input
              value={title}
              aria-label="笔记标题"
              onChange={(event) => setTitle(event.target.value)}
            />
          </div>
          <div className="notes-toolbar-actions">
            <button
              type="button"
              className={notesLayout.outlineOpen ? "is-active" : ""}
              title={notesLayout.outlineOpen ? "收起大纲" : "展开大纲"}
              aria-label={notesLayout.outlineOpen ? "收起大纲" : "展开大纲"}
              aria-pressed={notesLayout.outlineOpen}
              onClick={toggleOutline}
            >
              <ListTree size={16} />
            </button>
            <div className="notes-mode-tabs" role="tablist" aria-label="编辑模式">
              <button
                type="button"
                role="tab"
                aria-selected={mode === "source"}
                className={mode === "source" ? "is-active" : ""}
                onClick={() => { setSelectionMenu(null); setMode("source"); }}
              >
                <FileCode2 size={14} />
                <span>Markdown</span>
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={mode === "preview"}
                className={mode === "preview" ? "is-active" : ""}
                onClick={() => { setSelectionMenu(null); setMode("preview"); }}
              >
                <Eye size={14} />
                <span>预览</span>
              </button>
            </div>
            <button type="button" title="删除笔记" aria-label="删除笔记" onClick={() => void removeNote()}>
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
