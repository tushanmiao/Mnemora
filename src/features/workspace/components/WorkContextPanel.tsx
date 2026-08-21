import { lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import {
  BookOpenText,
  Check,
  ChevronDown,
  Download,
  FileText,
  Files,
  Highlighter,
  Info,
  Link2,
  ListTree,
  LoaderCircle,
  MessageCircle,
  MessagesSquare,
  MoreHorizontal,
  NotebookPen,
  PanelRightClose,
  Save,
  Star,
} from "lucide-react";
import type { LiteratureReference } from "../../../types/chat";
import type { ConversationListItem } from "../../../types/conversation";
import type {
  LibraryCollection,
  LibraryItem,
  LibraryItemUpdate,
} from "../../library/types";
import {
  PanelResizeHandle,
  type PanelResizeHandleProps,
} from "../../layout/components/PanelResizeHandle";
import type { ActiveWorkNoteContext, WorkContextView } from "../types";
import { usePdfReaderBridge } from "../../pdf/context/PdfReaderContext";
import { createLiteratureReference, MAX_LINKED_LIBRARY_ITEMS } from "../../chat/utils/literatureReferences";
import type { WorkPdfDocument } from "../types";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/work-context-panel.css";

const PdfNavigatorPanel = lazy(() => import("../../pdf/components/PdfNavigatorPanel"));
const PdfAnnotationsPanel = lazy(() => import("../../pdf/components/PdfAnnotationsPanel"));
const PdfNotesPanel = lazy(() => import("../../notes/components/PdfNotesPanel"));

export type WorkContextPanelProps = {
  activeView: WorkContextView;
  resourceLabel: string;
  resourceCount: number;
  searchQuery: string;
  chatBusy: boolean;
  chatPanel: ReactNode;
  pdfDocuments: WorkPdfDocument[];
  linkedLibraryItemIds: string[];
  literatureReferenceError: string;
  conversationAvailable: boolean;
  conversations: ConversationListItem[];
  currentConversationId: string | null;
  activeNoteContext: ActiveWorkNoteContext | null;
  libraryItem: LibraryItem | null;
  collections: LibraryCollection[];
  itemSaving: boolean;
  onViewChange: (view: WorkContextView) => void;
  onClose: () => void;
  onLinkedLibraryItemIdsChange: (itemIds: string[]) => void;
  onAddLiteratureReference: (reference: LiteratureReference) => void;
  onClearLiteratureReferenceError: () => void;
  onConversationChange: (conversationId: string) => void;
  onCreateConversation: () => void;
  onSaveLibraryItem: (update: LibraryItemUpdate) => Promise<LibraryItem>;
  resize: Omit<PanelResizeHandleProps, "edge" | "label">;
};

const contextTabs = [
  { id: "info", label: "信息", icon: Info },
  { id: "navigator", label: "导航", icon: ListTree },
  { id: "annotations", label: "批注", icon: Highlighter },
  { id: "notes", label: "笔记", icon: NotebookPen },
  { id: "chat", label: "Chat", icon: MessageCircle },
] satisfies Array<{ id: WorkContextView; label: string; icon: typeof Info }>;

export function WorkContextPanel({
  activeView,
  resourceLabel,
  resourceCount,
  searchQuery,
  chatBusy,
  chatPanel,
  pdfDocuments,
  linkedLibraryItemIds,
  literatureReferenceError,
  conversationAvailable,
  conversations,
  currentConversationId,
  activeNoteContext,
  libraryItem,
  collections,
  itemSaving,
  onViewChange,
  onClose,
  onLinkedLibraryItemIdsChange,
  onAddLiteratureReference,
  onClearLiteratureReferenceError,
  onConversationChange,
  onCreateConversation,
  onSaveLibraryItem,
  resize,
}: WorkContextPanelProps) {
  const [moreOpen, setMoreOpen] = useState(false);
  const moreRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function closeMore(event: MouseEvent) {
      if (!moreRef.current?.contains(event.target as Node)) setMoreOpen(false);
    }
    document.addEventListener("mousedown", closeMore);
    return () => document.removeEventListener("mousedown", closeMore);
  }, []);

  useEffect(() => {
    if (activeNoteContext && activeView !== "chat" && activeView !== "info") {
      onViewChange("chat");
    }
  }, [activeNoteContext, activeView, onViewChange]);

  const visibleTabs = activeNoteContext
    ? contextTabs.filter((tab) => tab.id === "info" || tab.id === "chat")
    : contextTabs;

  return (
    <aside className="work-context-panel" aria-label="当前资源工具">
      <PanelResizeHandle
        {...resize}
        edge="left"
        label="调整 Work 右侧工具面板宽度"
      />

      <div className="work-context-body">
        {activeView === "chat" ? (
          <div className="work-context-chat">
            {activeNoteContext ? (
              <NoteChatScope
                note={activeNoteContext}
                conversations={conversations}
                currentConversationId={currentConversationId}
                disabled={chatBusy}
                onConversationChange={onConversationChange}
                onCreateConversation={onCreateConversation}
              />
            ) : (
              <LiteratureChatScope
                documents={pdfDocuments}
                linkedLibraryItemIds={linkedLibraryItemIds}
                error={literatureReferenceError}
                disabled={!conversationAvailable || chatBusy}
                onLinkedLibraryItemIdsChange={onLinkedLibraryItemIdsChange}
                onAddLiteratureReference={onAddLiteratureReference}
                onClearError={onClearLiteratureReferenceError}
              />
            )}
            {chatPanel}
          </div>
        ) : activeView === "info" ? (
          activeNoteContext ? (
            <NoteContextInfo note={activeNoteContext} />
          ) : libraryItem ? (
            <LibraryItemInfoPanel
              item={libraryItem}
              collections={collections}
              saving={itemSaving}
              onSave={onSaveLibraryItem}
            />
          ) : (
            <InfoPanel
              resourceLabel={resourceLabel}
              resourceCount={resourceCount}
              searchQuery={searchQuery}
            />
          )
        ) : activeView === "navigator" ? (
          <Suspense fallback={<ContextToolLoading />}>
            <PdfNavigatorPanel />
          </Suspense>
        ) : activeView === "annotations" ? (
          <Suspense fallback={<ContextToolLoading />}>
            <PdfAnnotationsPanel />
          </Suspense>
        ) : activeView === "notes" ? (
          <Suspense fallback={<ContextToolLoading />}>
            <PdfNotesPanel />
          </Suspense>
        ) : (
          <InfoPanel
            resourceLabel={resourceLabel}
            resourceCount={resourceCount}
            searchQuery={searchQuery}
          />
        )}
      </div>

      <nav className="work-context-rail" aria-label="上下文工具栏">
        <button
          className="icon-button work-context-close"
          type="button"
          title="收起右侧面板"
          aria-label="收起右侧面板"
          onClick={onClose}
        >
          <PanelRightClose size={18} />
        </button>

        <div className="work-context-tool-list" role="tablist" aria-label="上下文工具">
          {visibleTabs.map(({ id, label, icon: Icon }) => {
            const active = activeView === id;
            return (
              <button
                className={`icon-button work-context-tool${active ? " work-context-tool-active" : ""}`}
                type="button"
                role="tab"
                key={id}
                title={label}
                aria-label={label}
                aria-selected={active}
                onClick={() => onViewChange(id)}
              >
                <Icon size={17} />
                {id === "chat" && chatBusy ? (
                  <span className="work-context-status-dot" aria-label="AI 正在生成" />
                ) : null}
              </button>
            );
          })}
        </div>

        <div className="work-context-more" ref={moreRef}>
          <button
            className="icon-button work-context-tool"
            type="button"
            title="更多工具"
            aria-label="更多工具"
            aria-expanded={moreOpen}
            onClick={() => setMoreOpen((open) => !open)}
          >
            <MoreHorizontal size={17} />
          </button>
          {moreOpen ? (
            <div className="work-context-menu" role="menu">
              <button type="button" role="menuitem" disabled>
                <Link2 size={15} />
                <span>关联文献</span>
              </button>
              <button type="button" role="menuitem" disabled>
                <Files size={15} />
                <span>文件版本</span>
              </button>
              <button type="button" role="menuitem" disabled>
                <Download size={15} />
                <span>导出</span>
              </button>
            </div>
          ) : null}
        </div>
      </nav>
    </aside>
  );
}

function NoteContextInfo({ note }: { note: ActiveWorkNoteContext }) {
  return (
    <section className="work-context-info work-note-context-info" aria-label="当前笔记信息">
      <header>
        <NotebookPen size={18} />
        <div>
          <h2 title={note.noteTitle}>{note.noteTitle}</h2>
          <span>当前笔记上下文</span>
        </div>
      </header>
      <dl>
        <div><dt>资源类型</dt><dd>Markdown 笔记</dd></div>
        <div><dt>版本</dt><dd title={note.revisionHash}>{note.revisionHash}</dd></div>
        <div><dt>Chat 读取</dt><dd>Tool 按需 / 非 Tool 有界快照</dd></div>
        <div><dt>来源 PDF</dt><dd>{note.source?.sourcePdfTitle ?? "无"}</dd></div>
        <div><dt>来源页码</dt><dd>{note.source?.sourcePageIndex === null || note.source?.sourcePageIndex === undefined ? "无" : `第 ${note.source.sourcePageIndex + 1} 页`}</dd></div>
      </dl>
      <p>AI 修改只生成候选差异，确认后才写入；笔记版本变化时后端会拒绝覆盖。</p>
    </section>
  );
}

type NoteChatScopeProps = {
  note: ActiveWorkNoteContext;
  conversations: ConversationListItem[];
  currentConversationId: string | null;
  disabled: boolean;
  onConversationChange: (conversationId: string) => void;
  onCreateConversation: () => void;
};

function NoteChatScope({
  note,
  conversations,
  currentConversationId,
  disabled,
  onConversationChange,
  onCreateConversation,
}: NoteChatScopeProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const current = conversations.find((item) => item.id === currentConversationId) ?? null;
  const recent = conversations.slice(0, 12);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);

  return (
    <section className="work-note-chat-scope" aria-label="当前笔记上下文 Chat">
      <div className="work-note-context-copy">
        <NotebookPen size={15} />
        <div>
          <strong title={note.noteTitle}>{note.noteTitle}</strong>
          <span>
            当前笔记
            {note.source ? ` · ${note.source.sourcePdfTitle}${note.source.sourcePageIndex === null ? "" : ` 第 ${note.source.sourcePageIndex + 1} 页`}` : ""}
          </span>
        </div>
      </div>
      <div className="work-note-conversation-switcher" ref={menuRef}>
        <button type="button" disabled={disabled} aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>
          <MessagesSquare size={14} />
          <span title={current?.title ?? "未选择对话"}>{current?.title ?? "选择对话"}</span>
          <ChevronDown size={13} />
        </button>
        {menuOpen ? (
          <div className="work-note-conversation-menu" role="menu">
            <header>
              <strong>笔记 Chat 会话</strong>
              <button type="button" onClick={() => { onCreateConversation(); setMenuOpen(false); }}>新建</button>
            </header>
            <div>
              {recent.length > 0 ? recent.map((conversation) => (
                <button
                  className={conversation.id === currentConversationId ? "is-active" : ""}
                  type="button"
                  role="menuitemradio"
                  aria-checked={conversation.id === currentConversationId}
                  key={conversation.id}
                  onClick={() => { onConversationChange(conversation.id); setMenuOpen(false); }}
                >
                  <span><strong>{conversation.title}</strong><small>{conversation.preview}</small></span>
                  <time>{new Date(conversation.updatedAt).toLocaleDateString("zh-CN", { month: "2-digit", day: "2-digit" })}</time>
                </button>
              )) : <p>暂无会话，先新建一个当前笔记 Chat。</p>}
            </div>
            <footer>同一时间只加载一个会话，切换不会挂载第二套消息列表。</footer>
          </div>
        ) : null}
      </div>
    </section>
  );
}

function ContextToolLoading() {
  const { t } = useI18n();
  return (
    <div className="work-context-loading" role="status">
      <LoaderCircle size={18} aria-hidden="true" />
      <span>{t("common.loading")}</span>
    </div>
  );
}

type LiteratureChatScopeProps = {
  documents: WorkPdfDocument[];
  linkedLibraryItemIds: string[];
  error: string;
  disabled: boolean;
  onLinkedLibraryItemIdsChange: (itemIds: string[]) => void;
  onAddLiteratureReference: (reference: LiteratureReference) => void;
  onClearError: () => void;
};

function LiteratureChatScope({
  documents,
  linkedLibraryItemIds,
  error,
  disabled,
  onLinkedLibraryItemIdsChange,
  onAddLiteratureReference,
  onClearError,
}: LiteratureChatScopeProps) {
  const { controller } = usePdfReaderBridge();
  const [scopeOpen, setScopeOpen] = useState(false);
  const [readingPage, setReadingPage] = useState(false);
  const [readError, setReadError] = useState("");
  const scopeRef = useRef<HTMLDivElement>(null);
  const activeDocument = documents.find((document) => document.active) ?? null;
  const activeReaderAvailable = Boolean(
    controller && activeDocument && controller.itemId === activeDocument.libraryItemId,
  );
  const visibleError = readError || error;

  useEffect(() => {
    if (!scopeOpen) return;
    const close = (event: MouseEvent) => {
      if (!scopeRef.current?.contains(event.target as Node)) setScopeOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [scopeOpen]);

  const toggleDocument = (document: WorkPdfDocument) => {
    const linked = linkedLibraryItemIds.includes(document.libraryItemId);
    onClearError();
    setReadError("");
    onLinkedLibraryItemIdsChange(linked
      ? linkedLibraryItemIds.filter((itemId) => itemId !== document.libraryItemId)
      : [...linkedLibraryItemIds, document.libraryItemId]);
  };

  const addCurrentPage = async () => {
    if (!controller || !activeDocument || !activeReaderAvailable || readingPage || disabled) return;
    setReadingPage(true);
    setReadError("");
    onClearError();
    try {
      const pageIndex = Math.max(0, controller.currentPage - 1);
      const text = await controller.readPageText(pageIndex);
      const reference = createLiteratureReference({
        libraryItemId: activeDocument.libraryItemId,
        title: activeDocument.title,
        pageIndex,
        kind: "page",
        text,
      });
      if (!reference) throw new Error("当前页面没有可引用的文字内容。");
      onAddLiteratureReference(reference);
    } catch (pageError) {
      setReadError(pageError instanceof Error ? pageError.message : String(pageError));
    } finally {
      setReadingPage(false);
    }
  };

  return (
    <section className="work-literature-chat-scope" aria-label="文献 Chat 引用范围">
      <div className="work-literature-scope-row">
        <div className="work-literature-current" title={activeDocument?.title ?? "当前没有活动 PDF"}>
          <BookOpenText size={14} />
          <span>{activeDocument?.title ?? "未打开 PDF"}</span>
        </div>
        <div className="work-literature-scope-menu-wrap" ref={scopeRef}>
          <button
            className="work-literature-scope-button"
            type="button"
            disabled={disabled}
            aria-expanded={scopeOpen}
            onClick={() => setScopeOpen((open) => !open)}
          >
            <Files size={14} />
            <span>范围 {linkedLibraryItemIds.length}</span>
            <ChevronDown size={13} />
          </button>
          {scopeOpen ? (
            <div className="work-literature-scope-menu" role="menu">
              <header>
                <strong>已打开的 PDF</strong>
                <span>最多 {MAX_LINKED_LIBRARY_ITEMS} 篇</span>
                <button
                  type="button"
                  disabled={disabled || linkedLibraryItemIds.length === 0}
                  onClick={() => onLinkedLibraryItemIdsChange([])}
                >
                  清空
                </button>
              </header>
              {documents.length > 0 ? documents.map((document) => {
                const linked = linkedLibraryItemIds.includes(document.libraryItemId);
                const limitReached = !linked
                  && linkedLibraryItemIds.length >= MAX_LINKED_LIBRARY_ITEMS;
                return (
                  <button
                    type="button"
                    role="menuitemcheckbox"
                    aria-checked={linked}
                    disabled={disabled || limitReached}
                    key={document.libraryItemId}
                    onClick={() => toggleDocument(document)}
                  >
                    <span className={`work-literature-scope-check${linked ? " is-checked" : ""}`}>
                      {linked ? <Check size={12} /> : null}
                    </span>
                    <span title={document.title}>{document.title}</span>
                  </button>
                );
              }) : <p>请先在中间工作区打开 PDF。</p>}
              <footer>范围仅表示允许引用，不会自动读取全文。</footer>
            </div>
          ) : null}
        </div>
        <button
          className="work-literature-page-button"
          type="button"
          disabled={!activeReaderAvailable || disabled || readingPage}
          onClick={() => void addCurrentPage()}
        >
          {readingPage ? <LoaderCircle size={14} /> : <FileText size={14} />}
          <span>{readingPage ? "读取中" : "引用当前页"}</span>
        </button>
      </div>
      {visibleError ? (
        <div className="work-literature-scope-error" role="alert">
          <span>{visibleError}</span>
          <button type="button" aria-label="关闭文献引用错误" onClick={() => {
            setReadError("");
            onClearError();
          }}>×</button>
        </div>
      ) : null}
    </section>
  );
}

type InfoPanelProps = {
  resourceLabel: string;
  resourceCount: number;
  searchQuery: string;
};

function InfoPanel({ resourceLabel, resourceCount, searchQuery }: InfoPanelProps) {
  return (
    <section className="work-context-info" aria-label="当前资源信息">
      <header>
        <BookOpenText size={18} />
        <div>
          <h2>{resourceLabel}</h2>
          <span>文库视图</span>
        </div>
      </header>
      <dl>
        <div>
          <dt>资源类型</dt>
          <dd>文献集合</dd>
        </div>
        <div>
          <dt>项目数量</dt>
          <dd>{resourceCount}</dd>
        </div>
        <div>
          <dt>查询条件</dt>
          <dd>{searchQuery.trim() || "无"}</dd>
        </div>
        <div>
          <dt>活动文档</dt>
          <dd><FileText size={14} />未打开</dd>
        </div>
      </dl>
    </section>
  );
}

type LibraryItemInfoPanelProps = {
  item: LibraryItem;
  collections: LibraryCollection[];
  saving: boolean;
  onSave: (update: LibraryItemUpdate) => Promise<LibraryItem>;
};

type LibraryItemDraft = {
  title: string;
  authors: string;
  publicationYear: string;
  publicationTitle: string;
  doi: string;
  abstractText: string;
  tags: string;
  favorite: boolean;
  collectionIds: string[];
};

function createItemDraft(item: LibraryItem): LibraryItemDraft {
  return {
    title: item.title,
    authors: item.authors.join("，"),
    publicationYear: item.publicationYear?.toString() ?? "",
    publicationTitle: item.publicationTitle,
    doi: item.doi,
    abstractText: item.abstractText,
    tags: item.tags.join("，"),
    favorite: item.favorite,
    collectionIds: item.collectionIds,
  };
}

function LibraryItemInfoPanel({
  item,
  collections,
  saving,
  onSave,
}: LibraryItemInfoPanelProps) {
  const [draft, setDraft] = useState<LibraryItemDraft>(() => createItemDraft(item));

  useEffect(() => {
    setDraft(createItemDraft(item));
  }, [item.id, item.updatedAt]);

  const updateDraft = <Key extends keyof LibraryItemDraft>(
    key: Key,
    value: LibraryItemDraft[Key],
  ) => setDraft((current) => ({ ...current, [key]: value }));

  const save = async () => {
    const publicationYear = draft.publicationYear.trim()
      ? Number.parseInt(draft.publicationYear, 10)
      : null;
    try {
      const saved = await onSave({
        itemId: item.id,
        title: draft.title,
        authors: splitList(draft.authors),
        publicationYear: Number.isFinite(publicationYear) ? publicationYear : null,
        publicationTitle: draft.publicationTitle,
        doi: draft.doi,
        abstractText: draft.abstractText,
        favorite: draft.favorite,
        tags: splitList(draft.tags),
        collectionIds: draft.collectionIds,
      });
      setDraft(createItemDraft(saved));
    } catch {
      // 数据层已经记录可展示的错误。
    }
  };

  return (
    <section className="work-context-info work-library-item-info" aria-label={`${item.title} 文献信息`}>
      <header>
        <FileText size={18} />
        <div>
          <h2 title={item.title}>{item.title}</h2>
          <span>{item.file.originalName}</span>
        </div>
      </header>

      <form onSubmit={(event) => { event.preventDefault(); void save(); }}>
        <label>
          <span>标题</span>
          <input
            value={draft.title}
            maxLength={500}
            required
            disabled={saving || item.deletedAt !== null}
            onChange={(event) => updateDraft("title", event.target.value)}
          />
        </label>
        <label>
          <span>作者</span>
          <input
            value={draft.authors}
            placeholder="使用逗号分隔"
            disabled={saving || item.deletedAt !== null}
            onChange={(event) => updateDraft("authors", event.target.value)}
          />
        </label>
        <div className="work-library-info-pair">
          <label>
            <span>年份</span>
            <input
              value={draft.publicationYear}
              type="number"
              min="1000"
              max="3000"
              disabled={saving || item.deletedAt !== null}
              onChange={(event) => updateDraft("publicationYear", event.target.value)}
            />
          </label>
          <label>
            <span>收藏</span>
            <button
              className={`work-library-info-favorite${draft.favorite ? " work-library-info-favorite-active" : ""}`}
              type="button"
              disabled={saving || item.deletedAt !== null}
              aria-pressed={draft.favorite}
              onClick={() => updateDraft("favorite", !draft.favorite)}
            >
              <Star size={15} fill={draft.favorite ? "currentColor" : "none"} />
              <span>{draft.favorite ? "已收藏" : "未收藏"}</span>
            </button>
          </label>
        </div>
        <label>
          <span>期刊或出版物</span>
          <input
            value={draft.publicationTitle}
            disabled={saving || item.deletedAt !== null}
            onChange={(event) => updateDraft("publicationTitle", event.target.value)}
          />
        </label>
        <label>
          <span>DOI</span>
          <input
            value={draft.doi}
            disabled={saving || item.deletedAt !== null}
            onChange={(event) => updateDraft("doi", event.target.value)}
          />
        </label>
        <label>
          <span>标签</span>
          <input
            value={draft.tags}
            placeholder="使用逗号分隔"
            disabled={saving || item.deletedAt !== null}
            onChange={(event) => updateDraft("tags", event.target.value)}
          />
        </label>
        <fieldset disabled={saving || item.deletedAt !== null}>
          <legend>分类</legend>
          {collections.length > 0 ? collections.map((collection) => (
            <label className="work-library-collection-check" key={collection.id}>
              <input
                type="checkbox"
                checked={draft.collectionIds.includes(collection.id)}
                onChange={(event) => updateDraft(
                  "collectionIds",
                  event.target.checked
                    ? [...draft.collectionIds, collection.id]
                    : draft.collectionIds.filter((id) => id !== collection.id),
                )}
              />
              <span>{collection.name}</span>
            </label>
          )) : <p>暂无分类</p>}
        </fieldset>
        <label>
          <span>摘要</span>
          <textarea
            value={draft.abstractText}
            rows={5}
            disabled={saving || item.deletedAt !== null}
            onChange={(event) => updateDraft("abstractText", event.target.value)}
          />
        </label>

        <dl className="work-library-file-details">
          <div><dt>文件大小</dt><dd>{formatFileSize(item.file.fileSize)}</dd></div>
          <div><dt>文件状态</dt><dd>{item.file.available ? "可用" : "缺失"}</dd></div>
          <div><dt>文件哈希</dt><dd title={item.file.fileHash}>{item.file.fileHash.slice(0, 12)}…</dd></div>
        </dl>

        {item.deletedAt === null ? (
          <button className="work-library-info-save" type="submit" disabled={saving || !draft.title.trim()}>
            {saving ? <LoaderCircle size={15} /> : <Save size={15} />}
            <span>{saving ? "正在保存" : "保存信息"}</span>
          </button>
        ) : null}
      </form>
    </section>
  );
}

function splitList(value: string): string[] {
  return value
    .split(/[,，;；\n]/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
