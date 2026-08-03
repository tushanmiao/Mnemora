/**
 * 总览功能域只描述聚合后的只读数据，不拥有会话、笔记或文献的持久化状态。
 * Review 与 English 使用各自目录中的类型，避免未来把学习业务塞进总览模型。
 */
export type OverviewDestination = "chat" | "notes" | "work";

export type OverviewRecentItem = {
  id: string;
  kind: "conversation" | "note" | "literature";
  title: string;
  description: string;
  updatedAt: number;
  destination: OverviewDestination;
};

export type OverviewSnapshot = {
  conversationCount: number;
  noteCount: number;
  literatureCount: number;
  recentItems: OverviewRecentItem[];
};
