import { describe, expect, it } from "vitest";
import { normalizeWorkSession } from "./useWorkSession";

describe("normalizeWorkSession", () => {
  it("始终保留不可关闭的文库页签", () => {
    expect(normalizeWorkSession(null)).toEqual({
      version: 1,
      tabs: [{ id: "library", kind: "library", title: "我的文库", closable: false }],
      activeTabId: "library",
    });
  });

  it("丢弃损坏页签并恢复有效的活动 PDF", () => {
    const session = normalizeWorkSession({
      tabs: [
        { id: "broken", kind: "pdf", title: "", closable: true },
        {
          id: "pdf:item-1",
          kind: "pdf",
          title: "Paper",
          closable: true,
          resourceId: "item-1",
        },
      ],
      activeTabId: "pdf:item-1",
    });
    expect(session.tabs).toHaveLength(2);
    expect(session.activeTabId).toBe("pdf:item-1");
  });

  it("恢复有效的笔记页签", () => {
    const session = normalizeWorkSession({
      tabs: [
        {
          id: "note:note-1",
          kind: "note",
          title: "研究笔记",
          closable: true,
          resourceId: "note-1",
        },
      ],
      activeTabId: "note:note-1",
    });
    expect(session.tabs[1]).toMatchObject({ kind: "note", resourceId: "note-1" });
    expect(session.activeTabId).toBe("note:note-1");
  });

  it("恢复 PDF 笔记的轻量来源上下文", () => {
    const session = normalizeWorkSession({
      tabs: [{
        id: "note:note-2",
        kind: "note",
        title: "带来源的笔记",
        closable: true,
        resourceId: "note-2",
        noteSource: {
          sourcePdfId: "item-2",
          sourcePdfTitle: "Source Paper",
          sourcePageIndex: 6,
        },
      }],
      activeTabId: "note:note-2",
    });
    expect(session.tabs[1]).toMatchObject({
      noteSource: {
        sourcePdfId: "item-2",
        sourcePdfTitle: "Source Paper",
        sourcePageIndex: 6,
      },
    });
  });

  it("丢弃来源页码损坏的笔记页签", () => {
    const session = normalizeWorkSession({
      tabs: [{
        id: "note:note-broken",
        kind: "note",
        title: "损坏来源",
        closable: true,
        resourceId: "note-broken",
        noteSource: {
          sourcePdfId: "item-2",
          sourcePdfTitle: "Source Paper",
          sourcePageIndex: -1,
        },
      }],
      activeTabId: "note:note-broken",
    });
    expect(session.tabs).toHaveLength(1);
    expect(session.activeTabId).toBe("library");
  });
});
