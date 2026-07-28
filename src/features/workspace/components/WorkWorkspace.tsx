import { lazy, Suspense, useEffect, useRef, useState } from "react";
import {
  ArrowDownAZ,
  ArrowDownWideNarrow,
  BookOpenText,
  Check,
  Clock3,
  Columns3,
  ExternalLink,
  FilePlus2,
  FileText,
  Inbox,
  LoaderCircle,
  MoreHorizontal,
  Network,
  NotebookPen,
  RotateCcw,
  SearchX,
  Star,
  Trash2,
} from "lucide-react";
import type { LibraryItem, LibrarySort } from "../../library/types";
import type { WorkLibraryView } from "../types";
import { useWorkSession } from "../hooks/useWorkSession";
import { WorkTabStrip } from "./WorkTabStrip";
import "../styles/work-workspace.css";

const PdfReader = lazy(() => import("../../pdf/components/PdfReader"));
const NoteListView = lazy(() => import("../../notes/components/NoteListView"));
const NoteWorkspace = lazy(() => import("../../notes/components/NoteWorkspace"));

type WorkWorkspaceProps = {
  libraryView: WorkLibraryView;
  searchQuery: string;
  collectionName: string | null;
  items: LibraryItem[];
  total: number;
  loading: boolean;
  error: string;
  notice: string;
  actionPending: boolean;
  selectedItem: LibraryItem | null;
  selectedItemLoading: boolean;
  selectionError: string;
  sort: LibrarySort;
  contextPanelOpen: boolean;
  chatBusy: boolean;
  onToggleContextPanel: () => void;
  onImport: () => Promise<unknown>;
  onRefresh: () => void;
  onDismissNotice: () => void;
  onSortChange: (sort: LibrarySort) => void;
  onSelectItem: (itemId: string | null) => Promise<LibraryItem | null>;
  onMarkOpened: (itemId: string) => Promise<LibraryItem | null>;
  onOpenExternal: (itemId: string) => Promise<LibraryItem>;
  onSetFavorite: (itemId: string, favorite: boolean) => Promise<LibraryItem>;
  onMoveToTrash: (itemId: string) => Promise<LibraryItem>;
  onRestoreItem: (itemId: string) => Promise<LibraryItem>;
  onDeletePermanently: (itemId: string) => Promise<boolean>;
};

const viewDetails = {
  all: { title: "全部文献", empty: "文库中暂无文献", icon: BookOpenText },
  recent: { title: "最近阅读", empty: "暂无最近阅读记录", icon: Clock3 },
  favorites: { title: "收藏", empty: "暂无收藏文献", icon: Star },
  unfiled: { title: "未分类", empty: "暂无未分类文献", icon: Inbox },
  notes: { title: "笔记", empty: "暂无学习笔记", icon: NotebookPen },
  "mind-maps": { title: "思维导图", empty: "暂无思维导图", icon: Network },
  trash: { title: "回收站", empty: "回收站为空", icon: Trash2 },
} satisfies Record<WorkLibraryView, { title: string; empty: string; icon: typeof BookOpenText }>;

const sortOptions = [
  { id: "updated", label: "最近更新", icon: Clock3 },
  { id: "title", label: "标题", icon: ArrowDownAZ },
  { id: "year", label: "出版年份", icon: ArrowDownWideNarrow },
  { id: "imported", label: "导入时间", icon: FilePlus2 },
] satisfies Array<{ id: LibrarySort; label: string; icon: typeof Clock3 }>;

export function WorkWorkspace({
  libraryView,
  searchQuery,
  collectionName,
  items,
  total,
  loading,
  error,
  notice,
  actionPending,
  selectedItem,
  selectedItemLoading,
  selectionError,
  sort,
  contextPanelOpen,
  chatBusy,
  onToggleContextPanel,
  onImport,
  onRefresh,
  onDismissNotice,
  onSortChange,
  onSelectItem,
  onMarkOpened,
  onOpenExternal,
  onSetFavorite,
  onMoveToTrash,
  onRestoreItem,
  onDeletePermanently,
}: WorkWorkspaceProps) {
  const session = useWorkSession();
  const [sortMenuOpen, setSortMenuOpen] = useState(false);
  const [itemMenuId, setItemMenuId] = useState<string | null>(null);
  const [noteTotal, setNoteTotal] = useState(0);
  const menuAreaRef = useRef<HTMLDivElement>(null);
  const libraryContextRef = useRef({ libraryView, searchQuery, collectionName });

  useEffect(() => {
    const previous = libraryContextRef.current;
    libraryContextRef.current = { libraryView, searchQuery, collectionName };
    if (
      previous.libraryView === libraryView
      && previous.searchQuery === searchQuery
      && previous.collectionName === collectionName
    ) return;
    session.showLibrary();
    void onSelectItem(null);
  }, [collectionName, libraryView, onSelectItem, searchQuery, session.showLibrary]);

  useEffect(() => {
    const itemId = session.activeTab.resourceId;
    if (session.activeTab.kind === "pdf" && itemId) void onSelectItem(itemId);
  }, [onSelectItem, session.activeTab.kind, session.activeTab.resourceId]);

  useEffect(() => {
    const item = selectedItem;
    if (
      session.activeTab.kind === "pdf"
      && item
      && item.id === session.activeTab.resourceId
      && item.title !== session.activeTab.title
    ) {
      session.openPdf(item);
    }
  }, [selectedItem, session.activeTab, session.openPdf]);

  useEffect(() => {
    function closeMenus(event: MouseEvent) {
      if (!menuAreaRef.current?.contains(event.target as Node)) {
        setSortMenuOpen(false);
        setItemMenuId(null);
      }
    }
    document.addEventListener("mousedown", closeMenus);
    return () => document.removeEventListener("mousedown", closeMenus);
  }, []);

  const view = viewDetails[libraryView];
  const title = collectionName ?? view.title;
  const EmptyIcon = searchQuery.trim() ? SearchX : view.icon;
  const emptyTitle = searchQuery.trim()
    ? `没有找到“${searchQuery.trim()}”`
    : collectionName
      ? `“${collectionName}”中暂无文献`
      : view.empty;
  const isLearningOutcome = libraryView === "notes" || libraryView === "mind-maps";
  const activePdfItem = session.activeTab.kind === "pdf"
    ? selectedItem?.id === session.activeTab.resourceId
      ? selectedItem
      : items.find((item) => item.id === session.activeTab.resourceId) ?? null
    : null;

  const openItem = async (item: LibraryItem) => {
    try {
      const refreshed = await onMarkOpened(item.id);
      session.openPdf(refreshed ?? item);
    } catch {
      // 数据层已经记录可展示的错误。
    }
  };

  return (
    <section className="work-workspace" aria-label="Work 文献学习工作区" ref={menuAreaRef}>
      <WorkTabStrip
        tabs={session.tabs}
        activeTabId={session.activeTab.id}
        contextPanelOpen={contextPanelOpen}
        chatBusy={chatBusy}
        onTabSelect={session.selectTab}
        onTabClose={session.closeTab}
        onToggleContextPanel={onToggleContextPanel}
      />

      {session.activeTab.kind === "pdf" ? (
        activePdfItem ? (
          <Suspense fallback={<div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>正在准备 PDF 阅读器</span></div>}>
            <PdfReader
              key={activePdfItem.id}
              item={activePdfItem}
              onOpenExternal={onOpenExternal}
              onOpenNote={session.openNote}
            />
          </Suspense>
        ) : (
          <WorkPdfResource
            item={activePdfItem}
            fallbackTitle={session.activeTab.title}
            loading={selectedItemLoading}
            error={selectionError}
            busy={actionPending}
            onOpenExternal={onOpenExternal}
          />
        )
      ) : session.activeTab.kind === "note" && session.activeTab.resourceId ? (
        <Suspense fallback={<div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>正在准备笔记</span></div>}>
          <NoteWorkspace
            noteId={session.activeTab.resourceId}
            onUpdated={session.updateNoteTab}
            onDeleted={() => session.closeTab(session.activeTab.id)}
          />
        </Suspense>
      ) : (
        <div className="work-library-view">
          <header className="work-library-header">
            <div className="work-library-heading">
              <h1>{title}</h1>
              <span>{libraryView === "notes" ? noteTotal : total} 项</span>
            </div>
            {!isLearningOutcome ? (
              <div className="work-library-header-actions">
                <div className="work-sort-menu-wrap">
                  <button
                    className="icon-button"
                    type="button"
                    title="排序"
                    aria-expanded={sortMenuOpen}
                    onClick={() => setSortMenuOpen((open) => !open)}
                  >
                    <ArrowDownWideNarrow size={17} />
                  </button>
                  {sortMenuOpen ? (
                    <div className="work-sort-menu" role="menu">
                      {sortOptions.map(({ id, label, icon: Icon }) => (
                        <button
                          className={sort === id ? "work-sort-active" : ""}
                          type="button"
                          role="menuitemradio"
                          aria-checked={sort === id}
                          key={id}
                          onClick={() => {
                            onSortChange(id);
                            setSortMenuOpen(false);
                          }}
                        >
                          <Icon size={15} />
                          <span>{label}</span>
                          {sort === id ? <Check size={14} /> : null}
                        </button>
                      ))}
                    </div>
                  ) : null}
                </div>
                <button className="icon-button" type="button" title="列表列设置" disabled>
                  <Columns3 size={17} />
                </button>
              </div>
            ) : null}
          </header>

          {notice ? (
            <div className="work-library-notice" role="status">
              <span>{notice}</span>
              <button type="button" aria-label="关闭提示" onClick={onDismissNotice}>×</button>
            </div>
          ) : null}

          <div className="work-library-content">
            <div
              className={`work-library-columns${isLearningOutcome ? " work-library-columns-outcome" : ""}`}
              aria-hidden="true"
            >
              {isLearningOutcome ? (
                <>
                  <span>名称</span>
                  <span>关联文献</span>
                  <span>更新时间</span>
                </>
              ) : (
                <>
                  <span>标题</span>
                  <span>作者</span>
                  <span>年份</span>
                  <span>分类</span>
                  <span>最近阅读</span>
                  <span />
                </>
              )}
            </div>

            {loading ? (
              <div className="work-library-state" role="status">
                <LoaderCircle className="work-library-spinner" size={24} />
                <span>正在读取文献库</span>
              </div>
            ) : error ? (
              <div className="work-library-state work-library-error" role="alert">
                <strong>文献库暂时不可用</strong>
                <span>{error}</span>
                <button type="button" onClick={onRefresh}>重新加载</button>
              </div>
            ) : libraryView === "notes" ? (
              <Suspense fallback={<div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>正在准备笔记列表</span></div>}>
                <NoteListView
                  searchQuery={searchQuery}
                  onOpenNote={session.openNote}
                  onCountChange={setNoteTotal}
                />
              </Suspense>
            ) : isLearningOutcome || items.length === 0 ? (
              <div className="work-library-empty" role="status">
                <EmptyIcon size={34} aria-hidden="true" />
                <h2>{emptyTitle}</h2>
                {libraryView === "all" && !searchQuery.trim() && !collectionName ? (
                  <button type="button" onClick={() => void onImport().catch(() => undefined)}>
                    <FilePlus2 size={16} />
                    <span>导入 PDF</span>
                  </button>
                ) : (
                  <p>{searchQuery.trim() ? "请调整查询条件" : "0 项"}</p>
                )}
              </div>
            ) : (
              <div className="work-library-list" role="table" aria-label={`${title}文献列表`}>
                {items.map((item) => (
                  <LibraryRow
                    item={item}
                    selected={selectedItem?.id === item.id}
                    trashView={libraryView === "trash"}
                    busy={actionPending}
                    menuOpen={itemMenuId === item.id}
                    key={item.id}
                    onSelect={() => void onSelectItem(item.id)}
                    onOpen={() => void openItem(item)}
                    onToggleMenu={() => setItemMenuId((current) => current === item.id ? null : item.id)}
                    onFavorite={() => void onSetFavorite(item.id, !item.favorite).catch(() => undefined)}
                    onMoveToTrash={() => {
                      setItemMenuId(null);
                      void onMoveToTrash(item.id)
                        .then(() => session.closeResource(item.id))
                        .catch(() => undefined);
                    }}
                    onRestore={() => {
                      setItemMenuId(null);
                      void onRestoreItem(item.id).catch(() => undefined);
                    }}
                    onDeletePermanently={() => {
                      setItemMenuId(null);
                      if (!window.confirm(`永久删除“${item.title}”及其应用内 PDF 快照吗？此操作无法撤销。`)) return;
                      void onDeletePermanently(item.id)
                        .then((removed) => {
                          if (removed) session.closeResource(item.id);
                        })
                        .catch(() => undefined);
                    }}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

type LibraryRowProps = {
  item: LibraryItem;
  selected: boolean;
  trashView: boolean;
  busy: boolean;
  menuOpen: boolean;
  onSelect: () => void;
  onOpen: () => void;
  onToggleMenu: () => void;
  onFavorite: () => void;
  onMoveToTrash: () => void;
  onRestore: () => void;
  onDeletePermanently: () => void;
};

function LibraryRow({
  item,
  selected,
  trashView,
  busy,
  menuOpen,
  onSelect,
  onOpen,
  onToggleMenu,
  onFavorite,
  onMoveToTrash,
  onRestore,
  onDeletePermanently,
}: LibraryRowProps) {
  return (
    <div
      className={`work-library-row${selected ? " work-library-row-selected" : ""}`}
      role="row"
      tabIndex={0}
      aria-selected={selected}
      onClick={onSelect}
      onDoubleClick={onOpen}
      onKeyDown={(event) => {
        if (event.key === "Enter") onOpen();
        if (event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      <span className="work-library-title-cell" role="cell" title={item.title}>
        <FileText size={15} />
        <strong>{item.title}</strong>
        {!item.file.available ? <small>文件缺失</small> : null}
      </span>
      <span role="cell" title={item.authors.join("、")}>
        {item.authors.length > 0 ? item.authors.join("、") : "-"}
      </span>
      <span role="cell">{item.publicationYear ?? "-"}</span>
      <span role="cell" title={item.collectionNames.join("、")}>
        {item.collectionNames.length > 0 ? item.collectionNames.join("、") : "未分类"}
      </span>
      <span role="cell">{formatTimestamp(item.lastOpenedAt)}</span>
      <div className="work-library-row-actions" role="cell" onClick={(event) => event.stopPropagation()}>
        {!trashView ? (
          <button
            className={item.favorite ? "work-library-favorite-active" : ""}
            type="button"
            title={item.favorite ? "取消收藏" : "收藏"}
            disabled={busy}
            onClick={onFavorite}
          >
            <Star size={15} fill={item.favorite ? "currentColor" : "none"} />
          </button>
        ) : null}
        <button
          type="button"
          title="文献操作"
          disabled={busy}
          aria-expanded={menuOpen}
          onClick={onToggleMenu}
        >
          <MoreHorizontal size={15} />
        </button>
        {menuOpen ? (
          <div className="work-library-row-menu" role="menu">
            {trashView ? (
              <>
                <button type="button" role="menuitem" onClick={onRestore}>
                  <RotateCcw size={14} />
                  <span>恢复</span>
                </button>
                <button
                  className="work-library-menu-danger"
                  type="button"
                  role="menuitem"
                  onClick={onDeletePermanently}
                >
                  <Trash2 size={14} />
                  <span>永久删除</span>
                </button>
              </>
            ) : (
              <button
                className="work-library-menu-danger"
                type="button"
                role="menuitem"
                onClick={onMoveToTrash}
              >
                <Trash2 size={14} />
                <span>移入回收站</span>
              </button>
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}

type WorkPdfResourceProps = {
  item: LibraryItem | null;
  fallbackTitle: string;
  loading: boolean;
  error: string;
  busy: boolean;
  onOpenExternal: (itemId: string) => Promise<LibraryItem>;
};

function WorkPdfResource({
  item,
  fallbackTitle,
  loading,
  error,
  busy,
  onOpenExternal,
}: WorkPdfResourceProps) {
  if (!item) {
    return (
      <div
        className={`work-pdf-resource work-library-state${loading ? "" : " work-library-error"}`}
        role={loading ? "status" : "alert"}
      >
        {loading ? <LoaderCircle className="work-library-spinner" size={24} /> : <SearchX size={28} />}
        <strong>{loading ? `正在读取 ${fallbackTitle}` : "无法读取该文献"}</strong>
        {!loading ? <span>{error || "文献可能已被删除，请关闭这个页签后重新打开。"}</span> : null}
      </div>
    );
  }
  return (
    <article className="work-pdf-resource" aria-label={item.title}>
      <div className="work-pdf-resource-icon"><FileText size={30} /></div>
      <h1>{item.title}</h1>
      <dl>
        <div><dt>文件</dt><dd>{item.file.originalName}</dd></div>
        <div><dt>大小</dt><dd>{formatFileSize(item.file.fileSize)}</dd></div>
        <div><dt>作者</dt><dd>{item.authors.join("、") || "未填写"}</dd></div>
        <div><dt>分类</dt><dd>{item.collectionNames.join("、") || "未分类"}</dd></div>
      </dl>
      <button
        type="button"
        disabled={busy || !item.file.available}
        onClick={() => void onOpenExternal(item.id).catch(() => undefined)}
      >
        <ExternalLink size={16} />
        <span>在系统阅读器中打开</span>
      </button>
    </article>
  );
}

function formatTimestamp(value: number | null): string {
  if (!value) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default WorkWorkspace;
