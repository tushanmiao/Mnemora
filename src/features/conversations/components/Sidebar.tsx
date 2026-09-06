import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  BookOpenText,
  Boxes,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Download,
  FilePenLine,
  FileJson,
  FileText,
  Folder,
  FolderInput,
  Layers3,
  LoaderCircle,
  MessageCircle,
  MessageSquarePlus,
  MoreHorizontal,
  NotebookPen,
  Pencil,
  Pin,
  Plug,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import type { ConversationListItem } from "../../../types/conversation";
import type { LibraryCollection } from "../../library/types";
import {
  PanelResizeHandle,
  type PanelResizeHandleProps,
} from "../../layout/components/PanelResizeHandle";
import { WorkSidebarNavigation } from "../../workspace/components/WorkSidebarNavigation";
import type { WorkLibraryView, WorkspaceMode } from "../../workspace/types";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/sidebar.css";

type SidebarProps = {
  mode: WorkspaceMode;
  workLibraryView: WorkLibraryView;
  workSearchQuery: string;
  workCollections: LibraryCollection[];
  workSelectedCollectionId: string | null;
  workLibraryBusy: boolean;
  workLibraryRuntimeAvailable: boolean;
  collapsed: boolean;
  userDisplayName: string;
  userAvatar: string;
  conversations: ConversationListItem[];
  conversationListLoading: boolean;
  conversationListError: string;
  conversationListHasMore: boolean;
  currentConversationId: string | null;
  onCreateConversation: () => void;
  onSelectConversation: (conversationId: string) => void;
  onDeleteConversation: (conversationId: string) => void;
  onRenameConversation: (conversationId: string, title: string) => Promise<boolean>;
  onExportConversation: (conversationId: string, format: "markdown" | "json") => void;
  onSaveConversationAsNote: (conversationId: string) => void;
  onSummarizeConversationToNote: (conversationId: string) => void;
  onGenerateDeepNote: (conversationId: string) => void;
  onUpdateExistingNote: (conversationId: string) => void;
  onClearConversations: () => void;
  onLoadMoreConversations: () => void;
  onOpenSkills: () => void;
  onOpenKnowledge: () => void;
  onOpenPlugins: () => void;
  onWorkLibraryViewChange: (view: WorkLibraryView) => void;
  onWorkSearchQueryChange: (query: string) => void;
  onWorkCollectionSelect: (collectionId: string) => void;
  onWorkImport: () => Promise<unknown>;
  onWorkCreateCollection: (name: string) => Promise<LibraryCollection>;
  onWorkRenameCollection: (collectionId: string, name: string) => Promise<void>;
  onWorkDeleteCollection: (collectionId: string) => Promise<boolean>;
  onToggleCollapse: () => void;
  resize: Omit<PanelResizeHandleProps, "edge" | "label">;
};

export function Sidebar({
  mode,
  workLibraryView,
  workSearchQuery,
  workCollections,
  workSelectedCollectionId,
  workLibraryBusy,
  workLibraryRuntimeAvailable,
  collapsed,
  userDisplayName,
  userAvatar,
  conversations,
  conversationListLoading,
  conversationListError,
  conversationListHasMore,
  currentConversationId,
  onCreateConversation,
  onSelectConversation,
  onDeleteConversation,
  onRenameConversation,
  onExportConversation,
  onSaveConversationAsNote,
  onSummarizeConversationToNote,
  onGenerateDeepNote,
  onUpdateExistingNote,
  onClearConversations,
  onLoadMoreConversations,
  onOpenSkills,
  onOpenKnowledge,
  onOpenPlugins,
  onWorkLibraryViewChange,
  onWorkSearchQueryChange,
  onWorkCollectionSelect,
  onWorkImport,
  onWorkCreateCollection,
  onWorkRenameCollection,
  onWorkDeleteCollection,
  onToggleCollapse,
  resize,
}: SidebarProps) {
  const { t } = useI18n();
  const extensionItems = [
    { id: "skills", label: t("sidebar.skills"), icon: Sparkles },
    { id: "knowledge", label: t("sidebar.knowledge"), icon: BookOpenText },
    { id: "plugins", label: t("sidebar.plugins"), icon: Plug },
  ];
  const normalizedDisplayName = userDisplayName.trim() || t("common.user");
  const avatarInitial = (Array.from(normalizedDisplayName)[0] ?? "M").toUpperCase();
  // 扩展是可选工具，不在打开 Chat 时主动展开，避免占用对话列表空间。
  const [extensionsOpen, setExtensionsOpen] = useState(false);
  const [activeSection, setActiveSection] = useState<"recent" | "collections" | "projects">("recent");
  const [listMenuOpen, setListMenuOpen] = useState(false);
  const [conversationMenu, setConversationMenu] = useState<string | null>(null);
  const [sidebarPopover, setSidebarPopover] = useState<{
    kind: "extensions" | "conversations";
    anchor: "extensions" | "search" | "conversation";
  } | null>(null);
  const [conversationQuery, setConversationQuery] = useState("");
  const [pickerMenuPosition, setPickerMenuPosition] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const menuAreaRef = useRef<HTMLDivElement>(null);
  const loadMoreRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function closeMenus(event: MouseEvent) {
      if (!menuAreaRef.current?.contains(event.target as Node)) {
        setListMenuOpen(false);
        setConversationMenu(null);
        setPickerMenuPosition(null);
        setSidebarPopover(null);
        setConversationQuery("");
      }
    }

    document.addEventListener("mousedown", closeMenus);
    return () => document.removeEventListener("mousedown", closeMenus);
  }, []);

  useEffect(() => {
    if (!collapsed) {
      setSidebarPopover(null);
      setConversationQuery("");
    } else {
      setExtensionsOpen(false);
    }
    setListMenuOpen(false);
    setConversationMenu(null);
    setPickerMenuPosition(null);
  }, [collapsed]);

  useEffect(() => {
    setExtensionsOpen(false);
    setListMenuOpen(false);
    setConversationMenu(null);
    setPickerMenuPosition(null);
    setSidebarPopover(null);
    setConversationQuery("");
  }, [mode]);

  useEffect(() => {
    if (!sidebarPopover) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setSidebarPopover(null);
      setConversationMenu(null);
      setPickerMenuPosition(null);
      setConversationQuery("");
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [sidebarPopover]);

  const closeSidebarPopover = () => {
    setSidebarPopover(null);
    setConversationMenu(null);
    setPickerMenuPosition(null);
    setConversationQuery("");
  };

  const openConversationPicker = (anchor: "search" | "conversation") => {
    if (sidebarPopover?.kind === "conversations" && sidebarPopover.anchor === anchor) {
      closeSidebarPopover();
      return;
    }
    setConversationMenu(null);
    setPickerMenuPosition(null);
    setListMenuOpen(false);
    setExtensionsOpen(false);
    setConversationQuery("");
    setSidebarPopover({ kind: "conversations", anchor });
  };

  const openExtensionsPicker = () => {
    if (sidebarPopover?.kind === "extensions") {
      closeSidebarPopover();
      return;
    }
    setConversationMenu(null);
    setPickerMenuPosition(null);
    setListMenuOpen(false);
    setSidebarPopover({ kind: "extensions", anchor: "extensions" });
  };

  useEffect(() => {
    const target = loadMoreRef.current;
    if (
      !target
      || collapsed
      || activeSection !== "recent"
      || conversationListLoading
      || Boolean(conversationListError)
      || !conversationListHasMore
      || typeof IntersectionObserver === "undefined"
    ) return;

    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) onLoadMoreConversations();
    }, { rootMargin: "120px 0px" });
    observer.observe(target);
    return () => observer.disconnect();
  }, [
    activeSection,
    collapsed,
    conversationListHasMore,
    conversationListError,
    conversationListLoading,
    onLoadMoreConversations,
  ]);

  const conversationPicker = (
    <ConversationPicker
      t={t}
      conversations={conversations}
      loading={conversationListLoading}
      error={conversationListError}
      hasMore={conversationListHasMore}
      currentConversationId={currentConversationId}
      query={conversationQuery}
      openConversationMenu={conversationMenu}
      onQueryChange={setConversationQuery}
      onSelect={(conversationId) => {
        onSelectConversation(conversationId);
        closeSidebarPopover();
      }}
      onCreate={() => {
        onCreateConversation();
        closeSidebarPopover();
      }}
      onLoadMore={onLoadMoreConversations}
      pickerMenuPosition={pickerMenuPosition}
      onOpenConversationMenu={(conversationId, anchor) => {
        if (conversationMenu === conversationId) {
          setConversationMenu(null);
          setPickerMenuPosition(null);
          return;
        }
        setConversationMenu(conversationId);
        setPickerMenuPosition(resolvePickerMenuPosition(anchor.getBoundingClientRect()));
        setListMenuOpen(false);
      }}
      onCloseConversationMenu={() => {
        setConversationMenu(null);
        setPickerMenuPosition(null);
      }}
      onExport={(conversationId, format) => {
        closeSidebarPopover();
        onExportConversation(conversationId, format);
      }}
      onRename={(conversation) => {
        const title = window.prompt(t("sidebar.rename"), conversation.title);
        if (title === null || title.trim() === conversation.title) return;
        closeSidebarPopover();
        void onRenameConversation(conversation.id, title).catch((error) => {
          window.alert(error instanceof Error ? error.message : String(error));
        });
      }}
      onSaveAsNote={(conversationId) => {
        closeSidebarPopover();
        onSaveConversationAsNote(conversationId);
      }}
      onSummarizeToNote={(conversationId) => {
        closeSidebarPopover();
        onSummarizeConversationToNote(conversationId);
      }}
      onGenerateDeepNote={(conversationId) => {
        closeSidebarPopover();
        onGenerateDeepNote(conversationId);
      }}
      onUpdateExistingNote={(conversationId) => {
        closeSidebarPopover();
        onUpdateExistingNote(conversationId);
      }}
      onDelete={(conversationId) => {
        closeSidebarPopover();
        onDeleteConversation(conversationId);
      }}
    />
  );

  return (
    <aside
      className={`sidebar${collapsed ? " sidebar-collapsed" : ""}`}
      aria-label={t("sidebar.primary")}
      ref={menuAreaRef}
    >
      <div className="sidebar-brand">
        <div className="brand-mark" aria-hidden="true">
          M
        </div>
        <span className="sidebar-brand-name">Mnemora</span>
        <button
          className="icon-button sidebar-collapse"
          type="button"
          title={collapsed ? t("sidebar.expand") : t("sidebar.collapse")}
          aria-label={collapsed ? t("sidebar.expand") : t("sidebar.collapse")}
          aria-expanded={!collapsed}
          onClick={onToggleCollapse}
        >
          {collapsed ? <ChevronRight size={18} /> : <ChevronLeft size={18} />}
        </button>
      </div>

      {mode === "chat" ? (
        <>
          <nav className="sidebar-actions" aria-label={t("sidebar.primary")}>
            <button
              className="sidebar-action sidebar-action-primary"
              type="button"
              title={collapsed ? t("sidebar.newChat") : undefined}
              onClick={() => {
                closeSidebarPopover();
                setExtensionsOpen(false);
                onCreateConversation();
              }}
            >
              <MessageSquarePlus size={18} />
              <span>{t("sidebar.newChat")}</span>
            </button>
            <div className="sidebar-action-anchor">
              <button
                className={`sidebar-action${
                  sidebarPopover?.kind === "conversations" && sidebarPopover.anchor === "search"
                    ? " sidebar-action-active"
                    : ""
                }`}
                type="button"
                title={collapsed ? t("sidebar.search") : undefined}
                aria-haspopup="dialog"
                aria-expanded={sidebarPopover?.kind === "conversations" && sidebarPopover.anchor === "search"}
                onClick={() => openConversationPicker("search")}
              >
                <Search size={18} />
                <span>{t("sidebar.search")}</span>
              </button>
              {sidebarPopover?.kind === "conversations" && sidebarPopover.anchor === "search"
                ? conversationPicker
                : null}
            </div>
            <div className="sidebar-action-anchor">
              <button
                className={`sidebar-action${
                  collapsed && sidebarPopover?.kind === "extensions" ? " sidebar-action-active" : ""
                }`}
                type="button"
                title={collapsed ? t("sidebar.extensions") : undefined}
                aria-haspopup={collapsed ? "dialog" : undefined}
                aria-expanded={collapsed
                  ? sidebarPopover?.kind === "extensions"
                  : extensionsOpen}
                onClick={() => {
                  if (collapsed) {
                    openExtensionsPicker();
                    return;
                  }
                  closeSidebarPopover();
                  setExtensionsOpen((open) => !open);
                }}
              >
                <Boxes size={18} />
                <span>{t("sidebar.extensions")}</span>
                <ChevronDown className={`sidebar-chevron${extensionsOpen ? " sidebar-chevron-open" : ""}`} size={16} />
              </button>
              {collapsed && sidebarPopover?.kind === "extensions" ? (
                <ExtensionPicker
                  items={extensionItems}
                  onOpenSkills={() => {
                    closeSidebarPopover();
                    onOpenSkills();
                  }}
                  onOpenKnowledge={() => {
                    closeSidebarPopover();
                    onOpenKnowledge();
                  }}
                  onOpenPlugins={() => {
                    closeSidebarPopover();
                    onOpenPlugins();
                  }}
                />
              ) : null}
            </div>

            {extensionsOpen && !collapsed ? (
              <div className="extension-list">
                {extensionItems.map(({ id, label, icon: Icon }) => (
                  <button
                    className="extension-item"
                    type="button"
                    key={id}
                    onClick={() => {
                      setExtensionsOpen(false);
                      if (id === "skills") onOpenSkills();
                      if (id === "knowledge") onOpenKnowledge();
                      if (id === "plugins") onOpenPlugins();
                    }}
                  >
                    <Icon size={15} />
                    <span>{label}</span>
                  </button>
                ))}
              </div>
            ) : null}
          </nav>

          <div className="sidebar-divider" />

          {collapsed ? (
            <section className="collapsed-conversation-section" aria-label={t("sidebar.conversationCategories")}>
              <div className="collapsed-sidebar-divider" aria-hidden="true" />
              <div className="collapsed-conversation-anchor">
                <button
                  className={`sidebar-action collapsed-conversation-button${
                    sidebarPopover?.kind === "conversations" && sidebarPopover.anchor === "conversation"
                      ? " sidebar-action-active"
                      : ""
                  }`}
                  type="button"
                  title={t("sidebar.switchConversation")}
                  aria-label={t("sidebar.switchConversation")}
                  aria-haspopup="dialog"
                  aria-expanded={sidebarPopover?.kind === "conversations" && sidebarPopover.anchor === "conversation"}
                  onClick={() => openConversationPicker("conversation")}
                >
                  <MessageCircle size={18} />
                  <span>{t("sidebar.conversations")}</span>
                </button>
                {sidebarPopover?.kind === "conversations" && sidebarPopover.anchor === "conversation"
                  ? conversationPicker
                  : null}
              </div>
            </section>
          ) : (
          <section className="conversation-section" aria-label={t("sidebar.conversationCategories")}>
        <div className="conversation-tabs">
          <button
            className={activeSection === "recent" ? "conversation-tab conversation-tab-active" : "conversation-tab"}
            type="button"
            onClick={() => setActiveSection("recent")}
          >
            {t("sidebar.recent")}
          </button>
          <button
            className={activeSection === "collections" ? "conversation-tab conversation-tab-active" : "conversation-tab"}
            type="button"
            onClick={() => setActiveSection("collections")}
          >
            {t("sidebar.collections")}
          </button>
          <button
            className={activeSection === "projects" ? "conversation-tab conversation-tab-active" : "conversation-tab"}
            type="button"
            onClick={() => setActiveSection("projects")}
          >
            {t("sidebar.projects")}
          </button>

          <div className="conversation-list-actions">
            <button
              className="icon-button"
              type="button"
              title={t("sidebar.listActions")}
              aria-expanded={listMenuOpen}
              onClick={() => {
                setListMenuOpen((open) => !open);
                setConversationMenu(null);
              }}
            >
              <MoreHorizontal size={17} />
            </button>
            <button
              className="icon-button"
              type="button"
              title={t("sidebar.newChat")}
              onClick={onCreateConversation}
            >
              <MessageSquarePlus size={16} />
            </button>

            {listMenuOpen ? (
              <div className="sidebar-menu list-menu" role="menu">
                <button
                  className="sidebar-menu-item sidebar-menu-danger"
                  type="button"
                  role="menuitem"
                  disabled={conversations.length === 0}
                  onClick={() => {
                    setListMenuOpen(false);
                    onClearConversations();
                  }}
                >
                  <Trash2 size={16} />
                  <span>{t("sidebar.clearAll")}</span>
                </button>
              </div>
            ) : null}
          </div>
        </div>

        <div className="conversation-list">
          {activeSection === "recent" ? (
            <>
              {conversations.map((conversation) => (
                <div className="conversation-item-wrap" key={conversation.id}>
                  <button
                    className={`conversation-item${
                      currentConversationId === conversation.id ? " conversation-item-active" : ""
                    }`}
                    type="button"
                    aria-current={currentConversationId === conversation.id ? "page" : undefined}
                    title={conversation.title}
                    onClick={() => {
                      onSelectConversation(conversation.id);
                      setConversationMenu(null);
                    }}
                  >
                    <FileText size={16} />
                    <span>{conversation.title}</span>
                  </button>
                  <button
                    className="conversation-more"
                    type="button"
                    title={t("sidebar.conversationActions")}
                    aria-expanded={conversationMenu === conversation.id}
                    onClick={() => {
                      setConversationMenu((current) =>
                        current === conversation.id ? null : conversation.id,
                      );
                      setPickerMenuPosition(null);
                      setListMenuOpen(false);
                    }}
                  >
                    <MoreHorizontal size={16} />
                  </button>

                  {conversationMenu === conversation.id ? (
                    <ConversationMenu t={t}
                      onRename={() => {
                        setConversationMenu(null);
                        const title = window.prompt(t("sidebar.rename"), conversation.title);
                        if (title === null || title.trim() === conversation.title) return;
                        void onRenameConversation(conversation.id, title).catch((error) => {
                          window.alert(error instanceof Error ? error.message : String(error));
                        });
                      }}
                      onExport={(format) => {
                        setConversationMenu(null);
                        onExportConversation(conversation.id, format);
                      }}
                      onSaveAsNote={() => {
                        setConversationMenu(null);
                        onSaveConversationAsNote(conversation.id);
                      }}
                      onSummarizeToNote={() => {
                        setConversationMenu(null);
                        onSummarizeConversationToNote(conversation.id);
                      }}
                      onGenerateDeepNote={() => {
                        setConversationMenu(null);
                        onGenerateDeepNote(conversation.id);
                      }}
                      onUpdateExistingNote={() => {
                        setConversationMenu(null);
                        onUpdateExistingNote(conversation.id);
                      }}
                      onDelete={() => {
                        setConversationMenu(null);
                        onDeleteConversation(conversation.id);
                      }}
                    />
                  ) : null}
                </div>
              ))}
              {conversations.length === 0 && !conversationListLoading ? (
                <button className="empty-section-action" type="button" onClick={onCreateConversation}>
                  <MessageSquarePlus size={17} />
                  <span>{t("sidebar.firstConversation")}</span>
                </button>
              ) : null}
              {conversationListLoading || conversationListHasMore || conversationListError ? (
                <div className="conversation-load-more" ref={loadMoreRef}>
                  {conversationListError ? (
                    <button type="button" onClick={onLoadMoreConversations}>{t("sidebar.retryLoad")}</button>
                  ) : conversationListLoading ? (
                    <span role="status">
                      <LoaderCircle size={15} />
                      {t("common.loading")}
                    </span>
                  ) : (
                    <button type="button" onClick={onLoadMoreConversations}>{t("sidebar.loadMore")}</button>
                  )}
                </div>
              ) : null}
            </>
          ) : activeSection === "projects" ? (
            <button className="empty-section-action" type="button">
              <Folder size={17} />
              <span>{t("sidebar.firstProject")}</span>
            </button>
          ) : (
            <button className="empty-section-action" type="button">
              <Layers3 size={17} />
              <span>{t("sidebar.firstCollection")}</span>
            </button>
          )}
        </div>
          </section>
          )}
        </>
      ) : (
        <WorkSidebarNavigation
          collapsed={collapsed}
          activeView={workLibraryView}
          searchQuery={workSearchQuery}
          collections={workCollections}
          selectedCollectionId={workSelectedCollectionId}
          busy={workLibraryBusy}
          runtimeAvailable={workLibraryRuntimeAvailable}
          onViewChange={onWorkLibraryViewChange}
          onSearchQueryChange={onWorkSearchQueryChange}
          onCollectionSelect={onWorkCollectionSelect}
          onImport={onWorkImport}
          onCreateCollection={onWorkCreateCollection}
          onRenameCollection={onWorkRenameCollection}
          onDeleteCollection={onWorkDeleteCollection}
        />
      )}

      <div className="sidebar-footer">
        <div className="user-profile">
          <div className="user-avatar" aria-hidden="true">
            {userAvatar ? <img src={userAvatar} alt="" /> : avatarInitial}
          </div>
          <div className="user-meta">
            <strong title={normalizedDisplayName}>{normalizedDisplayName}</strong>
            <span>{t("sidebar.localWorkspace")}</span>
          </div>
        </div>
      </div>

      {!collapsed ? (
        <PanelResizeHandle
          {...resize}
          edge="right"
          label={mode === "chat" ? t("common.resizeChatSidebar") : t("common.resizeWorkSidebar")}
        />
      ) : null}
    </aside>
  );
}

type ExtensionPickerItem = {
  id: string;
  label: string;
  icon: typeof Sparkles;
};

function ExtensionPicker({
  items,
  onOpenSkills,
  onOpenKnowledge,
  onOpenPlugins,
}: {
  items: ExtensionPickerItem[];
  onOpenSkills: () => void;
  onOpenKnowledge: () => void;
  onOpenPlugins: () => void;
}) {
  const { t } = useI18n();
  return (
    <section
      className="sidebar-popover sidebar-extension-popover"
      role="dialog"
      aria-labelledby="sidebar-extension-picker-title"
    >
      <header className="sidebar-popover-heading">
        <div>
          <strong id="sidebar-extension-picker-title">{t("sidebar.extensions")}</strong>
          <span>{t("sidebar.extensionsDescription")}</span>
        </div>
      </header>
      <div className="sidebar-extension-grid">
        {items.map(({ id, label, icon: Icon }) => (
          <button
            type="button"
            key={id}
            onClick={id === "skills" ? onOpenSkills : id === "knowledge" ? onOpenKnowledge : onOpenPlugins}
          >
            <Icon size={17} />
            <span>{label}</span>
          </button>
        ))}
      </div>
    </section>
  );
}

type ConversationPickerProps = {
  t: ReturnType<typeof useI18n>["t"];
  conversations: ConversationListItem[];
  loading: boolean;
  error: string;
  hasMore: boolean;
  currentConversationId: string | null;
  query: string;
  openConversationMenu: string | null;
  pickerMenuPosition: { top: number; left: number } | null;
  onQueryChange: (query: string) => void;
  onSelect: (conversationId: string) => void;
  onCreate: () => void;
  onLoadMore: () => void;
  onOpenConversationMenu: (conversationId: string, anchor: HTMLButtonElement) => void;
  onCloseConversationMenu: () => void;
  onExport: (conversationId: string, format: "markdown" | "json") => void;
  onRename: (conversation: ConversationListItem) => void;
  onSaveAsNote: (conversationId: string) => void;
  onSummarizeToNote: (conversationId: string) => void;
  onGenerateDeepNote: (conversationId: string) => void;
  onUpdateExistingNote: (conversationId: string) => void;
  onDelete: (conversationId: string) => void;
};

function ConversationPicker({
  t,
  conversations,
  loading,
  error,
  hasMore,
  currentConversationId,
  query,
  openConversationMenu,
  pickerMenuPosition,
  onQueryChange,
  onSelect,
  onCreate,
  onLoadMore,
  onOpenConversationMenu,
  onCloseConversationMenu,
  onExport,
  onRename,
  onSaveAsNote,
  onSummarizeToNote,
  onGenerateDeepNote,
  onUpdateExistingNote,
  onDelete,
}: ConversationPickerProps) {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filtered = normalizedQuery
    ? conversations.filter((conversation) => (
        conversation.title.toLocaleLowerCase().includes(normalizedQuery)
        || conversation.preview.toLocaleLowerCase().includes(normalizedQuery)
      ))
    : conversations;
  const pinned = filtered.filter((conversation) => conversation.pinned);
  const recent = filtered.filter((conversation) => !conversation.pinned);
  const selected = conversations.find((conversation) => conversation.id === currentConversationId) ?? null;
  const hasResults = pinned.length > 0 || recent.length > 0;

  const renderConversation = (conversation: ConversationListItem) => {
    const active = conversation.id === currentConversationId;
    return (
      <div className="conversation-picker-item-wrap" key={conversation.id}>
        <button
          className={`conversation-picker-item${active ? " is-active" : ""}`}
          type="button"
          aria-current={active ? "page" : undefined}
          onClick={() => onSelect(conversation.id)}
        >
          <span className="conversation-picker-icon" aria-hidden="true">
            {active ? <Check size={14} /> : <MessageCircle size={14} />}
          </span>
          <span className="conversation-picker-copy">
            <strong title={conversation.title}>{conversation.title}</strong>
            <small title={conversation.preview}>{conversation.preview || t("sidebar.noPreview")}</small>
          </span>
          <time>{formatConversationTime(conversation.updatedAt)}</time>
        </button>
        <button
          className="conversation-picker-more"
          type="button"
          title={t("sidebar.conversationActions")}
          aria-expanded={openConversationMenu === conversation.id}
          onClick={(event) => onOpenConversationMenu(conversation.id, event.currentTarget)}
        >
          <MoreHorizontal size={15} />
        </button>
        {openConversationMenu === conversation.id ? (
          <ConversationMenu
            t={t}
            onRename={() => onRename(conversation)}
            onExport={(format) => onExport(conversation.id, format)}
            onSaveAsNote={() => onSaveAsNote(conversation.id)}
            onSummarizeToNote={() => onSummarizeToNote(conversation.id)}
            onGenerateDeepNote={() => onGenerateDeepNote(conversation.id)}
            onUpdateExistingNote={() => onUpdateExistingNote(conversation.id)}
            onDelete={() => onDelete(conversation.id)}
            className="conversation-menu-floating"
            style={pickerMenuPosition ?? undefined}
          />
        ) : null}
      </div>
    );
  };

  return (
    <section
      className="sidebar-popover conversation-picker"
      role="dialog"
      aria-labelledby="sidebar-conversation-picker-title"
    >
      <header className="conversation-picker-header">
        <div>
          <strong id="sidebar-conversation-picker-title">{t("sidebar.conversations")}</strong>
          <span title={selected?.title ?? t("chat.noConversation")}>{selected?.title ?? t("chat.noConversation")}</span>
        </div>
        <button type="button" onClick={onCreate}><MessageSquarePlus size={15} />{t("sidebar.newChatShort")}</button>
      </header>
      <label className="conversation-picker-search">
        <Search size={15} aria-hidden="true" />
        <input
          autoFocus
          type="search"
          value={query}
          placeholder={t("sidebar.searchConversations")}
          aria-label={t("sidebar.searchConversations")}
          onChange={(event) => onQueryChange(event.target.value)}
        />
        {query ? (
          <button type="button" title={t("sidebar.clearSearch")} onClick={() => onQueryChange("")}>
            <X size={14} />
          </button>
        ) : null}
      </label>
      <div className="conversation-picker-list" onScroll={onCloseConversationMenu}>
        {pinned.length > 0 ? (
          <section className="conversation-picker-group">
            <header><Pin size={13} /><span>{t("sidebar.pinned")}</span></header>
            {pinned.map(renderConversation)}
          </section>
        ) : null}
        {recent.length > 0 ? (
          <section className="conversation-picker-group">
            <header><MessageCircle size={13} /><span>{t("sidebar.recent")}</span></header>
            {recent.map(renderConversation)}
          </section>
        ) : null}
        {!hasResults && !loading ? (
          <div className="conversation-picker-empty">
            <MessageCircle size={20} />
            <strong>{query ? t("sidebar.noConversationMatches") : t("chat.emptyNoConversation")}</strong>
            <span>{query ? t("sidebar.adjustConversationSearch") : t("sidebar.firstConversation")}</span>
          </div>
        ) : null}
        {error ? (
          <div className="conversation-picker-status" role="alert">
            <span>{error}</span><button type="button" onClick={onLoadMore}>{t("sidebar.retryLoad")}</button>
          </div>
        ) : loading ? (
          <div className="conversation-picker-status" role="status"><LoaderCircle size={15} />{t("common.loading")}</div>
        ) : hasMore ? (
          <div className="conversation-picker-status"><button type="button" onClick={onLoadMore}>{t("sidebar.loadMore")}</button></div>
        ) : null}
      </div>
      <footer>{t("sidebar.conversationPickerHint")}</footer>
    </section>
  );
}

function formatConversationTime(timestamp: number) {
  const date = new Date(timestamp);
  const now = new Date();
  const sameDay = date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate();
  if (sameDay) return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return date.toLocaleDateString([], { month: "2-digit", day: "2-digit" });
}

function resolvePickerMenuPosition(anchor: DOMRect) {
  const viewportPadding = 8;
  const menuWidth = 190;
  const menuHeight = Math.min(430, window.innerHeight - viewportPadding * 2);
  const maxLeft = Math.max(viewportPadding, window.innerWidth - menuWidth - viewportPadding);
  const maxTop = Math.max(viewportPadding, window.innerHeight - menuHeight - viewportPadding);
  return {
    left: Math.min(maxLeft, Math.max(viewportPadding, anchor.right - menuWidth)),
    top: Math.min(maxTop, Math.max(viewportPadding, anchor.bottom + 4)),
  };
}

type ConversationMenuProps = {
  t: ReturnType<typeof useI18n>["t"];
  className?: string;
  style?: CSSProperties;
  onExport: (format: "markdown" | "json") => void;
  onRename: () => void;
  onSaveAsNote: () => void;
  onSummarizeToNote: () => void;
  onGenerateDeepNote: () => void;
  onUpdateExistingNote: () => void;
  onDelete: () => void;
};

function ConversationMenu({
  t,
  className,
  style,
  onExport,
  onRename,
  onSaveAsNote,
  onSummarizeToNote,
  onGenerateDeepNote,
  onUpdateExistingNote,
  onDelete,
}: ConversationMenuProps) {
  return (
    <div
      className={`sidebar-menu conversation-menu${className ? ` ${className}` : ""}`}
      role="menu"
      style={style}
    >
      <button className="sidebar-menu-item" type="button" role="menuitem" onClick={onRename}>
        <Pencil size={16} />
        <span>{t("sidebar.rename")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <Pin size={16} />
        <span>{t("sidebar.pin")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <FolderInput size={16} />
        <span>{t("sidebar.addProject")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <Layers3 size={16} />
        <span>{t("sidebar.moveCollection")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem" onClick={onSaveAsNote}>
        <NotebookPen size={16} />
        <span>{t("sidebar.saveAsNote")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem" onClick={onSummarizeToNote}>
        <Sparkles size={16} />
        <span>{t("sidebar.summarizeToNote")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem" onClick={onGenerateDeepNote}>
        <BookOpenText size={16} />
        <span>{t("sidebar.deepNote")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem" onClick={onUpdateExistingNote}>
        <FilePenLine size={16} />
        <span>{t("sidebar.updateExistingNote")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem" onClick={() => onExport("markdown")}>
        <Download size={16} />
        <span>{t("sidebar.exportMarkdown")}</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem" onClick={() => onExport("json")}>
        <FileJson size={16} />
        <span>{t("sidebar.exportJson")}</span>
      </button>
      <div className="sidebar-menu-separator" />
      <button
        className="sidebar-menu-item sidebar-menu-danger"
        type="button"
        role="menuitem"
        onClick={onDelete}
      >
        <Trash2 size={16} />
        <span>{t("sidebar.delete")}</span>
      </button>
    </div>
  );
}
