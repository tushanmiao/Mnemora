import type { ChatMessage } from "../../../types/chat";

export type MessageNavigatorNode = {
  id: string;
  targetMessageId: string;
  title: string;
  answerPreview: string;
  modelLabel: string;
};

const PREVIEW_LIMIT = 180;

function preview(content: string) {
  const normalized = content.replace(/\s+/g, " ").trim();
  return normalized.length <= PREVIEW_LIMIT
    ? normalized
    : `${normalized.slice(0, PREVIEW_LIMIT).trimEnd()}...`;
}

/** 每条用户消息代表一轮对话，后续第一条助手回复作为导航预览。 */
export function buildMessageNavigatorNodes(messages: ChatMessage[]) {
  const nodes: MessageNavigatorNode[] = [];
  let current: MessageNavigatorNode | null = null;

  for (const message of messages) {
    if (message.role === "user") {
      current = {
        id: `turn-${message.id}`,
        targetMessageId: message.id,
        title: preview(message.content) || "空消息",
        answerPreview: "",
        modelLabel: "",
      };
      nodes.push(current);
      continue;
    }
    if (!current || current.answerPreview) continue;
    current.answerPreview = preview(message.content);
    current.modelLabel = message.modelSnapshot
      ? `${message.modelSnapshot.providerName} · ${message.modelSnapshot.displayName}`
      : "";
  }
  return nodes;
}
