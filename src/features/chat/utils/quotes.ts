import type { ChatQuote } from "../../../types/chat";

/** 单条消息最多携带的回答引用数量，避免输入区和请求上下文无限增长。 */
export const MAX_CHAT_QUOTES = 10;

/** 将引用文本规范化，空引用不进入待发送列表。 */
export function normalizeChatQuoteText(text: string) {
  return text.trim();
}

/** 追加一条引用；相同文本去重，达到上限时保留已有引用。 */
export function addChatQuote(
  quotes: ChatQuote[],
  text: string,
  maxQuotes = MAX_CHAT_QUOTES,
): ChatQuote[] {
  const normalized = normalizeChatQuoteText(text);
  if (!normalized || quotes.some((quote) => quote.text === normalized) || quotes.length >= maxQuotes) {
    return quotes;
  }
  return [...quotes, { id: crypto.randomUUID(), text: normalized }];
}

/** 删除指定引用。 */
export function removeChatQuote(quotes: ChatQuote[], quoteId: string): ChatQuote[] {
  return quotes.filter((quote) => quote.id !== quoteId);
}

/** 将多条引用转换为独立的 Markdown 引用块，再拼接用户问题。 */
export function formatChatQuotes(quotes: ChatQuote[], draft: string): string {
  if (quotes.length === 0) return draft;
  const quoteBlocks = quotes.map((quote) => (
    quote.text.split(/\r?\n/).map((line) => `> ${line}`).join("\n")
  ));
  const quotedContent = quoteBlocks.join("\n\n");
  return draft.trim() ? `${quotedContent}\n\n${draft}` : quotedContent;
}
