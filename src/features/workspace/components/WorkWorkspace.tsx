import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
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
  FolderInput,
  LoaderCircle,
  MoreHorizontal,
  NotebookPen,
  RotateCcw,
  SearchX,
  Star,
  Trash2,
} from "lucide-react";
import type {
  LibraryCollection,
  LibraryItem,
  LibraryItemUpdate,
  LibrarySort,
} from "../../library/types";
import type { LiteratureReference, NoteReference } from "../../../types/chat";
import { usePdfReaderBridge } from "../../pdf/context/PdfReaderContext";
import type {
  ActiveWorkNoteContext,
  LiteratureNavigationRequest,
  WorkLibraryView,
  WorkNoteSourceContext,
  WorkPdfDocument,
} from "../types";
import { useWorkSession } from "../hooks/useWorkSession";
import { WorkTabStrip } from "./WorkTabStrip";
import { useI18n } from "../../../i18n/I18nProvider";
import type { TranslationKey } from "../../../i18n/translations";
import "../styles/work-workspace.css";

const PdfReader = lazy(() => import("../../pdf/components/PdfReader"));
const NoteListView = lazy(() => import("../../notes/components/NoteListView"));
const NoteWorkspace = lazy(() => import("../../notes/components/NoteWorkspace"));

export type WorkWorkspaceProps = {
  libraryView: WorkLibraryView;
  searchQuery: string;
  collectionName: string | null;
  items: LibraryItem[];
  collections: LibraryCollection[];
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
  noteChatOpen: boolean;
  chatBusy: boolean;
  literatureNavigationRequest: LiteratureNavigationRequest | null;
  noteRefreshVersion: number;
  onToggleContextPanel: () => void;
  onAskSelection: (reference: LiteratureReference) => void;
  onAskNoteSelection: (reference: NoteReference) => void;
  onEditNoteSelection: (selection: {
    noteId: string;
    selectedText: string;
    sectionHeading: string;
  }) => void;
  onActiveNoteContextChange: (context: ActiveWorkNoteContext | null) => void;
  onPdfDocumentsChange: (documents: WorkPdfDocument[]) => void;
  onLiteratureNavigationHandled: (requestId: string) => void;
  onImport: () => Promise<unknown>;
  onRefresh: () => void;
  onDismissNotice: () => void;
  onSortChange: (sort: LibrarySort) => void;
  onSelectItem: (itemId: string | null) => Promise<LibraryItem | null>;
  onMarkOpened: (itemId: string) => Promise<LibraryItem | null>;
  onOpenExternal: (itemId: string) => Promise<LibraryItem>;
  onSetFavorite: (itemId: string, favorite: boolean) => Promise<LibraryItem>;
  onSaveItem: (update: LibraryItemUpdate) => Promise<LibraryItem>;
  onMoveToTrash: (itemId: string) => Promise<LibraryItem>;
  onRestoreItem: (itemId: string) => Promise<LibraryItem>;
  onDeletePermanently: (itemId: string) => Promise<boolean>;
};

export function WorkWorkspace({
  libraryView,
  searchQuery,
  collectionName,
  items,
  collections,
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
  noteChatOpen,
  chatBusy,
  literatureNavigationRequest,
  noteRefreshVersion,
  onToggleContextPanel,
  onAskSelection,
  onAskNoteSelection,
  onEditNoteSelection,
  onActiveNoteContextChange,
  onPdfDocumentsChange,
  onLiteratureNavigationHandled,
  onImport,
  onRefresh,
  onDismissNotice,
  onSortChange,
  onSelectItem,
  onMarkOpened,
  onOpenExternal,
  onSetFavorite,
  onSaveItem,
  onMoveToTrash,
  onRestoreItem,
  onDeletePermanently,
}: WorkWorkspaceProps) {
  const { language, t } = useI18n();
  const session = useWorkSession();
  const { controller: pdfController } = usePdfReaderBridge();
  const [sortMenuOpen, setSortMenuOpen] = useState(false);
  const [itemMenuId, setItemMenuId] = useState<string | null>(null);
  const [noteTotal, setNoteTotal] = useState(0);
  const [sourceNavigation, setSourceNavigation] = useState<WorkNoteSourceContext | null>(null);
  const menuAreaRef = useRef<HTMLDivElement>(null);
  const libraryContextRef = useRef({ libraryView, searchQuery, collectionName });
  const pdfDocuments = useMemo<WorkPdfDocument[]>(() => session.tabs.flatMap((tab) => (
    tab.kind === "pdf" && tab.resourceId
      ? [{
          libraryItemId: tab.resourceId,
          title: tab.title,
          active: tab.id === session.activeTab.id,
        }]
      : []
  )), [session.activeTab.id, session.tabs]);

  useEffect(() => {
    onPdfDocumentsChange(pdfDocuments);
  }, [onPdfDocumentsChange, pdfDocuments]);

  useEffect(() => () => onPdfDocumentsChange([]), [onPdfDocumentsChange]);

  useEffect(() => {
    if (session.activeTab.kind !== "note") onActiveNoteContextChange(null);
  }, [onActiveNoteContextChange, session.activeTab.kind]);

  useEffect(() => () => onActiveNoteContextChange(null), [onActiveNoteContextChange]);

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
    const request = literatureNavigationRequest;
    if (!request) return;
    const targetTabId = `pdf:${request.libraryItemId}`;
    if (session.activeTab.id !== targetTabId) {
      session.openPdfReference(request.libraryItemId, request.title);
      return;
    }
    if (pdfController?.itemId !== request.libraryItemId) return;
    pdfController.goToPage(request.pageIndex);
    onLiteratureNavigationHandled(request.requestId);
  }, [
    literatureNavigationRequest,
    onLiteratureNavigationHandled,
    pdfController,
    session.activeTab.id,
    session.openPdfReference,
  ]);

  useEffect(() => {
    const source = sourceNavigation;
    if (!source || session.activeTab.id !== `pdf:${source.sourcePdfId}`) return;
    if (pdfController?.itemId !== source.sourcePdfId) return;
    if (source.sourcePageIndex !== null) pdfController.goToPage(source.sourcePageIndex);
    setSourceNavigation(null);
  }, [pdfController, session.activeTab.id, sourceNavigation]);

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

  const viewDetails = {
    all: { title: t("work.all"), empty: t("work.emptyAll"), icon: BookOpenText },
    recent: { title: t("work.recent"), empty: t("work.emptyRecent"), icon: Clock3 },
    favorites: { title: t("work.favorites"), empty: t("work.emptyFavorites"), icon: Star },
    unfiled: { title: t("work.unfiled"), empty: t("work.emptyUnfiled"), icon: Inbox },
    notes: { title: t("work.notes"), empty: t("work.emptyNotes"), icon: NotebookPen },
    trash: { title: t("work.trash"), empty: t("work.emptyTrash"), icon: Trash2 },
  } satisfies Record<WorkLibraryView, { title: string; empty: string; icon: typeof BookOpenText }>;
  const sortOptions = [
    { id: "updated" as const, label: t("work.updated"), icon: Clock3 },
    { id: "title" as const, label: t("work.title"), icon: ArrowDownAZ },
    { id: "year" as const, label: t("work.year"), icon: ArrowDownWideNarrow },
    { id: "imported" as const, label: t("work.imported"), icon: FilePlus2 },
  ];
  const view = viewDetails[libraryView];
  const title = collectionName ?? view.title;
  const EmptyIcon = searchQuery.trim() ? SearchX : view.icon;
  const emptyTitle = searchQuery.trim()
    ? t("work.noSearchResult", { query: searchQuery.trim() })
    : collectionName
      ? t("work.emptyCollection", { name: collectionName })
      : view.empty;
  const isLearningOutcome = libraryView === "notes";
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
    <section className="work-workspace" aria-label={t("work.workspace")} ref={menuAreaRef}>
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
          <Suspense fallback={<div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>{t("work.preparingPdf")}</span></div>}>
            <PdfReader
              key={activePdfItem.id}
              item={activePdfItem}
              onOpenExternal={onOpenExternal}
              onOpenNote={session.openNote}
              onAskSelection={onAskSelection}
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
        <Suspense fallback={<div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>{t("work.preparingNotes")}</span></div>}>
          <NoteWorkspace
            noteId={session.activeTab.resourceId}
            source={session.activeTab.noteSource ?? null}
            chatOpen={noteChatOpen}
            chatBusy={chatBusy}
            refreshVersion={noteRefreshVersion}
            onUpdated={session.updateNoteTab}
            onDeleted={() => session.closeTab(session.activeTab.id)}
            onToggleChat={onToggleContextPanel}
            onAskSelection={onAskNoteSelection}
            onEditSelection={onEditNoteSelection}
            onContextChange={onActiveNoteContextChange}
            onOpenSourcePdf={(source) => {
              setSourceNavigation(source);
              session.openPdfReference(source.sourcePdfId, source.sourcePdfTitle);
            }}
          />
        </Suspense>
      ) : (
        <div className="work-library-view">
          <header className="work-library-header">
            <div className="work-library-heading">
              <h1>{title}</h1>
              <span>{t("common.items", { count: libraryView === "notes" ? noteTotal : total })}</span>
            </div>
            {!isLearningOutcome ? (
              <div className="work-library-header-actions">
                <button className="icon-button" type="button" title={t("work.importPdfButton")} onClick={() => void onImport().catch(() => undefined)}>
                  <FilePlus2 size={17} />
                </button>
                <div className="work-sort-menu-wrap">
                  <button
                    className="icon-button"
                    type="button"
                    title={t("work.sort")}
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
                <button className="icon-button" type="button" title={t("work.columns")} disabled>
                  <Columns3 size={17} />
                </button>
              </div>
            ) : null}
          </header>

          {notice ? (
            <div className="work-library-notice" role="status">
              <span>{notice}</span>
              <button type="button" aria-label={t("work.closeNotice")} onClick={onDismissNotice}>×</button>
            </div>
          ) : null}

          <div className="work-library-content">
            <div
              className={`work-library-columns${isLearningOutcome ? " work-library-columns-outcome" : ""}`}
              aria-hidden="true"
            >
              {isLearningOutcome ? (
                <>
                  <span>{t("work.name")}</span>
                  <span>{t("work.linkedLiterature")}</span>
                  <span>{t("work.updatedAt")}</span>
                </>
              ) : (
                <>
                  <span>{t("work.title")}</span>
                  <span>{t("work.author")}</span>
                  <span>{t("work.year")}</span>
                  <span>{t("work.collectionColumn")}</span>
                  <span>{t("work.lastRead")}</span>
                  <span />
                </>
              )}
            </div>

            {loading ? (
              <div className="work-library-state" role="status">
                <LoaderCircle className="work-library-spinner" size={24} />
                <span>{t("work.loadingLibrary")}</span>
              </div>
            ) : error ? (
              <div className="work-library-state work-library-error" role="alert">
                <strong>{t("work.libraryUnavailable")}</strong>
                <span>{error}</span>
                <button type="button" onClick={onRefresh}>{t("work.reload")}</button>
              </div>
            ) : libraryView === "notes" ? (
              <Suspense fallback={<div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>{t("work.preparingNoteList")}</span></div>}>
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
                    <span>{t("work.importPdfButton")}</span>
                  </button>
                ) : (
                  <p>{searchQuery.trim() ? t("work.adjustSearch") : t("common.items", { count: 0 })}</p>
                )}
              </div>
            ) : (
              <div className="work-library-list" role="table" aria-label={t("work.libraryList", { title })}>
                {items.map((item) => (
                  <LibraryRow
                    item={item}
                    collections={collections}
                    selected={selectedItem?.id === item.id}
                    trashView={libraryView === "trash"}
                    busy={actionPending}
                    menuOpen={itemMenuId === item.id}
                    language={language}
                    t={t}
                    key={item.id}
                    onSelect={() => void onSelectItem(item.id)}
                    onOpen={() => void openItem(item)}
                    onToggleMenu={() => setItemMenuId((current) => current === item.id ? null : item.id)}
                    onFavorite={() => void onSetFavorite(item.id, !item.favorite).catch(() => undefined)}
                    onCollectionsChange={(collectionIds) => {
                      void onSaveItem({
                        itemId: item.id,
                        title: item.title,
                        authors: item.authors,
                        publicationYear: item.publicationYear,
                        publicationTitle: item.publicationTitle,
                        doi: item.doi,
                        abstractText: item.abstractText,
                        favorite: item.favorite,
                        tags: item.tags,
                        collectionIds,
                      }).catch(() => undefined);
                    }}
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
                      if (!window.confirm(t("work.deletePermanentConfirm", { title: item.title }))) return;
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
  collections: LibraryCollection[];
  selected: boolean;
  trashView: boolean;
  busy: boolean;
  menuOpen: boolean;
  language: "zh" | "en";
  t: (key: TranslationKey, values?: Record<string, string | number>) => string;
  onSelect: () => void;
  onOpen: () => void;
  onToggleMenu: () => void;
  onFavorite: () => void;
  onCollectionsChange: (collectionIds: string[]) => void;
  onMoveToTrash: () => void;
  onRestore: () => void;
  onDeletePermanently: () => void;
};

function LibraryRow({
  item,
  collections,
  selected,
  trashView,
  busy,
  menuOpen,
  language,
  t,
  onSelect,
  onOpen,
  onToggleMenu,
  onFavorite,
  onCollectionsChange,
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
        {!item.file.available ? <small>{t("work.fileMissing")}</small> : null}
      </span>
      <span role="cell" title={item.authors.join("、")}>
        {item.authors.length > 0 ? item.authors.join("、") : "-"}
      </span>
      <span role="cell">{item.publicationYear ?? "-"}</span>
      <span role="cell" title={item.collectionNames.join("、")}>
        {item.collectionNames.length > 0 ? item.collectionNames.join(language === "en" ? ", " : "、") : t("work.unfiled")}
      </span>
      <span role="cell">{formatTimestamp(item.lastOpenedAt, language)}</span>
      <div className="work-library-row-actions" role="cell" onClick={(event) => event.stopPropagation()}>
        {!trashView ? (
          <button
            className={item.favorite ? "work-library-favorite-active" : ""}
            type="button"
            title={item.favorite ? t("work.unfavorite") : t("work.favorite")}
            disabled={busy}
            onClick={onFavorite}
          >
            <Star size={15} fill={item.favorite ? "currentColor" : "none"} />
          </button>
        ) : null}
        <button
          type="button"
          title={t("work.itemActions")}
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
                  <span>{t("common.restore")}</span>
                </button>
                <button
                  className="work-library-menu-danger"
                  type="button"
                  role="menuitem"
                  onClick={onDeletePermanently}
                >
                  <Trash2 size={14} />
                  <span>{t("work.deletePermanent")}</span>
                </button>
              </>
            ) : (
              <>
                <div className="work-library-collection-menu" aria-label={t("work.collectionColumn")}>
                  <span><FolderInput size={14} />{t("work.addCollection")}</span>
                  {collections.length > 0 ? collections.map((collection) => {
                    const checked = item.collectionIds.includes(collection.id);
                    return (
                      <label key={collection.id}>
                        <input
                          type="checkbox"
                          checked={checked}
                          disabled={busy}
                          onChange={() => onCollectionsChange(checked
                            ? item.collectionIds.filter((id) => id !== collection.id)
                            : [...item.collectionIds, collection.id])}
                        />
                        <span>{collection.name}</span>
                      </label>
                    );
                  }) : <small>{t("work.createCollectionFirst")}</small>}
                </div>
                <div className="work-library-menu-separator" />
                <button
                  className="work-library-menu-danger"
                  type="button"
                  role="menuitem"
                  onClick={onMoveToTrash}
                >
                  <Trash2 size={14} />
                  <span>{t("work.moveTrash")}</span>
                </button>
              </>
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
  const { language, t } = useI18n();
  if (!item) {
    return (
      <div
        className={`work-pdf-resource work-library-state${loading ? "" : " work-library-error"}`}
        role={loading ? "status" : "alert"}
      >
        {loading ? <LoaderCircle className="work-library-spinner" size={24} /> : <SearchX size={28} />}
        <strong>{loading ? t("work.loadingDocument", { title: fallbackTitle }) : t("work.documentUnavailable")}</strong>
        {!loading ? <span>{error || t("work.documentUnavailableDescription")}</span> : null}
      </div>
    );
  }
  return (
    <article className="work-pdf-resource" aria-label={item.title}>
      <div className="work-pdf-resource-icon"><FileText size={30} /></div>
      <h1>{item.title}</h1>
      <dl>
        <div><dt>{t("work.file")}</dt><dd>{item.file.originalName}</dd></div>
        <div><dt>{t("work.size")}</dt><dd>{formatFileSize(item.file.fileSize)}</dd></div>
        <div><dt>{t("work.author")}</dt><dd>{item.authors.join(language === "en" ? ", " : "、") || t("work.notProvided")}</dd></div>
        <div><dt>{t("work.collectionColumn")}</dt><dd>{item.collectionNames.join(language === "en" ? ", " : "、") || t("work.unfiled")}</dd></div>
      </dl>
      <button
        type="button"
        disabled={busy || !item.file.available}
        onClick={() => void onOpenExternal(item.id).catch(() => undefined)}
      >
        <ExternalLink size={16} />
        <span>{t("work.openSystem")}</span>
      </button>
    </article>
  );
}

function formatTimestamp(value: number | null, language: "zh" | "en"): string {
  if (!value) return "-";
  return new Intl.DateTimeFormat(language === "en" ? "en-US" : "zh-CN", {
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
