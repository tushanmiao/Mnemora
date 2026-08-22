import { describe, expect, it } from "vitest";
import { containsMermaidFence, isMermaidLanguage, normalizeCodeLanguage } from "./codeBlock";

describe("enhanced code language", () => {
  it("normalizes common aliases without automatic language detection", () => {
    expect(normalizeCodeLanguage("TS")).toBe("typescript");
    expect(normalizeCodeLanguage("rs")).toBe("rust");
    expect(normalizeCodeLanguage(undefined)).toBeNull();
  });

  it("recognizes Mermaid case-insensitively", () => {
    expect(isMermaidLanguage("MerMaid")).toBe(true);
    expect(isMermaidLanguage("markdown")).toBe(false);
  });

  it("detects Mermaid nested inside a Markdown source block", () => {
    expect(containsMermaidFence("### 示例\n\n```mermaid\nflowchart TD\nA-->B\n```"))
      .toBe(true);
    expect(containsMermaidFence("> ``` mermaid title=\"Flow\"\n> A-->B\n> ```"))
      .toBe(true);
    expect(containsMermaidFence("1. Diagram\n\n   ~~~mermaid\n   A-->B\n   ~~~"))
      .toBe(true);
    expect(containsMermaidFence("`mermaid` 只是行内文字"))
      .toBe(false);
  });
});
