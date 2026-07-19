import type { ChatMessage, MessageRole } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";

export const AUTO_COMPRESSION_RATIO = 0.9;
const RECENT_MESSAGES_TO_KEEP = 4;

export function activeContextMessages(conversation: Conversation) {
  const boundaryId = conversation.compressedUntilMessageId;
  if (!boundaryId) return conversation.messages;
  const boundaryIndex = conversation.messages.findIndex((message) => message.id === boundaryId);
  return boundaryIndex < 0 ? conversation.messages : conversation.messages.slice(boundaryIndex + 1);
}

export function contextSummaryPrompt(conversation: Conversation) {
  const summary = conversation.contextSummary.trim();
  if (!summary) return "";
  return [
    "以下是此前对话的压缩摘要。请将其视为早期对话上下文，并在回答时保持其中的事实、约束和未完成事项：",
    summary,
  ].join("\n\n");
}

export function compressionCandidates(conversation: Conversation) {
  const active = activeContextMessages(conversation)
    .filter((message) => message.status === "completed" && message.content.trim());
  if (active.length <= RECENT_MESSAGES_TO_KEEP + 1) return [];
  return active.slice(0, -RECENT_MESSAGES_TO_KEEP);
}

export function compressionTranscript(
  existingSummary: string,
  messages: ChatMessage[],
) {
  const sections = messages.map((message) => {
    const role = message.role === "user" ? "用户" : "助手";
    return `### ${role}\n${message.content.trim()}`;
  });
  return [
    existingSummary.trim()
      ? `### 已有摘要\n${existingSummary.trim()}`
      : "",
    ...sections,
  ].filter(Boolean).join("\n\n");
}

export function toModelMessages(messages: ChatMessage[]) {
  return messages
    .filter((message) => message.content.trim() && message.status === "completed")
    .map((message) => ({
      role: message.role as MessageRole,
      content: message.content,
    }));
}
