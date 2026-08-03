import { describe, expect, it } from "vitest";
import { projectOverviewSnapshot } from "./overviewProjection";

describe("projectOverviewSnapshot", () => {
  it("merges recent activity and sorts it by update time", () => {
    const snapshot = projectOverviewSnapshot(
      [{ id: "c1", title: "对话", preview: "问题", messageCount: 1, assistantId: null, providerId: null, modelId: null, projectId: null, collectionId: null, pinned: false, createdAt: 1, updatedAt: 3 }],
      [{ id: "n1", itemId: null, itemTitle: null, title: "笔记", contentPreview: "内容", contentChars: 2, groupName: null, createdAt: 1, updatedAt: 5 }],
      [],
      4,
      2,
    );
    expect(snapshot.recentItems.map((item) => item.id)).toEqual(["n1", "c1"]);
    expect(snapshot.conversationCount).toBe(4);
    expect(snapshot.literatureCount).toBe(2);
  });

  it("limits the activity list without inventing review metrics", () => {
    const conversations = Array.from({ length: 12 }, (_, index) => ({
      id: `c${index}`,
      title: `对话 ${index}`,
      preview: "",
      messageCount: 0,
      assistantId: null,
      providerId: null,
      modelId: null,
      projectId: null,
      collectionId: null,
      pinned: false,
      createdAt: index,
      updatedAt: index,
    }));
    const snapshot = projectOverviewSnapshot(conversations, [], []);
    expect(snapshot.recentItems).toHaveLength(8);
    expect(snapshot).not.toHaveProperty("reviewCount");
  });
});
