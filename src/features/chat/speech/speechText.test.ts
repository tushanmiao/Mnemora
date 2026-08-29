import { describe, expect, it } from "vitest";
import {
  detectSpeechLanguage,
  extractSpeakableText,
  normalizeSelectedSpeechText,
  splitSpeechText,
} from "./speechText";

describe("speech text projection", () => {
  it("keeps readable Markdown text and skips code, Mermaid, and formula source", () => {
    const result = extractSpeakableText([
      "# 结论",
      "",
      "这是 **重要** 的 [说明](https://example.com)。",
      "",
      "```mermaid",
      "flowchart TD",
      "A-->B",
      "```",
      "",
      "$$E=mc^2$$",
      "",
      "- 第一项",
      "- 第二项",
    ].join("\n"));

    expect(result).toContain("结论");
    expect(result).toContain("这是 重要 的 说明");
    expect(result).toContain("第一项");
    expect(result).not.toContain("flowchart");
    expect(result).not.toContain("E=mc^2");
  });

  it("skips multiline display formulas without dropping surrounding prose", () => {
    const result = extractSpeakableText([
      "公式前的说明。",
      "$$",
      "\\frac{a}{b} = c",
      "$$",
      "公式后的说明。",
    ].join("\n"));

    expect(result).toContain("公式前的说明");
    expect(result).toContain("公式后的说明");
    expect(result).not.toContain("frac");
  });

  it("bounds and normalizes selected text", () => {
    expect(normalizeSelectedSpeechText("  hello\n world  ")).toBe("hello world");
    expect(normalizeSelectedSpeechText("abcdef", 3)).toBe("abc");
  });

  it("splits long text at natural punctuation", () => {
    const chunks = splitSpeechText("第一句。第二句。第三句。", 5);
    expect(chunks).toEqual(["第一句。", "第二句。", "第三句。"]);
  });

  it("detects Chinese and English speech languages", () => {
    expect(detectSpeechLanguage("你好")).toBe("zh-CN");
    expect(detectSpeechLanguage("hello")).toBe("en-US");
  });
});
