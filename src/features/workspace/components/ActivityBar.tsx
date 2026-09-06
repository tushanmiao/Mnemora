import { useRef, type KeyboardEvent } from "react";
import { Settings } from "lucide-react";
import { ACTIVITY_BAR_WORKSPACE_VIEWS } from "../viewRegistry";
import type { WorkspaceMode } from "../types";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/activity-bar.css";

type ActivityBarProps = {
  /** 当前活跃视图；设置页打开时视图入口不显示激活态。 */
  activeView: WorkspaceMode;
  settingsOpen: boolean;
  onSelectView: (view: WorkspaceMode) => void;
  onOpenSettings: () => void;
  /**
   * 需要用户回到该视图处理的事情（当前只有待处理的工具审批与反问）。
   *
   * 审批有 300 秒后端超时，而弹窗只在对应消息可见时渲染；不给这个提示，用户切走之后
   * 那次中断会静默超时。
   */
  attentionViews?: ReadonlySet<WorkspaceMode>;
};

/**
 * 极窄活动栏（P09.5）：所有工作区视图的常驻导航，替代旧的 Chat/Work 分段切换器。
 * 只负责导航——不持有任何视图运行时；视图仍然单挂载、切换即卸载。
 */
export function ActivityBar({
  activeView,
  settingsOpen,
  onSelectView,
  onOpenSettings,
  attentionViews,
}: ActivityBarProps) {
  const { t } = useI18n();
  const viewButtonsRef = useRef<Array<HTMLButtonElement | null>>([]);

  const moveFocus = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const lastIndex = ACTIVITY_BAR_WORKSPACE_VIEWS.length - 1;
    const nextIndex = event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? lastIndex
        : event.key === 'ArrowDown'
          ? (index + 1) % ACTIVITY_BAR_WORKSPACE_VIEWS.length
          : (index - 1 + ACTIVITY_BAR_WORKSPACE_VIEWS.length) % ACTIVITY_BAR_WORKSPACE_VIEWS.length;
    viewButtonsRef.current[nextIndex]?.focus();
  };

  return (
    <nav className="activity-bar" aria-label={t("view.activityBar")}>
      <div className="activity-bar-views">
        {ACTIVITY_BAR_WORKSPACE_VIEWS.map((view, index) => {
          const active = !settingsOpen && activeView === view.id;
          const needsAttention = attentionViews?.has(view.id) === true;
          const Icon = view.icon;
          const label = needsAttention
            ? `${t(view.labelKey)}（有待处理的确认）`
            : t(view.labelKey);
          return (
            <button
              key={view.id}
              className={`activity-bar-item${active ? " is-active" : ""}${needsAttention ? " needs-attention" : ""}`}
              data-workspace={view.id}
              type="button"
              ref={(element) => { viewButtonsRef.current[index] = element; }}
              title={label}
              aria-label={label}
              aria-current={active ? "page" : undefined}
              onClick={() => onSelectView(view.id)}
              onKeyDown={(event) => moveFocus(event, index)}
            >
              <Icon size={20} />
              {needsAttention ? <span className="activity-bar-badge" aria-hidden="true" /> : null}
            </button>
          );
        })}
      </div>
      <div className="activity-bar-footer">
        <button
          className={`activity-bar-item${settingsOpen ? " is-active" : ""}`}
          data-workspace="settings"
          type="button"
          title={t("sidebar.settings")}
          aria-label={t("sidebar.settings")}
          aria-current={settingsOpen ? "page" : undefined}
          onClick={onOpenSettings}
        >
          <Settings size={20} />
        </button>
      </div>
    </nav>
  );
}
