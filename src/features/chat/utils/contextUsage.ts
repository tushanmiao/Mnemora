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

function estimateImageTokens(width?: number, height?: number) {
  if (!width || !height) return 1_200;
  let scaledWidth = width;
  let scaledHeight = height;
  const longest = Math.max(scaledWidth, scaledHeight);
  if (longest > 2_048) {
    const scale = 2_048 / longest;
    scaledWidth *= scale;
    scaledHeight *= scale;
  }
  const shortest = Math.min(scaledWidth, scaledHeight);
  if (shortest > 768) {
    const scale = 768 / shortest;
    scaledWidth *= scale;
    scaledHeight *= scale;
  }
  const tiles = Math.max(1, Math.ceil(scaledWidth / 512) * Math.ceil(scaledHeight / 512));
  return 85 + tiles * 170;
}

function estimateAttachmentTokens(message: ChatMessage, includeImageBodies: boolean) {
  return (message.attachments ?? []).reduce(
    (total, attachment) => total + (attachment.kind === "image"
      ? includeImageBodies
        ? estimateImageTokens(attachment.width, attachment.height)
        : 40
      : 80),
    0,
  );
}

function effectiveInputTokens(message: ChatMessage) {
  const usage = message.usage;
  return usage?.contextInputTokens ?? usage?.inputTokens ?? null;
}

/**
 * 优先使用最近一次供应商实报输入 Token 作为锚点，再补上该回复和后续消息。
 * 没有实报数据时只执行本地字符估算，不引入 tokenizer 依赖和常驻模型。
 */
export function estimateConversationContext(
  messages: ChatMessage[],
  systemPrompt: string,
): ContextUsageEstimate {
  let lastUserIndex = -1;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index].role === "user") {
      lastUserIndex = index;
      break;
    }
  }
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.role !== "assistant") continue;
    const reportedInput = effectiveInputTokens(message);
    if (reportedInput === null) continue;

    const assistantTokens = message.usage?.outputTokens
      ?? estimateTextTokens(message.content);
    const laterTokens = messages
      .slice(index + 1)
      .reduce(
        (total, item, relativeIndex) => {
          const absoluteIndex = index + 1 + relativeIndex;
          return total
            + estimateTextTokens(item.content)
            + estimateAttachmentTokens(item, absoluteIndex === lastUserIndex);
        },
        0,
      );
    return {
      tokens: reportedInput + assistantTokens + laterTokens,
      source: "providerAnchored",
    };
  }

  return {
    tokens: estimateTextTokens(systemPrompt)
      + messages.reduce(
        (total, message, index) => total
          + estimateTextTokens(message.content)
          + estimateAttachmentTokens(message, index === lastUserIndex),
        0,
      ),
    source: "estimated",
  };
}
