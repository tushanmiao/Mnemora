import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  BookOpenText,
  Download,
  FileText,
  Files,
  Highlighter,
  Info,
  Link2,
  ListTree,
  LoaderCircle,
  MessageCircle,
  MoreHorizontal,
  NotebookPen,
  PanelRightClose,
  Save,
  Star,
} from "lucide-react";
import type {
  LibraryCollection,
  LibraryItem,
  LibraryItemUpdate,
} from "../../library/types";
import {
  PanelResizeHandle,
  type PanelResizeHandleProps,
} from "../../layout/components/PanelResizeHandle";
import type { WorkContextView } from "../types";
import { PdfNotesPanel } from "../../notes/components/PdfNotesPanel";
import { PdfAnnotationsPanel } from "../../pdf/components/PdfAnnotationsPanel";
import { PdfNavigatorPanel } from "../../pdf/components/PdfNavigatorPanel";
import "../styles/work-context-panel.css";

type WorkContextPanelProps = {
  activeView: WorkContextView;
  resourceLabel: string;
  resourceCount: number;
  searchQuery: string;
  chatBusy: boolean;
  chatPanel: ReactNode;
  libraryItem: LibraryItem | null;
  collections: LibraryCollection[];
  itemSaving: boolean;
  onViewChange: (view: WorkContextView) => void;
  onClose: () => void;
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
  libraryItem,
  collections,
  itemSaving,
  onViewChange,
  onClose,
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

  return (
    <aside className="work-context-panel" aria-label="当前资源工具">
      <PanelResizeHandle
        {...resize}
        edge="left"
        label="调整 Work 右侧工具面板宽度"
      />

      <div className="work-context-body">
        {activeView === "chat" ? (
          <div className="work-context-chat">{chatPanel}</div>
        ) : activeView === "info" ? (
          libraryItem ? (
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
          <PdfNavigatorPanel />
        ) : activeView === "annotations" ? (
          <PdfAnnotationsPanel />
        ) : activeView === "notes" ? (
          <PdfNotesPanel />
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
          {contextTabs.map(({ id, label, icon: Icon }) => {
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
