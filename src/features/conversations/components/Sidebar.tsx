import { useEffect, useRef, useState } from "react";
import {
  Bot,
  BookOpenText,
  Boxes,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Download,
  FileJson,
  FileText,
  Folder,
  FolderInput,
  Layers3,
  LoaderCircle,
  MessageSquarePlus,
  MoreHorizontal,
  NotebookPen,
  Pencil,
  Pin,
  Plug,
  Search,
  Settings,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { ConversationListItem } from "../../../types/conversation";
import type { LibraryCollection } from "../../library/types";
import {
  PanelResizeHandle,
  type PanelResizeHandleProps,
} from "../../layout/components/PanelResizeHandle";
import { WorkspaceModeSwitch } from "../../workspace/components/WorkspaceModeSwitch";
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
  settingsOpen: boolean;
  skillsOpen: boolean;
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
  onExportConversation: (conversationId: string, format: "markdown" | "json") => void;
  onClearConversations: () => void;
  onLoadMoreConversations: () => void;
  onOpenSettings: () => void;
  onOpenSkills: () => void;
  onOpenNotes: () => void;
  onModeChange: (mode: WorkspaceMode) => void;
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
  settingsOpen,
  skillsOpen,
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
  onExportConversation,
  onClearConversations,
  onLoadMoreConversations,
  onOpenSettings,
  onOpenSkills,
  onOpenNotes,
  onModeChange,
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
    { id: "assistants", label: t("sidebar.assistants"), icon: Bot },
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
  const menuAreaRef = useRef<HTMLDivElement>(null);
  const loadMoreRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function closeMenus(event: MouseEvent) {
      if (!menuAreaRef.current?.contains(event.target as Node)) {
        setListMenuOpen(false);
        setConversationMenu(null);
      }
    }

    document.addEventListener("mousedown", closeMenus);
    return () => document.removeEventListener("mousedown", closeMenus);
  }, []);

  useEffect(() => {
    if (!collapsed) return;
    setListMenuOpen(false);
    setConversationMenu(null);
  }, [collapsed]);

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

      <WorkspaceModeSwitch mode={mode} collapsed={collapsed} onChange={onModeChange} />

      {mode === "chat" || mode === "notes" ? (
        <>
          <nav className="sidebar-actions" aria-label={t("sidebar.primary")}>
        <button
          className="sidebar-action sidebar-action-primary"
          type="button"
          title={collapsed ? t("sidebar.newChat") : undefined}
          onClick={onCreateConversation}
        >
          <MessageSquarePlus size={18} />
          <span>{t("sidebar.newChat")}</span>
        </button>
        <button className="sidebar-action" type="button" title={collapsed ? t("sidebar.search") : undefined}>
          <Search size={18} />
          <span>{t("sidebar.search")}</span>
        </button>
        <button
          className={`sidebar-action${mode === "notes" ? " sidebar-action-active" : ""}`}
          type="button"
          title={collapsed ? "笔记" : undefined}
          onClick={onOpenNotes}
        >
          <NotebookPen size={18} />
          <span>笔记</span>
        </button>
        <button
          className="sidebar-action"
          type="button"
          title={collapsed ? t("sidebar.extensions") : undefined}
          aria-expanded={extensionsOpen}
          onClick={() => {
            if (collapsed) {
              setExtensionsOpen(true);
              onToggleCollapse();
              return;
            }
            setExtensionsOpen((open) => !open);
          }}
        >
          <Boxes size={18} />
          <span>{t("sidebar.extensions")}</span>
          <ChevronDown className={`sidebar-chevron${extensionsOpen ? " sidebar-chevron-open" : ""}`} size={16} />
        </button>

        {extensionsOpen && !collapsed ? (
          <div className="extension-list">
            {extensionItems.map(({ id, label, icon: Icon }) => (
              <button
                className={`extension-item${id === "skills" && skillsOpen ? " extension-item-active" : ""}`}
                type="button"
                key={id}
                aria-current={id === "skills" && skillsOpen ? "page" : undefined}
                onClick={id === "skills" ? onOpenSkills : undefined}
              >
                <Icon size={15} />
                <span>{label}</span>
              </button>
            ))}
          </div>
        ) : null}
          </nav>

          <div className="sidebar-divider" />

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
                      setListMenuOpen(false);
                    }}
                  >
                    <MoreHorizontal size={16} />
                  </button>

                  {conversationMenu === conversation.id ? (
                    <ConversationMenu t={t}
                      onExport={(format) => {
                        setConversationMenu(null);
                        onExportConversation(conversation.id, format);
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
        <button
          className={`icon-button${settingsOpen ? " sidebar-settings-active" : ""}`}
          type="button"
          title={t("sidebar.settings")}
          aria-current={settingsOpen ? "page" : undefined}
          onClick={onOpenSettings}
        >
          <Settings size={18} />
        </button>
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

type ConversationMenuProps = {
  t: ReturnType<typeof useI18n>["t"];
  onExport: (format: "markdown" | "json") => void;
  onDelete: () => void;
};

function ConversationMenu({ t, onExport, onDelete }: ConversationMenuProps) {
  return (
    <div className="sidebar-menu conversation-menu" role="menu">
      <button className="sidebar-menu-item" type="button" role="menuitem">
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
