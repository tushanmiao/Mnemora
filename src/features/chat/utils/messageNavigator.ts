import type { ChatMessage } from "../../../types/chat";

export type MessageNavigatorNode = {
  id: string;
  targetMessageId: string;
  targetRenderIndex: number;
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

  for (const [index, message] of messages.entries()) {
    if (message.role === "user") {
      const attachmentTitle = message.attachments?.length
        ? `附件：${message.attachments.map((attachment) => attachment.name).join("、")}`
        : "";
      const literatureTitle = message.literatureReferences?.[0]
        ? `文献：${message.literatureReferences[0].title}，第 ${message.literatureReferences[0].pageIndex + 1} 页`
        : "";
      const noteTitle = message.noteReferences?.[0]
        ? `笔记：${message.noteReferences[0].noteTitle}`
        : "";
      current = {
        id: `turn-${message.id}`,
        targetMessageId: message.id,
        targetRenderIndex: index,
        title: preview(message.content) || attachmentTitle || literatureTitle || noteTitle || "空消息",
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

/** 根据虚拟列表阅读线所在的 item，找到当前轮次。 */
export function activeMessageNavigatorNodeId(
  nodes: MessageNavigatorNode[],
  renderIndex: number,
) {
  if (nodes.length === 0) return null;
  let active = nodes[0];
  for (const node of nodes) {
    if (node.targetRenderIndex > renderIndex) break;
    active = node;
  }
  return active.id;
}
