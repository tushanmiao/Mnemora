import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
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
  renameLibraryNote,
  setLibraryNoteGroup,
} from "../../library/api/library";
import type { LibraryNote, LibraryNoteGroup, LibraryNoteSummary } from "../../library/types";
import type { NoteReference } from "../../../types/chat";
import type { MarkdownOutlineItem } from "../../chat/markdown/utils/outline";
import { noteOutline } from "../editor/markdownRanges";
import { getNoteEditSession, useNoteEditSession } from "../runtime/noteEditSession";
import { prepareNoteSelection } from "../runtime/noteSelection";
import type { NoteEditorMode } from "../api/noteEditing";
import { getNoteEditorPreferences } from "../runtime/noteEditorPreferences";
import { NotesBrowser, type GroupFilter, type NoteSort } from "./NotesBrowser";
import { chooseLocalNoteSourceFiles } from "../api/localNoteSource";
import { NoteEditor, type NoteSelectionMenu } from "./NoteEditor";
import type { MarkdownSourceEditorHandle } from "./MarkdownSourceEditor";
import {
  lineAtOffset,
  loadNotesLayout,
  noteStats,
  persistNotesLayout,
  type NotesLayout,
} from "../utils/notesWorkspace";
import "../styles/notes-workspace.css";

const MAX_SELECTION_CHARACTERS = 16_000;
/** v4 之前分组存放在 localStorage；首次进入时一次性迁入 SQLite 后移除。 */
const LEGACY_NOTE_GROUPS_STORAGE_KEY = "mnemora.notes.groups.v1";
const LEGACY_CUSTOM_GROUPS_STORAGE_KEY = "mnemora.notes.custom-groups.v1";
const NOTE_SORT_STORAGE_KEY = "mnemora.notes.sort.v1";
/**
 * 会话内记住最后打开的笔记。组件可能被 Suspense 或路由切换卸载重挂，
 * 重挂后凭此恢复编辑现场；用户主动返回列表时会同步清空，不会误恢复。
 */
let lastOpenNoteId: string | null = null;

export type NotesWorkspaceProps = {
  chatOpen: boolean;
  chatBusy: boolean;
  userDisplayName: string;
  onToggleChat: () => void;
  onAskSelection: (reference: NoteReference) => void;
  onEditSelection: (selection: {
    noteId: string;
    selectedText: string;
    sectionHeading: string;
  }) => void;
  onBack: () => void;
  onOpenSourceConversation?: (conversationId: string, messageId: string | null) => void;
  onGenerateFromLocalFiles?: (paths: string[]) => Promise<void>;
};

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
  onToggleChat,
  onAskSelection,
  onEditSelection,
  onBack,
  onOpenSourceConversation,
  onGenerateFromLocalFiles,
}: NotesWorkspaceProps) {
  const editorRef = useRef<MarkdownSourceEditorHandle>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const saveChainRef = useRef<Promise<void>>(Promise.resolve());
  const activeNoteRef = useRef<LibraryNote | null>(null);
  const titleRef = useRef("");
  const contentRef = useRef("");
  const mountedRef = useRef(true);
  const [notes, setNotes] = useState<LibraryNoteSummary[]>([]);
  const [groups, setGroups] = useState<LibraryNoteGroup[]>([]);
  const [groupFilter, setGroupFilter] = useState<GroupFilter>("all");
  const [activeNote, setActiveNote] = useState<LibraryNote | null>(null);
  const document = useNoteEditSession(activeNote?.id ?? null);
  const title = document.base ? document.title : activeNote?.title ?? "";
  const content = document.base ? document.content : activeNote?.content ?? "";
  const setTitle = (title: string) => document.session?.edit({ title });
  const setContent = (content: string) => document.session?.edit({ content });
  const [query, setQuery] = useState("");
  const [sort, setSortState] = useState<NoteSort>(loadNoteSort);
  const [mode, setMode] = useState<NoteEditorMode>(() => getNoteEditorPreferences().defaultMode);
  const [loading, setLoading] = useState(true);
  const saving = document.phase === "saving";
  const saved = !!document.base && !document.session?.dirty;
  const [error, setError] = useState("");
  const [selectionMenu, setSelectionMenu] = useState<NoteSelectionMenu | null>(null);
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

  const clearSaveTimer = useCallback(() => activeNoteRef.current && getNoteEditSession(activeNoteRef.current.id).releaseTimers(), []);

  const queueSave = useCallback((note: LibraryNote, nextTitle: string, nextContent: string) => {
    const session = getNoteEditSession(note.id);
    session.edit({ title: nextTitle, content: nextContent });
    const operation = session.save().then(() => {
        const updated = session.snapshot().base!.note;
        if (!mountedRef.current || activeNoteRef.current?.id !== updated.id) return;

        setActiveNote(updated);
        setNotes((current) => current.map((item) => item.id === updated.id ? {
          ...item,
          title: updated.title,
          contentPreview: updated.content.slice(0, 600),
          contentChars: Array.from(updated.content).length,
          contentBytes: new TextEncoder().encode(updated.content).byteLength,
          updatedAt: updated.updatedAt,
        } : item));

      })
      .catch((saveError) => {
        if (mountedRef.current) {
          setError(saveError instanceof Error ? saveError.message : String(saveError));
        }
        throw saveError;
      });
    saveChainRef.current = operation;
    return operation;
  }, []);

  const flushActiveDraft = useCallback(() => {
    clearSaveTimer();
    const note = activeNoteRef.current;
    if (!note || (titleRef.current === note.title && contentRef.current === note.content)) {
      return saveChainRef.current;
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
        return;
      }
      try {
        const note = await getLibraryNote(preferredId);
        setActiveNote(note);
      } catch (openError) {
        // 恢复场景（如笔记刚被删除）静默回到列表；主动打开的失败仍然提示。
        setActiveNote(null);
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

  const generateFromLocalFiles = useCallback(async () => {
    if (!onGenerateFromLocalFiles) return;
    const paths = await chooseLocalNoteSourceFiles();
    if (paths.length === 0) return;
    setLoading(true);
    setError("");
    try {
      await onGenerateFromLocalFiles(paths);
    } catch (generateError) {
      setError(generateError instanceof Error ? generateError.message : String(generateError));
    } finally {
      setLoading(false);
    }
  }, [onGenerateFromLocalFiles]);

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
      const note = activeNoteRef.current;
      if (note && (titleRef.current !== note.title || contentRef.current !== note.content)) {
        void getNoteEditSession(note.id).checkpoint().catch(() => undefined);
      }
    };
    // 仅挂载时执行一次；loadNotes 的依赖都是稳定引用。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clearSaveTimer, queueSave]);

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
      setMode("live");
      window.setTimeout(() => editorRef.current?.focus(), 0);
    } catch (createError) {
      setError(createError instanceof Error ? createError.message : String(createError));
    }
  };

  const openNote = async (noteId: string) => {
    if (activeNote?.id === noteId || saving) return;
    setSelectionMenu(null);
    setLoading(true);
    try {
      await flushActiveDraft();
      const note = await getLibraryNote(noteId);
      setActiveNote(note);
    } catch (openError) {
      setError(openError instanceof Error ? openError.message : String(openError));
    } finally {
      setLoading(false);
    }
  };

  const closeNote = () => {
    void flushActiveDraft().then(() => {
      setActiveNote(null);
      setSelectionMenu(null);
    }).catch((error: unknown) => setError(String(error)));
  };

  const removeNote = async () => {
    if (!activeNote || !window.confirm(`删除笔记“${activeNote.title}”吗？`)) return;
    try {
      clearSaveTimer();
      await saveChainRef.current.catch(() => undefined);
      await deleteLibraryNote(activeNote.id);
      setActiveNote(null);
      await loadNotes();
    } catch (deleteError) {
      setError(deleteError instanceof Error ? deleteError.message : String(deleteError));
    }
  };

  // 点击行菜单外的任意位置收起；菜单和三点按钮自身通过 stopPropagation 幸免。
  useEffect(() => {
    if (!rowMenu) return;
    const hide = () => setRowMenu(null);
    window.document.addEventListener("mousedown", hide);
    return () => window.document.removeEventListener("mousedown", hide);
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

  const renameNoteFromList = async (note: LibraryNoteSummary, nextTitle: string) => {
    setRowMenu(null);
    setError("");
    try {
      const updated = await renameLibraryNote({ noteId: note.id, title: nextTitle });
      setNotes((current) => current.map((item) => item.id === updated.id ? {
        ...item,
        title: updated.title,
        updatedAt: updated.updatedAt,
      } : item));
      if (activeNoteRef.current?.id === updated.id) {
        setActiveNote(updated);
        void getNoteEditSession(updated.id).load();
      }
      return true;
    } catch (renameError) {
      setError(renameError instanceof Error ? renameError.message : String(renameError));
      return false;
    }
  };

  const setSort = (next: NoteSort) => {
    setSortState(next);
    try { window.localStorage.setItem(NOTE_SORT_STORAGE_KEY, next); } catch { /* 本次会话仍然生效 */ }
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
    const filtered = notes.filter((note) => {
      if (groupFilter === "unfiled" && note.groupName) return false;
      if (typeof groupFilter === "object" && note.groupName !== groupFilter.group) return false;
      if (!normalized) return true;
      return [note.title, note.contentPreview]
        .some((value) => value.toLocaleLowerCase().includes(normalized));
    });
    return filtered.sort((left, right) => compareNotes(left, right, sort));
  }, [notes, query, groupFilter, sort]);
  const unfiledCount = useMemo(
    () => notes.filter((note) => !note.groupName).length,
    [notes],
  );
  const stats = useMemo(() => noteStats(content), [content]);

  // 大纲随输入实时更新；用 deferred 值避免大文档下每次击键都同步重算。
  const deferredContent = useDeferredValue(content);
  const outline = useMemo<MarkdownOutlineItem[]>(
    () => (activeNote ? noteOutline(deferredContent, `note-${activeNote.id}`) : []),
    [activeNote, deferredContent],
  );

  const jumpToOutlineItem = (item: MarkdownOutlineItem) => {
    if (mode === "read") {
      window.document.getElementById(item.id)?.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
    const editor = editorRef.current;
    if (!editor) return;
    const line = lineAtOffset(content, item.offset);
    editor.focus();
    editor.setSelection(item.offset, item.offset);
    editor.scrollToLine(line);
  };

  const showSourceSelection = (event: ReactMouseEvent<HTMLElement>) => {
    const editor = editorRef.current;
    if (!editor) return;
    const selected = editor.getSelection();
    const selectedText = selected.text.trim();
    if (!selectedText) {
      setSelectionMenu(null);
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    setSelectionMenu({
      left: Math.min(rect.width - 170, Math.max(12, event.clientX - rect.left)),
      top: Math.min(rect.height - 42, Math.max(12, event.clientY - rect.top + 10)),
      text: selectedText.slice(0, MAX_SELECTION_CHARACTERS),
      startLine: lineAtOffset(editor.getText(), selected.from),
      endLine: lineAtOffset(editor.getText(), selected.to),
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

  const askSelection = async () => {
    if (!activeNote || !selectionMenu || !document.session) return;
    try { onAskSelection(await prepareNoteSelection(document.session, selectionMenu.text)); }
    catch (error) { setError(String(error)); return; }
    setSelectionMenu(null);
    window.getSelection()?.removeAllRanges();
  };

  const editSelection = async () => {
    if (!activeNote || !selectionMenu) return;
    const generation = document.generation;
    try { await flushActiveDraft(); } catch { return; }
    if (document.session?.snapshot().generation !== generation) { setError("笔记已变化，请重新选择修改范围。"); return; }
    const lines = content.split(/\r?\n/);
    const beforeSelection = selectionMenu.startLine
      ? lines.slice(0, selectionMenu.startLine)
      : lines;
    const sectionHeading = beforeSelection
      .reverse()
      .find((line) => /^##\s+/.test(line))
      ?.replace(/^##\s+/, "")
      .trim() ?? "";
    onEditSelection({
      noteId: activeNote.id,
      selectedText: selectionMenu.text,
      sectionHeading,
    });
    setSelectionMenu(null);
    window.getSelection()?.removeAllRanges();
  };

  if (!activeNote) {
    return (
      <NotesBrowser
        notes={notes}
        groups={groups}
        groupFilter={groupFilter}
        setGroupFilter={setGroupFilter}
        filteredNotes={filteredNotes}
        unfiledCount={unfiledCount}
        query={query}
        setQuery={setQuery}
        sort={sort}
        setSort={setSort}
        rowMenu={rowMenu}
        setRowMenu={setRowMenu}
        loading={loading}
        error={error}
        onBack={onBack}
        onCreateNote={() => void createNote()}
        onImportNotes={() => void importNotes()}
        onGenerateFromFiles={onGenerateFromLocalFiles ? () => void generateFromLocalFiles() : undefined}
        onCreateGroup={() => void createGroup()}
        onRemoveGroup={(name) => void removeGroup(name)}
        onOpenNote={(noteId) => void openNote(noteId)}
        onAssignGroup={(noteId, groupName) => void assignGroup(noteId, groupName)}
        onRenameNote={renameNoteFromList}
        onRemoveNote={(note) => void removeNoteFromList(note)}
      />
    );
  }

  return (
    <NoteEditor
      activeNote={document.base?.note ?? activeNote}
      title={title}
      content={content}
      mode={mode}
      loading={loading || document.phase === "loading"}
      saving={saving}
      saved={saved}
      error={error}
      chatOpen={chatOpen}
      chatBusy={chatBusy}
      notesLayout={notesLayout}
      outline={outline}
      stats={stats}
      selectionMenu={selectionMenu}
      workspaceRef={workspaceRef}
      editorRef={editorRef}
      previewRef={previewRef}
      onTitleChange={setTitle}
      onContentChange={setContent}
      onModeChange={(nextMode) => { setSelectionMenu(null); setMode(nextMode); }}
      onClose={closeNote}
      onDelete={() => void removeNote()}
      onToggleChat={onToggleChat}
      onToggleOutline={toggleOutline}
      onOutlineJump={jumpToOutlineItem}
      onOutlineWidthPreview={previewOutlineWidth}
      onOutlineWidthCommit={commitOutlineWidth}
      onSourceSelection={showSourceSelection}
      onPreviewSelection={showPreviewSelection}
      onSelectionClear={() => setSelectionMenu(null)}
      onAskSelection={() => void askSelection()}
      onEditSelection={() => { if (mode === "read") setMode("live"); void editSelection(); }}
      onOpenSourceConversation={onOpenSourceConversation}
    />
  );
}

function loadNoteSort(): NoteSort {
  try {
    const value = window.localStorage.getItem(NOTE_SORT_STORAGE_KEY);
    if (["updatedDesc", "updatedAsc", "createdDesc", "createdAsc", "titleAsc", "titleDesc", "sizeDesc", "sizeAsc"].includes(value ?? "")) {
      return value as NoteSort;
    }
  } catch { /* 使用默认顺序 */ }
  return "updatedDesc";
}

function compareNotes(left: LibraryNoteSummary, right: LibraryNoteSummary, sort: NoteSort) {
  const titleOrder = left.title.localeCompare(right.title, "zh-CN", { numeric: true, sensitivity: "base" });
  let primary = 0;
  if (sort === "updatedDesc") primary = right.updatedAt - left.updatedAt;
  else if (sort === "updatedAsc") primary = left.updatedAt - right.updatedAt;
  else if (sort === "createdDesc") primary = right.createdAt - left.createdAt;
  else if (sort === "createdAsc") primary = left.createdAt - right.createdAt;
  else if (sort === "titleAsc") primary = titleOrder;
  else if (sort === "titleDesc") primary = -titleOrder;
  else if (sort === "sizeDesc") primary = right.contentBytes - left.contentBytes;
  else primary = left.contentBytes - right.contentBytes;
  return primary || titleOrder || left.id.localeCompare(right.id);
}
