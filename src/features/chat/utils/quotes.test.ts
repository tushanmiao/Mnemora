import { describe, expect, it } from "vitest";
import type { ChatQuote } from "../../../types/chat";
import {
  addChatQuote,
  formatChatQuotes,
  MAX_CHAT_QUOTES,
  removeChatQuote,
} from "./quotes";

function quote(index: number): ChatQuote {
  return { id: `quote-${index}`, text: `内容 ${index}` };
}

describe("chat quotes", () => {
  it("ignores empty and duplicate selections", () => {
    const first = addChatQuote([], "  第一条  ");
    expect(first).toHaveLength(1);
    expect(first[0].text).toBe("第一条");
    expect(addChatQuote(first, "第一条")).toBe(first);
    expect(addChatQuote(first, "   ")).toBe(first);
  });

  it("keeps the first ten quotes and does not replace existing items", () => {
    const quotes = Array.from({ length: MAX_CHAT_QUOTES }, (_, index) => quote(index));
    expect(addChatQuote(quotes, "第十一条")).toBe(quotes);
    expect(quotes).toHaveLength(MAX_CHAT_QUOTES);
  });

  it("removes one quote without changing the others", () => {
    expect(removeChatQuote([quote(1), quote(2)], "quote-1")).toEqual([quote(2)]);
  });

  it("formats each quote as an independent Markdown block", () => {
    expect(formatChatQuotes([
      { id: "a", text: "第一行\n第二行" },
      { id: "b", text: "另一条" },
    ], "请比较")).toBe("> 第一行\n> 第二行\n\n> 另一条\n\n请比较");
  });
});
