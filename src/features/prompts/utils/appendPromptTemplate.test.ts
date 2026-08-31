import { describe, expect, it } from "vitest";
import { appendPromptTemplate } from "./appendPromptTemplate";

describe("appendPromptTemplate", () => {
  it("inserts a prompt into an empty composer without sending it", () => {
    expect(appendPromptTemplate("", "  请给出结论  ")).toBe("请给出结论");
  });

  it("appends after the current message with one empty line", () => {
    expect(appendPromptTemplate("分析这篇论文  \n", "  请列出证据。\n"))
      .toBe("分析这篇论文\n\n请列出证据。");
  });
});
