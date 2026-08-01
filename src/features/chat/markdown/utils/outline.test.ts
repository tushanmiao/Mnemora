import { describe, expect, it } from "vitest";
import { extractMarkdownOutline, headingId } from "./outline";

describe("Markdown outline", () => {
  it("extracts real headings and ignores headings inside fenced code", () => {
    const content = "# 第一章\n\n```md\n## 不是标题\n```\n\n## 第二章\n\n### 小节";
    const outline = extractMarkdownOutline(content, "message-1");

    expect(outline.map((item) => item.title)).toEqual(["第一章", "第二章", "小节"]);
    expect(outline[0].id).toBe(headingId("message-1", 0));
  });
});

