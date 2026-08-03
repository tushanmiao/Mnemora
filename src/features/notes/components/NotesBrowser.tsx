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
  Search,
  Trash2,
} from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import type { LibraryNoteGroup, LibraryNoteSummary } from "../../library/types";
import { formatNoteSize, NOTE_TIME_FORMATTER } from "../utils/notesWorkspace";

export type GroupFilter = "all" | "unfiled" | { group: string };

type NotesBrowserProps = {
  notes: LibraryNoteSummary[];
  groups: LibraryNoteGroup[];
  groupFilter: GroupFilter;
  setGroupFilter: Dispatch<SetStateAction<GroupFilter>>;
  filteredNotes: LibraryNoteSummary[];
  unfiledCount: number;
  query: string;
  setQuery: (query: string) => void;
  creatorName: string;
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
  creatorName,
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
  onRemoveNote,
}: NotesBrowserProps) {
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
                <span role="columnheader">标题</span><span role="columnheader">创建者</span>
                <span role="columnheader">最后修改</span><span role="columnheader">分组</span>
                <span role="columnheader">大小</span><span role="columnheader" aria-label="操作" />
              </div>
              <div className="notes-table-body" role="rowgroup">
                {filteredNotes.map((note) => (
                  <div className="notes-table-row" role="row" key={note.id}>
                    <button type="button" className="notes-table-title" role="cell" title={note.contentPreview || "空白笔记"} onClick={() => onOpenNote(note.id)}>
                      <FileText size={16} /><span>{note.title}</span>
                    </button>
                    <span role="cell" className="notes-table-muted" title={creatorName}>{creatorName}</span>
                    <span role="cell" className="notes-table-muted">{NOTE_TIME_FORMATTER.format(note.updatedAt)}</span>
                    <span role="cell">
                      <select value={note.groupName ?? ""} aria-label={`${note.title} 所属分组`} onChange={(event) => onAssignGroup(note.id, event.target.value || null)}>
                        <option value="">未分类</option>
                        {groups.map((group) => <option value={group.name} key={group.name}>{group.name}</option>)}
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
                        onClick={() => setRowMenu((current) => current === note.id ? null : note.id)}
                      >
                        <MoreHorizontal size={15} />
                      </button>
                      {rowMenu === note.id ? (
                        <div className="notes-row-menu" role="menu" onMouseDown={(event) => event.stopPropagation()}>
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
