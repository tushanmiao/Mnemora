import { listStoredConversations } from "../../conversations/api/conversations";
import { listLibraryItems, listLibraryNotes } from "../../library/api/library";
import type { OverviewSnapshot } from "../types";
import { projectOverviewSnapshot } from "../utils/overviewProjection";

/** 读取总览所需的有限摘要；不会加载完整对话、PDF 内容或 AI 资源。 */
export async function loadOverviewSnapshot(): Promise<OverviewSnapshot> {
  const [conversationPage, notes, literaturePage] = await Promise.all([
    listStoredConversations(0, 50),
    listLibraryNotes(),
    listLibraryItems({
      view: "all",
      searchQuery: "",
      collectionId: null,
      sort: "updated",
      offset: 0,
      limit: 50,
    }),
  ]);
  return projectOverviewSnapshot(
    conversationPage.items,
    notes,
    literaturePage.items,
    conversationPage.total,
    literaturePage.total,
  );
}
