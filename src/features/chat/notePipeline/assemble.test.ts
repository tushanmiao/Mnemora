import { describe, expect, it } from "vitest";
import { assembleDeepNote } from "./assemble";

describe("assembleDeepNote", () => {
  it("collects warnings without blocking assembly", () => {
    const section = { id: "sec-1", heading: "概念", kind: "concept" as const, brief: "B", needsSupplement: true, sourceMessageIds: [] };
    const result = assembleDeepNote(
      { title: "标题", summary: "概览", weakPoints: [], sections: [section] },
      [{ section, markdown: "## 概念\n\n```mermaid\nflowchart LR\nA-->B\n" }],
    );
    expect(result.content).toContain("# 标题");
    expect(result.content).toContain("来源：源自本次对话；AI 补充背景");
    expect(result.warnings.some((warning) => warning.includes("AI 补充"))).toBe(false);
    expect(result.warnings.some((warning) => warning.includes("Mermaid"))).toBe(true);
    expect(result.warnings.some((warning) => warning.includes("自检"))).toBe(true);
  });

  it("adds a draft suffix when cancelled", () => {
    const section = { id: "sec-1", heading: "自检问题", kind: "selfcheck" as const, brief: "B", needsSupplement: false, sourceMessageIds: [] };
    const result = assembleDeepNote(
      { title: "标题", summary: "", weakPoints: [], sections: [section] },
      [{ section, markdown: "## 自检问题\n\n1. A?\n2. B?\n3. C?" }],
      true,
    );
    expect(result.title).toBe("标题（草稿）");
  });
});
