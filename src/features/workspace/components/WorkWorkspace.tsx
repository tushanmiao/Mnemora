import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ArrowUpDown,
  BookOpenText,
  Clock3,
  Columns3,
  Inbox,
  ListFilter,
  Network,
  NotebookPen,
  SearchX,
  Star,
  Trash2,
} from "lucide-react";
import type { WorkLibraryView, WorkResourceTab } from "../types";
import { WorkTabStrip } from "./WorkTabStrip";
import "../styles/work-workspace.css";

type WorkWorkspaceProps = {
  libraryView: WorkLibraryView;
  searchQuery: string;
  contextPanelOpen: boolean;
  chatBusy: boolean;
  onToggleContextPanel: () => void;
};

const initialTabs: WorkResourceTab[] = [
  { id: "library", kind: "library", title: "我的文库", closable: false },
];

const viewDetails = {
  all: { title: "全部文献", empty: "文库中暂无文献", icon: BookOpenText },
  recent: { title: "最近阅读", empty: "暂无最近阅读记录", icon: Clock3 },
  favorites: { title: "收藏", empty: "暂无收藏文献", icon: Star },
  unfiled: { title: "未分类", empty: "暂无未分类文献", icon: Inbox },
  notes: { title: "笔记", empty: "暂无学习笔记", icon: NotebookPen },
  "mind-maps": { title: "思维导图", empty: "暂无思维导图", icon: Network },
  trash: { title: "回收站", empty: "回收站为空", icon: Trash2 },
} satisfies Record<WorkLibraryView, { title: string; empty: string; icon: typeof BookOpenText }>;

export function WorkWorkspace({
  libraryView,
  searchQuery,
  contextPanelOpen,
  chatBusy,
  onToggleContextPanel,
}: WorkWorkspaceProps) {
  const [tabs, setTabs] = useState<WorkResourceTab[]>(initialTabs);
  const [activeTabId, setActiveTabId] = useState("library");

  useEffect(() => {
    setActiveTabId("library");
  }, [libraryView]);

  const closeTab = useCallback((tabId: string) => {
    setTabs((currentTabs) => {
      const target = currentTabs.find((tab) => tab.id === tabId);
      if (!target?.closable) return currentTabs;
      const nextTabs = currentTabs.filter((tab) => tab.id !== tabId);
      if (activeTabId === tabId) setActiveTabId("library");
      return nextTabs;
    });
  }, [activeTabId]);

  const activeTab = useMemo(
    () => tabs.find((tab) => tab.id === activeTabId) ?? tabs[0],
    [activeTabId, tabs],
  );
  const view = viewDetails[libraryView];
  const EmptyIcon = searchQuery.trim() ? SearchX : view.icon;
  const emptyTitle = searchQuery.trim()
    ? `没有找到“${searchQuery.trim()}”`
    : view.empty;
  const isLearningOutcome = libraryView === "notes" || libraryView === "mind-maps";

  return (
    <section className="work-workspace" aria-label="Work 文献学习工作区">
      <WorkTabStrip
        tabs={tabs}
        activeTabId={activeTab.id}
        contextPanelOpen={contextPanelOpen}
        chatBusy={chatBusy}
        onTabSelect={setActiveTabId}
        onTabClose={closeTab}
        onToggleContextPanel={onToggleContextPanel}
      />

      <header className="work-library-header">
        <div className="work-library-heading">
          <h1>{view.title}</h1>
          <span>0 项</span>
        </div>
        <div className="work-library-header-actions">
          <button className="icon-button" type="button" title="筛选" disabled>
            <ListFilter size={17} />
          </button>
          <button className="icon-button" type="button" title="排序" disabled>
            <ArrowUpDown size={17} />
          </button>
          <button className="icon-button" type="button" title="列表列设置" disabled>
            <Columns3 size={17} />
          </button>
        </div>
      </header>

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
            </>
          )}
        </div>

        <div className="work-library-empty" role="status">
          <EmptyIcon size={34} aria-hidden="true" />
          <h2>{emptyTitle}</h2>
          <p>{searchQuery.trim() ? "请调整查询条件" : "0 项"}</p>
        </div>
      </div>
    </section>
  );
}

export default WorkWorkspace;
