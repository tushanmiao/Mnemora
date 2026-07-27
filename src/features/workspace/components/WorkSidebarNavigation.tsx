import { useState } from "react";
import {
  BookOpenText,
  ChevronDown,
  ChevronRight,
  Clock3,
  FilePlus2,
  FolderPlus,
  FolderTree,
  Inbox,
  Network,
  NotebookPen,
  Search,
  Star,
  Trash2,
} from "lucide-react";
import type { WorkLibraryView } from "../types";
import "../styles/work-sidebar-navigation.css";

type WorkSidebarNavigationProps = {
  collapsed: boolean;
  activeView: WorkLibraryView;
  searchQuery: string;
  onViewChange: (view: WorkLibraryView) => void;
  onSearchQueryChange: (query: string) => void;
};

const primaryViews = [
  { id: "all", label: "全部文献", icon: BookOpenText },
  { id: "recent", label: "最近阅读", icon: Clock3 },
  { id: "favorites", label: "收藏", icon: Star },
  { id: "unfiled", label: "未分类", icon: Inbox },
] satisfies Array<{ id: WorkLibraryView; label: string; icon: typeof BookOpenText }>;

const outcomeViews = [
  { id: "notes", label: "笔记", icon: NotebookPen },
  { id: "mind-maps", label: "思维导图", icon: Network },
] satisfies Array<{ id: WorkLibraryView; label: string; icon: typeof BookOpenText }>;

export function WorkSidebarNavigation({
  collapsed,
  activeView,
  searchQuery,
  onViewChange,
  onSearchQueryChange,
}: WorkSidebarNavigationProps) {
  const [collectionsOpen, setCollectionsOpen] = useState(true);
  const [outcomesOpen, setOutcomesOpen] = useState(true);

  return (
    <section
      className={`work-sidebar-navigation${collapsed ? " work-sidebar-navigation-collapsed" : ""}`}
      aria-label="Work 文献库导航"
    >
      <div className="work-library-actions">
        <button type="button" title="导入文献" disabled>
          <FilePlus2 size={17} />
          <span>导入</span>
        </button>
        <button type="button" title="新建分类" disabled>
          <FolderPlus size={17} />
          <span>分类</span>
        </button>
      </div>

      <label className="work-library-search">
        <Search size={16} aria-hidden="true" />
        <input
          type="search"
          value={searchQuery}
          placeholder="查询文献"
          aria-label="查询文献"
          onChange={(event) => onSearchQueryChange(event.target.value)}
        />
      </label>

      <div className="work-library-tree">
        <section className="work-tree-group" aria-label="我的文库">
          <div className="work-tree-heading">
            <BookOpenText size={15} />
            <strong>我的文库</strong>
          </div>
          <nav className="work-tree-items">
            {primaryViews.map(({ id, label, icon: Icon }) => (
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
        </section>

        <section className="work-tree-group" aria-label="分类">
          <button
            className="work-tree-heading work-tree-heading-button"
            type="button"
            aria-expanded={collectionsOpen}
            onClick={() => setCollectionsOpen((open) => !open)}
          >
            {collectionsOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <FolderTree size={15} />
            <strong>分类</strong>
          </button>
          {collectionsOpen && !collapsed ? (
            <div className="work-tree-empty">暂无分类</div>
          ) : null}
        </section>

        <section className="work-tree-group" aria-label="学习成果">
          <button
            className="work-tree-heading work-tree-heading-button"
            type="button"
            aria-expanded={outcomesOpen}
            onClick={() => setOutcomesOpen((open) => !open)}
          >
            {outcomesOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <NotebookPen size={15} />
            <strong>学习成果</strong>
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
          className={`work-tree-item work-tree-trash${activeView === "trash" ? " work-tree-item-active" : ""}`}
          type="button"
          title={collapsed ? "回收站" : undefined}
          aria-current={activeView === "trash" ? "page" : undefined}
          onClick={() => onViewChange("trash")}
        >
          <Trash2 size={16} />
          <span>回收站</span>
        </button>
      </div>
    </section>
  );
}
