import { useEffect, useRef, useState } from "react";
import {
  Bot,
  BookOpenText,
  Boxes,
  ChevronDown,
  ChevronLeft,
  Download,
  FileText,
  Folder,
  FolderInput,
  Layers3,
  MessageSquarePlus,
  MoreHorizontal,
  Pencil,
  Pin,
  Plug,
  Search,
  Settings,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { ConversationListItem } from "../../../types/conversation";
import "../styles/sidebar.css";

const extensionItems = [
  { label: "助手", icon: Bot },
  { label: "技能", icon: Sparkles },
  { label: "知识库", icon: BookOpenText },
  { label: "插件", icon: Plug },
];

type SidebarProps = {
  settingsOpen: boolean;
  userDisplayName: string;
  userAvatar: string;
  conversations: ConversationListItem[];
  currentConversationId: string | null;
  onCreateConversation: () => void;
  onSelectConversation: (conversationId: string) => void;
  onDeleteConversation: (conversationId: string) => void;
  onClearConversations: () => void;
  onOpenSettings: () => void;
};

export function Sidebar({
  settingsOpen,
  userDisplayName,
  userAvatar,
  conversations,
  currentConversationId,
  onCreateConversation,
  onSelectConversation,
  onDeleteConversation,
  onClearConversations,
  onOpenSettings,
}: SidebarProps) {
  const normalizedDisplayName = userDisplayName.trim() || "Mnemora 用户";
  const avatarInitial = (Array.from(normalizedDisplayName)[0] ?? "M").toUpperCase();
  const [extensionsOpen, setExtensionsOpen] = useState(true);
  const [activeSection, setActiveSection] = useState<"recent" | "collections" | "projects">("recent");
  const [listMenuOpen, setListMenuOpen] = useState(false);
  const [conversationMenu, setConversationMenu] = useState<string | null>(null);
  const menuAreaRef = useRef<HTMLDivElement>(null);

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

  return (
    <aside className="sidebar" aria-label="应用导航" ref={menuAreaRef}>
      <div className="sidebar-brand">
        <div className="brand-mark" aria-hidden="true">
          M
        </div>
        <span>Mnemora</span>
        <button className="icon-button sidebar-collapse" type="button" title="收起侧边栏">
          <ChevronLeft size={18} />
        </button>
      </div>

      <nav className="sidebar-actions" aria-label="主要功能">
        <button
          className="sidebar-action sidebar-action-primary"
          type="button"
          onClick={onCreateConversation}
        >
          <MessageSquarePlus size={18} />
          <span>新建聊天</span>
        </button>
        <button className="sidebar-action" type="button">
          <Search size={18} />
          <span>搜索</span>
        </button>
        <button
          className="sidebar-action"
          type="button"
          aria-expanded={extensionsOpen}
          onClick={() => setExtensionsOpen((open) => !open)}
        >
          <Boxes size={18} />
          <span>扩展</span>
          <ChevronDown className={`sidebar-chevron${extensionsOpen ? " sidebar-chevron-open" : ""}`} size={16} />
        </button>

        {extensionsOpen ? (
          <div className="extension-list">
            {extensionItems.map(({ label, icon: Icon }) => (
              <button className="extension-item" type="button" key={label}>
                <Icon size={15} />
                <span>{label}</span>
              </button>
            ))}
          </div>
        ) : null}
      </nav>

      <div className="sidebar-divider" />

      <section className="conversation-section" aria-label="对话分类">
        <div className="conversation-tabs">
          <button
            className={activeSection === "recent" ? "conversation-tab conversation-tab-active" : "conversation-tab"}
            type="button"
            onClick={() => setActiveSection("recent")}
          >
            最近
          </button>
          <button
            className={activeSection === "collections" ? "conversation-tab conversation-tab-active" : "conversation-tab"}
            type="button"
            onClick={() => setActiveSection("collections")}
          >
            集合
          </button>
          <button
            className={activeSection === "projects" ? "conversation-tab conversation-tab-active" : "conversation-tab"}
            type="button"
            onClick={() => setActiveSection("projects")}
          >
            项目
          </button>

          <div className="conversation-list-actions">
            <button
              className="icon-button"
              type="button"
              title="列表操作"
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
              title="新建聊天"
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
                  <span>清空全部对话</span>
                </button>
              </div>
            ) : null}
          </div>
        </div>

        <div className="conversation-list">
          {activeSection === "recent" ? (
            conversations.length > 0 ? conversations.map((conversation) => (
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
                  title="对话操作"
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
                  <ConversationMenu
                    onDelete={() => {
                      setConversationMenu(null);
                      onDeleteConversation(conversation.id);
                    }}
                  />
                ) : null}
              </div>
            )) : (
              <button className="empty-section-action" type="button" onClick={onCreateConversation}>
                <MessageSquarePlus size={17} />
                <span>新建第一个对话</span>
              </button>
            )
          ) : activeSection === "projects" ? (
            <button className="empty-section-action" type="button">
              <Folder size={17} />
              <span>创建第一个项目</span>
            </button>
          ) : (
            <button className="empty-section-action" type="button">
              <Layers3 size={17} />
              <span>创建第一个集合</span>
            </button>
          )}
        </div>
      </section>

      <div className="sidebar-footer">
        <div className="user-profile">
          <div className="user-avatar" aria-hidden="true">
            {userAvatar ? <img src={userAvatar} alt="" /> : avatarInitial}
          </div>
          <div className="user-meta">
            <strong title={normalizedDisplayName}>{normalizedDisplayName}</strong>
            <span>本地工作区</span>
          </div>
        </div>
        <button
          className={`icon-button${settingsOpen ? " sidebar-settings-active" : ""}`}
          type="button"
          title="设置"
          aria-current={settingsOpen ? "page" : undefined}
          onClick={onOpenSettings}
        >
          <Settings size={18} />
        </button>
      </div>
    </aside>
  );
}

type ConversationMenuProps = {
  onDelete: () => void;
};

function ConversationMenu({ onDelete }: ConversationMenuProps) {
  return (
    <div className="sidebar-menu conversation-menu" role="menu">
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <Pencil size={16} />
        <span>重命名</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <Pin size={16} />
        <span>置顶</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <FolderInput size={16} />
        <span>添加到项目</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <Layers3 size={16} />
        <span>移动到集合</span>
      </button>
      <button className="sidebar-menu-item" type="button" role="menuitem">
        <Download size={16} />
        <span>导出</span>
      </button>
      <div className="sidebar-menu-separator" />
      <button
        className="sidebar-menu-item sidebar-menu-danger"
        type="button"
        role="menuitem"
        onClick={onDelete}
      >
        <Trash2 size={16} />
        <span>删除</span>
      </button>
    </div>
  );
}
