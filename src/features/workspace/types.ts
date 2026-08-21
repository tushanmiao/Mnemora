/** Notes 从 Chat 侧栏进入独立工作区；Work 仍保留 PDF 学习流程。 */
/** 已实现的工作区视图。Review 与 English 先保持功能域边界，待页面落地后再注册。 */
export type WorkspaceMode = "overview" | "chat" | "notes" | "deepNote" | "work" | "english";

/** Work 左侧文献库的稳定入口。 */
export type WorkLibraryView =
  | "all"
  | "recent"
  | "favorites"
  | "unfiled"
  | "notes"
  | "trash";

/** 中间工作区未来可以同时打开的资源类型。 */
export type WorkResourceTabKind = "library" | "pdf" | "note";

/** PDF 阅读器卸载后仍可保留的轻量来源，不持有 PDF.js、Canvas 或页文本。 */
export type WorkNoteSourceContext = {
  sourcePdfId: string;
  sourcePdfTitle: string;
  /** 0-based，与 PDF.js、文献引用保持一致。 */
  sourcePageIndex: number | null;
};

export type WorkResourceTab = {
  id: string;
  kind: WorkResourceTabKind;
  title: string;
  closable: boolean;
  resourceId?: string;
  noteSource?: WorkNoteSourceContext;
};

/** 当前 Work 中真正活动的笔记；供右侧 Chat 和安全编辑共享。 */
export type ActiveWorkNoteContext = {
  noteId: string;
  noteTitle: string;
  revisionHash: string;
  /** 非 Tool 模型使用的有界快照；Tool 模型会丢弃它并按需调用 note_read。 */
  noteSnapshot: string;
  source: WorkNoteSourceContext | null;
};

/** Work Chat 范围选择器使用的轻量 PDF 页签信息，不持有 PDF.js 资源。 */
export type WorkPdfDocument = {
  libraryItemId: string;
  title: string;
  active: boolean;
};

/** 点击聊天中的文献引用后交给 WorkWorkspace 的一次性跳转请求。 */
export type LiteratureNavigationRequest = {
  requestId: string;
  libraryItemId: string;
  title: string;
  pageIndex: number;
};

/** 右侧面板只处理当前活动资源，不保存第二套业务数据。 */
export type WorkContextView = "chat" | "navigator" | "annotations" | "notes" | "info";
