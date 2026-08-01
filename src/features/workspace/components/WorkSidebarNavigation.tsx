import { useEffect, useRef, useState } from "react";
import {
  BookOpenText,
  Check,
  ChevronDown,
  ChevronRight,
  Clock3,
  FilePlus2,
  Folder,
  FolderPlus,
  FolderTree,
  Inbox,
  MoreHorizontal,
  Network,
  NotebookPen,
  Pencil,
  Search,
  Star,
  Trash2,
  X,
} from "lucide-react";
import type { LibraryCollection } from "../../library/types";
import type { WorkLibraryView } from "../types";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/work-sidebar-navigation.css";

type WorkSidebarNavigationProps = {
  collapsed: boolean;
  activeView: WorkLibraryView;
  searchQuery: string;
  collections: LibraryCollection[];
  selectedCollectionId: string | null;
  busy: boolean;
  runtimeAvailable: boolean;
  onViewChange: (view: WorkLibraryView) => void;
  onSearchQueryChange: (query: string) => void;
  onCollectionSelect: (collectionId: string) => void;
  onImport: () => Promise<unknown>;
  onCreateCollection: (name: string) => Promise<LibraryCollection>;
  onRenameCollection: (collectionId: string, name: string) => Promise<void>;
  onDeleteCollection: (collectionId: string) => Promise<boolean>;
};

export function WorkSidebarNavigation({
  collapsed,
  activeView,
  searchQuery,
  collections,
  selectedCollectionId,
  busy,
  runtimeAvailable,
  onViewChange,
  onSearchQueryChange,
  onCollectionSelect,
  onImport,
  onCreateCollection,
  onRenameCollection,
  onDeleteCollection,
}: WorkSidebarNavigationProps) {
  const { t } = useI18n();
  const primaryViews = [
    { id: "all" as const, label: t("work.all"), icon: BookOpenText },
    { id: "recent" as const, label: t("work.recent"), icon: Clock3 },
    { id: "favorites" as const, label: t("work.favorites"), icon: Star },
    { id: "unfiled" as const, label: t("work.unfiled"), icon: Inbox },
  ];
  const outcomeViews = [
    { id: "notes" as const, label: t("work.notes"), icon: NotebookPen },
    { id: "mind-maps" as const, label: t("work.mindMaps"), icon: Network },
  ];
  const [collectionsOpen, setCollectionsOpen] = useState(true);
  const [outcomesOpen, setOutcomesOpen] = useState(true);
  const [creatingCollection, setCreatingCollection] = useState(false);
  const [collectionName, setCollectionName] = useState("");
  const [renamingCollectionId, setRenamingCollectionId] = useState<string | null>(null);
  const [collectionMenuId, setCollectionMenuId] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function closeMenu(event: MouseEvent) {
      if (!menuRef.current?.contains(event.target as Node)) setCollectionMenuId(null);
    }
    document.addEventListener("mousedown", closeMenu);
    return () => document.removeEventListener("mousedown", closeMenu);
  }, []);

  useEffect(() => {
    if (!collapsed) return;
    setCreatingCollection(false);
    setRenamingCollectionId(null);
    setCollectionMenuId(null);
  }, [collapsed]);

  const submitCollection = async () => {
    const name = collectionName.trim();
    if (!name) return;
    try {
      if (renamingCollectionId) {
        await onRenameCollection(renamingCollectionId, name);
      } else {
        await onCreateCollection(name);
      }
    } catch {
      return;
    }
    setCollectionName("");
    setCreatingCollection(false);
    setRenamingCollectionId(null);
    setCollectionMenuId(null);
  };

  const cancelCollectionEditor = () => {
    setCollectionName("");
    setCreatingCollection(false);
    setRenamingCollectionId(null);
  };

  return (
    <section
      className={`work-sidebar-navigation${collapsed ? " work-sidebar-navigation-collapsed" : ""}`}
      aria-label={t("work.navigation")}
      ref={menuRef}
    >
      <div className="work-library-actions">
        <button
          type="button"
          title={runtimeAvailable ? t("work.importPdf") : t("work.desktopImport")}
          disabled={busy || !runtimeAvailable}
          onClick={() => void onImport().catch(() => undefined)}
        >
          <FilePlus2 size={17} />
          <span>{t("work.import")}</span>
        </button>
        <button
          type="button"
          title={t("work.newCollection")}
          disabled={busy || !runtimeAvailable}
          onClick={() => {
            setCreatingCollection(true);
            setRenamingCollectionId(null);
            setCollectionName("");
            setCollectionsOpen(true);
          }}
        >
          <FolderPlus size={17} />
          <span>{t("work.collection")}</span>
        </button>
      </div>

      <label className="work-library-search">
        <Search size={16} aria-hidden="true" />
        <input
          type="search"
          value={searchQuery}
          placeholder={t("work.searchPlaceholder")}
          aria-label={t("work.searchPlaceholder")}
          onChange={(event) => onSearchQueryChange(event.target.value)}
        />
      </label>

      <div className="work-library-tree">
        <section className="work-tree-group" aria-label={t("work.myLibrary")}>
          <div className="work-tree-heading">
            <BookOpenText size={15} />
            <strong>{t("work.myLibrary")}</strong>
          </div>
          <nav className="work-tree-items">
            {primaryViews.map(({ id, label, icon: Icon }) => {
              const active = activeView === id && selectedCollectionId === null;
              return (
                <button
                  className={`work-tree-item${active ? " work-tree-item-active" : ""}`}
                  type="button"
                  title={collapsed ? label : undefined}
                  aria-current={active ? "page" : undefined}
                  key={id}
                  onClick={() => onViewChange(id)}
                >
                  <Icon size={16} />
                  <span>{label}</span>
                </button>
              );
            })}
          </nav>
        </section>

        <section className="work-tree-group" aria-label={t("work.collectionSection")}>
          <button
            className="work-tree-heading work-tree-heading-button"
            type="button"
            aria-expanded={collectionsOpen}
            onClick={() => setCollectionsOpen((open) => !open)}
          >
            {collectionsOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <FolderTree size={15} />
            <strong>{t("work.collectionSection")}</strong>
          </button>
          {collectionsOpen && !collapsed ? (
            <div className="work-collection-list">
              {creatingCollection || renamingCollectionId ? (
                <div className="work-collection-editor">
                  <Folder size={15} />
                  <input
                    autoFocus
                    value={collectionName}
                    maxLength={120}
                    aria-label={renamingCollectionId ? t("work.newCollectionName") : t("work.collectionName")}
                    placeholder={t("work.collectionName")}
                    onChange={(event) => setCollectionName(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void submitCollection();
                      if (event.key === "Escape") cancelCollectionEditor();
                    }}
                  />
                  <button
                    type="button"
                    title={t("work.saveCollection")}
                    disabled={busy || !collectionName.trim()}
                    onClick={() => void submitCollection()}
                  >
                    <Check size={14} />
                  </button>
                  <button type="button" title={t("common.cancel")} onClick={cancelCollectionEditor}>
                    <X size={14} />
                  </button>
                </div>
              ) : null}

              {collections.map((collection) => {
                const active = selectedCollectionId === collection.id;
                return (
                  <div className="work-collection-item-wrap" key={collection.id}>
                    <button
                      className={`work-tree-item work-collection-item${active ? " work-tree-item-active" : ""}`}
                      type="button"
                      aria-current={active ? "page" : undefined}
                      title={collection.name}
                      onClick={() => onCollectionSelect(collection.id)}
                    >
                      <Folder size={15} />
                      <span>{collection.name}</span>
                      <small>{collection.itemCount}</small>
                    </button>
                    <button
                      className="work-collection-more"
                      type="button"
                      title={t("work.collectionActions")}
                      aria-expanded={collectionMenuId === collection.id}
                      onClick={() => setCollectionMenuId((current) => (
                        current === collection.id ? null : collection.id
                      ))}
                    >
                      <MoreHorizontal size={14} />
                    </button>
                    {collectionMenuId === collection.id ? (
                      <div className="work-collection-menu" role="menu">
                        <button
                          type="button"
                          role="menuitem"
                          onClick={() => {
                            setRenamingCollectionId(collection.id);
                            setCollectionName(collection.name);
                            setCreatingCollection(false);
                            setCollectionMenuId(null);
                          }}
                        >
                          <Pencil size={14} />
                          <span>{t("common.rename")}</span>
                        </button>
                        <button
                          className="work-collection-menu-danger"
                          type="button"
                          role="menuitem"
                          onClick={() => {
                            setCollectionMenuId(null);
                            if (!window.confirm(t("work.deleteCollectionConfirm", { name: collection.name }))) return;
                            void onDeleteCollection(collection.id).catch(() => false);
                          }}
                        >
                          <Trash2 size={14} />
                          <span>{t("work.deleteCollection")}</span>
                        </button>
                      </div>
                    ) : null}
                  </div>
                );
              })}
              {collections.length === 0 && !creatingCollection ? (
                <div className="work-tree-empty">{t("work.noCollections")}</div>
              ) : null}
            </div>
          ) : null}
        </section>

        <section className="work-tree-group" aria-label={t("work.learningOutcomes")}>
          <button
            className="work-tree-heading work-tree-heading-button"
            type="button"
            aria-expanded={outcomesOpen}
            onClick={() => setOutcomesOpen((open) => !open)}
          >
            {outcomesOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <NotebookPen size={15} />
            <strong>{t("work.learningOutcomes")}</strong>
          </button>
          {outcomesOpen ? (
            <nav className="work-tree-items">
              {outcomeViews.map(({ id, label, icon: Icon }) => (
                <button
                  className={`work-tree-item${activeView === id ? " work-tree-item-active" : ""}`}
                  type="button"
                  title={collapsed ? label : undefined}
                  aria-current={activeView === id ? "page" : undefined}
                  key={id}
                  onClick={() => onViewChange(id)}
                >
                  <Icon size={16} />
                  <span>{label}</span>
                </button>
              ))}
            </nav>
          ) : null}
        </section>

        <button
          className={`work-tree-item work-tree-trash${activeView === "trash" && !selectedCollectionId ? " work-tree-item-active" : ""}`}
          type="button"
          title={collapsed ? t("work.trash") : undefined}
          aria-current={activeView === "trash" && !selectedCollectionId ? "page" : undefined}
          onClick={() => onViewChange("trash")}
        >
          <Trash2 size={16} />
          <span>{t("work.trash")}</span>
        </button>
      </div>
    </section>
  );
}
