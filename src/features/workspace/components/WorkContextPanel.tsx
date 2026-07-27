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
  MessageCircle,
  MoreHorizontal,
  NotebookPen,
  PanelRightClose,
} from "lucide-react";
import {
  PanelResizeHandle,
  type PanelResizeHandleProps,
} from "../../layout/components/PanelResizeHandle";
import type { WorkContextView } from "../types";
import "../styles/work-context-panel.css";

type WorkContextPanelProps = {
  activeView: WorkContextView;
  resourceLabel: string;
  searchQuery: string;
  chatBusy: boolean;
  chatPanel: ReactNode;
  onViewChange: (view: WorkContextView) => void;
  onClose: () => void;
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
  searchQuery,
  chatBusy,
  chatPanel,
  onViewChange,
  onClose,
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
          <InfoPanel resourceLabel={resourceLabel} searchQuery={searchQuery} />
        ) : (
          <ContextEmpty view={activeView} resourceLabel={resourceLabel} />
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

type ContextEmptyProps = {
  view: Exclude<WorkContextView, "chat" | "info">;
  resourceLabel: string;
};

function ContextEmpty({ view, resourceLabel }: ContextEmptyProps) {
  const content = {
    navigator: {
      title: "暂无文档导航",
      detail: "当前活动资源没有大纲或缩略图",
      icon: ListTree,
    },
    annotations: {
      title: "暂无批注",
      detail: "打开 PDF 后显示高亮和评论",
      icon: Highlighter,
    },
    notes: {
      title: "暂无关联笔记",
      detail: "当前资源尚未创建笔记",
      icon: NotebookPen,
    },
  } satisfies Record<ContextEmptyProps["view"], {
    title: string;
    detail: string;
    icon: typeof ListTree;
  }>;
  const current = content[view];
  const Icon = current.icon;

  return (
    <section className="work-context-empty" aria-label={`${resourceLabel} ${current.title}`}>
      <Icon size={28} aria-hidden="true" />
      <h2>{current.title}</h2>
      <p>{current.detail}</p>
    </section>
  );
}

type InfoPanelProps = {
  resourceLabel: string;
  searchQuery: string;
};

function InfoPanel({ resourceLabel, searchQuery }: InfoPanelProps) {
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
          <dd>0</dd>
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
