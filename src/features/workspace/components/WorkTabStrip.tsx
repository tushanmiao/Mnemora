import { useEffect, useRef, useState } from "react";
import {
  BookOpenText,
  ChevronDown,
  FileText,
  NotebookPen,
  PanelRightOpen,
  X,
} from "lucide-react";
import type { WorkResourceTab } from "../types";
import "../styles/work-tab-strip.css";

type WorkTabStripProps = {
  tabs: WorkResourceTab[];
  activeTabId: string;
  contextPanelOpen: boolean;
  chatBusy: boolean;
  onTabSelect: (tabId: string) => void;
  onTabClose: (tabId: string) => void;
  onToggleContextPanel: () => void;
};

const tabIcons = {
  library: BookOpenText,
  pdf: FileText,
  note: NotebookPen,
} satisfies Record<WorkResourceTab["kind"], typeof BookOpenText>;

export function WorkTabStrip({
  tabs,
  activeTabId,
  contextPanelOpen,
  chatBusy,
  onTabSelect,
  onTabClose,
  onToggleContextPanel,
}: WorkTabStripProps) {
  const [overflowOpen, setOverflowOpen] = useState(false);
  const overflowRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function closeOverflow(event: MouseEvent) {
      if (!overflowRef.current?.contains(event.target as Node)) setOverflowOpen(false);
    }
    document.addEventListener("mousedown", closeOverflow);
    return () => document.removeEventListener("mousedown", closeOverflow);
  }, []);

  return (
    <header className="work-tab-strip">
      <nav className="work-tab-list" role="tablist" aria-label="打开的 Work 资源">
        {tabs.map((tab) => {
          const Icon = tabIcons[tab.kind];
          const active = tab.id === activeTabId;
          return (
            <div className={`work-resource-tab${active ? " work-resource-tab-active" : ""}`} key={tab.id}>
              <button
                className="work-resource-tab-main"
                type="button"
                role="tab"
                title={tab.title}
                aria-selected={active}
                onClick={() => onTabSelect(tab.id)}
              >
                <Icon size={15} />
                <span>{tab.title}</span>
              </button>
              {tab.closable ? (
                <button
                  className="work-resource-tab-close"
                  type="button"
                  title="关闭页签"
                  aria-label={`关闭 ${tab.title}`}
                  onClick={() => onTabClose(tab.id)}
                >
                  <X size={13} />
                </button>
              ) : null}
            </div>
          );
        })}
      </nav>

      <div className="work-tab-overflow-wrap" ref={overflowRef}>
        <button
          className="icon-button work-tab-overflow"
          type="button"
          title="全部页签"
          aria-expanded={overflowOpen}
          onClick={() => setOverflowOpen((open) => !open)}
        >
          <ChevronDown size={16} />
        </button>
        {overflowOpen ? (
          <div className="work-tab-overflow-menu" role="menu">
            {tabs.map((tab) => {
              const Icon = tabIcons[tab.kind];
              return (
                <button
                  className={tab.id === activeTabId ? "work-tab-overflow-active" : ""}
                  type="button"
                  role="menuitem"
                  key={tab.id}
                  title={tab.title}
                  onClick={() => {
                    onTabSelect(tab.id);
                    setOverflowOpen(false);
                  }}
                >
                  <Icon size={15} />
                  <span>{tab.title}</span>
                </button>
              );
            })}
          </div>
        ) : null}
      </div>
      {!contextPanelOpen ? (
        <button
          className="icon-button work-context-toggle"
          type="button"
          title="打开上下文面板"
          aria-label="打开上下文面板"
          aria-pressed="false"
          onClick={onToggleContextPanel}
        >
          <PanelRightOpen size={18} />
          {chatBusy ? <span className="work-context-status-dot" aria-label="AI 正在生成" /> : null}
        </button>
      ) : null}
    </header>
  );
}
