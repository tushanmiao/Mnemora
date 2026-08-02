import { describe, expect, it } from "vitest";
import { composeChatSystemPrompt, DEFAULT_CHAT_SYSTEM_PROMPT } from "./systemPrompt";

describe("composeChatSystemPrompt", () => {
  it("uses the editable global prompt without adding a hidden duplicate", () => {
    const prompt = composeChatSystemPrompt({
      globalPrompt: "用户自定义规则",
      conversationPrompt: "本次会话规则",
      responseLanguage: "zh",
    });

    expect(prompt.startsWith("用户自定义规则")).toBe(true);
    expect(prompt).not.toContain(DEFAULT_CHAT_SYSTEM_PROMPT);
    expect(prompt).toContain("本次会话规则");
    expect(prompt).toContain("请使用简体中文回答。");
  });

  it("does not add an empty context section", () => {
    expect(composeChatSystemPrompt({})).toBe("");
  });
});
