/** 应用顶层只保留 Chat 与 Work 两种工作模式。 */
export type WorkspaceMode = "chat" | "work";

/** Work 左侧文献库的稳定入口。 */
export type WorkLibraryView =
  | "all"
  | "recent"
  | "favorites"
  | "unfiled"
  | "notes"
  | "mind-maps"
  | "trash";

/** 中间工作区未来可以同时打开的资源类型。 */
export type WorkResourceTabKind = "library" | "pdf" | "note" | "mind-map";

export type WorkResourceTab = {
  id: string;
  kind: WorkResourceTabKind;
  title: string;
  closable: boolean;
  resourceId?: string;
};

/** 右侧面板只处理当前活动资源，不保存第二套业务数据。 */
export type WorkContextView = "chat" | "navigator" | "annotations" | "notes" | "info";
