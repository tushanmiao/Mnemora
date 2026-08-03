import type { ComponentType, LazyExoticComponent } from "react";
import { BookOpenText, MessageCircle, NotebookPen, type LucideIcon } from "lucide-react";
import { retryLazy } from "../../bootstrap/retryLazy";
import type { WorkspaceMode } from "./types";
import type { TranslationKey } from "../../i18n/translations";

/**
 * 工作区视图清单（P09.5 视图架构的导航与渲染元数据）。
 *
 * 这是一份普通常量数组，不是动态插件框架：加视图 = 此处加一行 + 在 App 宿主
 * 挂对应视图组件。活动栏按 order 渲染入口；`contextSidebar` 决定该视图是否
 * 渲染共享上下文侧栏（Sidebar）。未来的复习、英语、总览视图在此登记。
 */
export type WorkspaceViewDefinition = {
  id: WorkspaceMode;
  /** 活动栏 tooltip 与无障碍标签的 i18n 键。 */
  labelKey: TranslationKey;
  icon: LucideIcon;
  order: number;
  /** 视图容器按需加载，切换离开后对应 React 树立即卸载。 */
  component: LazyExoticComponent<ComponentType>;
  /** 是否渲染共享上下文侧栏；笔记视图自带左栏所以不渲染。 */
  contextSidebar: boolean;
  /** AI 能力形态：primary=视图本体就是对话；panel=右侧按需 AI 面板。 */
  aiPanel: "primary" | "panel";
};

export const WORKSPACE_VIEWS: readonly WorkspaceViewDefinition[] = [
  {
    id: "chat",
    labelKey: "view.chat",
    icon: MessageCircle,
    order: 1,
    component: retryLazy(() => import("./views/ChatView")),
    contextSidebar: true,
    aiPanel: "primary",
  },
  {
    id: "notes",
    labelKey: "view.notes",
    icon: NotebookPen,
    order: 2,
    component: retryLazy(() => import("./views/NotesView")),
    contextSidebar: false,
    aiPanel: "panel",
  },
  {
    id: "work",
    labelKey: "view.work",
    icon: BookOpenText,
    order: 3,
    component: retryLazy(() => import("./views/WorkView")),
    contextSidebar: true,
    aiPanel: "panel",
  },
];

export const SORTED_WORKSPACE_VIEWS = [...WORKSPACE_VIEWS].sort(
  (left, right) => left.order - right.order,
);

export function findWorkspaceView(id: WorkspaceMode): WorkspaceViewDefinition | undefined {
  return WORKSPACE_VIEWS.find((view) => view.id === id);
}
