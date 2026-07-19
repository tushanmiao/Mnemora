import type { ChatMessage } from "../../../types/chat";

export type ContextUsageEstimate = {
  tokens: number;
  source: "providerAnchored" | "estimated";
};

/** 轻量估算：ASCII 约 4 字符一个 Token，非 ASCII 字符按一个 Token 计算。 */
export function estimateTextTokens(text: string) {
  let ascii = 0;
  let nonAscii = 0;
  for (const character of text) {
    if (character.charCodeAt(0) < 128) ascii += 1;
    else nonAscii += 1;
  }
  return Math.ceil(ascii / 4 + nonAscii);
}

function effectiveInputTokens(message: ChatMessage) {
  const usage = message.usage;
  if (!usage?.inputTokens) return null;
  return usage.inputTokens
    + (usage.cacheReadTokens ?? 0)
    + (usage.cacheWriteTokens ?? 0);
}

/**
 * 优先使用最近一次供应商实报输入 Token 作为锚点，再补上该回复和后续消息。
 * 没有实报数据时只执行本地字符估算，不引入 tokenizer 依赖和常驻模型。
 */
export function estimateConversationContext(
  messages: ChatMessage[],
  systemPrompt: string,
): ContextUsageEstimate {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.role !== "assistant") continue;
    const reportedInput = effectiveInputTokens(message);
    if (reportedInput === null) continue;

    const assistantTokens = message.usage?.outputTokens
      ?? estimateTextTokens(message.content);
    const laterTokens = messages
      .slice(index + 1)
      .reduce((total, item) => total + estimateTextTokens(item.content), 0);
    return {
      tokens: reportedInput + assistantTokens + laterTokens,
      source: "providerAnchored",
    };
  }

  return {
    tokens: estimateTextTokens(systemPrompt)
      + messages.reduce((total, message) => total + estimateTextTokens(message.content), 0),
    source: "estimated",
  };
}
