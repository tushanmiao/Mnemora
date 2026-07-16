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
import "../styles/sidebar.css";

const conversations = [
  "欢迎使用 Mnemora",
  "整理本周阅读计划",
  "Rust 与 Tauri 学习记录",
];

const extensionItems = [
  { label: "助手", icon: Bot },
  { label: "技能", icon: Sparkles },
  { label: "知识库", icon: BookOpenText },
  { label: "插件", icon: Plug },
];

export function Sidebar() {
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
        <button className="sidebar-action sidebar-action-primary" type="button">
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
            <button className="icon-button" type="button" title="新建聊天">
              <MessageSquarePlus size={16} />
            </button>

            {listMenuOpen ? (
              <div className="sidebar-menu list-menu" role="menu">
                <button className="sidebar-menu-item sidebar-menu-danger" type="button" role="menuitem">
                  <Trash2 size={16} />
                  <span>清空全部对话</span>
                </button>
              </div>
            ) : null}
          </div>
        </div>

        <div className="conversation-list">
          {activeSection === "recent" ? (
            conversations.map((title, index) => (
              <div className="conversation-item-wrap" key={title}>
                <button
                  className={`conversation-item${index === 0 ? " conversation-item-active" : ""}`}
                  type="button"
                >
                  <FileText size={16} />
                  <span>{title}</span>
                </button>
                <button
                  className="conversation-more"
                  type="button"
                  title="对话操作"
                  aria-expanded={conversationMenu === title}
                  onClick={() => {
                    setConversationMenu((current) => (current === title ? null : title));
                    setListMenuOpen(false);
                  }}
                >
                  <MoreHorizontal size={16} />
                </button>

                {conversationMenu === title ? <ConversationMenu /> : null}
              </div>
            ))
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
            T
          </div>
          <div className="user-meta">
            <strong>tushanmiao</strong>
            <span>本地工作区</span>
          </div>
        </div>
        <button className="icon-button" type="button" title="设置">
          <Settings size={18} />
        </button>
      </div>
    </aside>
  );
}

function ConversationMenu() {
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
      <button className="sidebar-menu-item sidebar-menu-danger" type="button" role="menuitem">
        <Trash2 size={16} />
        <span>删除</span>
      </button>
    </div>
  );
}
