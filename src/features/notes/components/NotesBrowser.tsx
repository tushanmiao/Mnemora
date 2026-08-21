import {
  ArrowLeft,
  FilePlus2,
  FileText,
  Folder,
  FolderPlus,
  Inbox,
  LoaderCircle,
  MoreHorizontal,
  NotebookText,
  Pencil,
  Search,
  SlidersHorizontal,
  Trash2,
} from "lucide-react";
import { useState, type Dispatch, type SetStateAction } from "react";
import type { LibraryNoteGroup, LibraryNoteSummary } from "../../library/types";
import { formatNoteSize, NOTE_TIME_FORMATTER } from "../utils/notesWorkspace";

export type GroupFilter = "all" | "unfiled" | { group: string };
export type NoteSort = "updatedDesc" | "updatedAsc" | "createdDesc" | "createdAsc" | "titleAsc" | "titleDesc" | "sizeDesc" | "sizeAsc";

type NotesBrowserProps = {
  notes: LibraryNoteSummary[];
  groups: LibraryNoteGroup[];
  groupFilter: GroupFilter;
  setGroupFilter: Dispatch<SetStateAction<GroupFilter>>;
  filteredNotes: LibraryNoteSummary[];
  unfiledCount: number;
  query: string;
  setQuery: (query: string) => void;
  sort: NoteSort;
  setSort: (sort: NoteSort) => void;
  rowMenu: string | null;
  setRowMenu: Dispatch<SetStateAction<string | null>>;
  loading: boolean;
  error: string;
  onBack: () => void;
  onCreateNote: () => void;
  onImportNotes: () => void;
  onCreateGroup: () => void;
  onRemoveGroup: (name: string) => void;
  onOpenNote: (noteId: string) => void;
  onAssignGroup: (noteId: string, groupName: string | null) => void;
  onRenameNote: (note: LibraryNoteSummary, title: string) => Promise<boolean>;
  onRemoveNote: (note: LibraryNoteSummary) => void;
};

/** 笔记库列表只负责浏览和分组交互，不持有加载或持久化状态。 */
export function NotesBrowser({
  notes,
  groups,
  groupFilter,
  setGroupFilter,
  filteredNotes,
  unfiledCount,
  query,
  setQuery,
  sort,
  setSort,
  rowMenu,
  setRowMenu,
  loading,
  error,
  onBack,
  onCreateNote,
  onImportNotes,
  onCreateGroup,
  onRemoveGroup,
  onOpenNote,
  onAssignGroup,
  onRenameNote,
  onRemoveNote,
}: NotesBrowserProps) {
  const [renamingNoteId, setRenamingNoteId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);

  const startRename = (note: LibraryNoteSummary) => {
    setRowMenu(null);
    setRenamingNoteId(note.id);
    setRenameDraft(note.title);
  };

  const finishRename = async (note: LibraryNoteSummary) => {
    if (renameBusy || renamingNoteId !== note.id) return;
    const nextTitle = renameDraft.trim();
    if (!nextTitle || nextTitle === note.title) {
      setRenamingNoteId(null);
      return;
    }
    setRenameBusy(true);
    const renamed = await onRenameNote(note, nextTitle);
    setRenameBusy(false);
    if (renamed) setRenamingNoteId(null);
  };

  return (
    <section className="notes-browser" aria-label="Markdown 笔记库">
      <header className="notes-browser-toolbar">
        <button type="button" className="notes-back-button" onClick={onBack}>
          <ArrowLeft size={16} />
          <span>返回 Chat</span>
        </button>
        <strong>笔记</strong>
        <button type="button" className="notes-create-button" onClick={onCreateNote}>
          <FilePlus2 size={16} />
          <span>新建 Markdown 笔记</span>
        </button>
        <button type="button" className="notes-back-button" onClick={onImportNotes}>
          <FileText size={15} />
          <span>导入 Markdown</span>
        </button>
      </header>
      <div className="notes-browser-body">
        <aside className="notes-groups-nav" aria-label="笔记分组">
          <header>
            <strong>分组</strong>
            <button type="button" title="新建分组" aria-label="新建分组" onClick={onCreateGroup}>
              <FolderPlus size={15} />
            </button>
          </header>
          <nav>
            <button
              type="button"
              className={groupFilter === "all" ? "notes-group-item is-active" : "notes-group-item"}
              onClick={() => setGroupFilter("all")}
            >
              <NotebookText size={15} /><span>全部笔记</span><small>{notes.length}</small>
            </button>
            <button
              type="button"
              className={groupFilter === "unfiled" ? "notes-group-item is-active" : "notes-group-item"}
              onClick={() => setGroupFilter("unfiled")}
            >
              <Inbox size={15} /><span>未分类</span><small>{unfiledCount}</small>
            </button>
            {groups.map((group) => (
              <div
                className={typeof groupFilter === "object" && groupFilter.group === group.name
                  ? "notes-group-row is-active"
                  : "notes-group-row"}
                key={group.name}
              >
                <button type="button" className="notes-group-item" onClick={() => setGroupFilter({ group: group.name })}>
                  <Folder size={15} /><span>{group.name}</span><small>{group.noteCount}</small>
                </button>
                <button
                  type="button"
                  className="notes-group-remove"
                  title={`删除分组 ${group.name}`}
                  aria-label={`删除分组 ${group.name}`}
                  onClick={() => onRemoveGroup(group.name)}
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
            <label className="notes-sort-control" title="笔记排序">
              <SlidersHorizontal size={14} />
              <select value={sort} aria-label="笔记排序" onChange={(event) => setSort(event.target.value as NoteSort)}>
                <option value="updatedDesc">最近更新</option>
                <option value="updatedAsc">最早更新</option>
                <option value="createdDesc">最近创建</option>
                <option value="createdAsc">最早创建</option>
                <option value="titleAsc">标题 A–Z</option>
                <option value="titleDesc">标题 Z–A</option>
                <option value="sizeDesc">大小：从大到小</option>
                <option value="sizeAsc">大小：从小到大</option>
              </select>
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
              <button type="button" onClick={onCreateNote}>新建笔记</button>
            </div>
          ) : (
            <div className="notes-table" role="table" aria-label="笔记列表">
              <div className="notes-table-head" role="row">
                <span role="columnheader">标题</span><span role="columnheader">创建时间</span>
                <span role="columnheader">最后修改</span><span role="columnheader">分组</span>
                <span role="columnheader">大小</span><span role="columnheader" aria-label="操作" />
              </div>
              <div className="notes-table-body" role="rowgroup">
                {filteredNotes.map((note) => (
                  <div className="notes-table-row" role="row" key={note.id}>
                    {renamingNoteId === note.id ? (
                      <label className="notes-table-title notes-table-title-editor" role="cell">
                        <FileText size={16} />
                        <input
                          autoFocus
                          value={renameDraft}
                          maxLength={240}
                          disabled={renameBusy}
                          aria-label={`重命名 ${note.title}`}
                          onChange={(event) => setRenameDraft(event.target.value)}
                          onBlur={() => void finishRename(note)}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") { event.preventDefault(); void finishRename(note); }
                            if (event.key === "Escape") { event.preventDefault(); setRenamingNoteId(null); }
                          }}
                        />
                      </label>
                    ) : (
                      <button type="button" className="notes-table-title" role="cell" title={note.contentPreview || "空白笔记"} onClick={() => onOpenNote(note.id)}>
                        <FileText size={16} /><span>{note.title}</span>
                      </button>
                    )}
                    <span role="cell" className="notes-table-muted">{NOTE_TIME_FORMATTER.format(note.createdAt)}</span>
                    <span role="cell" className="notes-table-muted">{NOTE_TIME_FORMATTER.format(note.updatedAt)}</span>
                    <span role="cell">
                      <select value={note.groupName ?? ""} aria-label={`${note.title} 所属分组`} onChange={(event) => onAssignGroup(note.id, event.target.value || null)}>
                        <option value="">未分类</option>
                        {groups.map((group) => <option value={group.name} key={group.name}>{group.name}</option>)}
                      </select>
                    </span>
                    <span role="cell" className="notes-table-muted" title={`${note.contentChars} 个字符`}>{formatNoteSize(note.contentBytes)}</span>
                    <span role="cell" className="notes-table-actions">
                      <button
                        type="button"
                        className="notes-table-more"
                        title="更多操作"
                        aria-label={`${note.title} 更多操作`}
                        aria-expanded={rowMenu === note.id}
                        onMouseDown={(event) => event.stopPropagation()}
                        onClick={() => setRowMenu((current) => current === note.id ? null : note.id)}
                      >
                        <MoreHorizontal size={15} />
                      </button>
                      {rowMenu === note.id ? (
                        <div className="notes-row-menu" role="menu" onMouseDown={(event) => event.stopPropagation()}>
                          <button type="button" role="menuitem" onClick={() => startRename(note)}>
                            <Pencil size={14} /><span>重命名</span>
                          </button>
                          <button type="button" role="menuitem" className="notes-row-menu-danger" onClick={() => onRemoveNote(note)}>
                            <Trash2 size={14} /><span>删除笔记</span>
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
