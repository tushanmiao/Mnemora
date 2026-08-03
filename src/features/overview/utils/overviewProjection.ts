import type { ConversationListItem } from "../../../types/conversation";
import type { LibraryItem, LibraryNoteSummary } from "../../library/types";
import type { OverviewRecentItem, OverviewSnapshot } from "../types";

const MAX_RECENT_ITEMS = 8;

/** 将现有功能域的轻量摘要合并为总览投影，缺失数据不会被伪造成统计值。 */
export function projectOverviewSnapshot(
  conversations: ConversationListItem[],
  notes: LibraryNoteSummary[],
  literature: LibraryItem[],
  conversationCount = conversations.length,
  literatureCount = literature.length,
): OverviewSnapshot {
  const items: OverviewRecentItem[] = [
    ...conversations.map((conversation) => ({
      id: conversation.id,
      kind: "conversation" as const,
      title: conversation.title,
      description: conversation.preview,
      updatedAt: conversation.updatedAt,
      destination: "chat" as const,
    })),
    ...notes.map((note) => ({
      id: note.id,
      kind: "note" as const,
      title: note.title,
      description: note.contentPreview,
      updatedAt: note.updatedAt,
      destination: "notes" as const,
    })),
    ...literature.map((item) => ({
      id: item.id,
      kind: "literature" as const,
      title: item.title,
      description: item.authors.join(", ") || item.publicationTitle,
      updatedAt: item.updatedAt,
      destination: "work" as const,
    })),
  ];

  return {
    conversationCount,
    noteCount: notes.length,
    literatureCount,
    recentItems: items
      .sort((left, right) => right.updatedAt - left.updatedAt || left.id.localeCompare(right.id))
      .slice(0, MAX_RECENT_ITEMS),
  };
}
