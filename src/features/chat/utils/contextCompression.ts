import type { ChatMessage, MessageRole } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import {
  formatLiteratureReferencesForCompression,
  formatLiteratureReferencesForModel,
} from "./literatureReferences";
import { formatNoteReferencesForCompression, formatNoteReferencesForModel } from "./noteReferences";

const RECENT_MESSAGES_TO_KEEP = 4;
export const CONTEXT_SAFETY_RATIO = 0.08;
export const MIN_CONTEXT_SAFETY_TOKENS = 4_096;
export const COMPRESSION_CHUNK_TARGET_TOKENS = 16_000;

export function contextInputBudget(contextWindowTokens: number, maxOutputTokens: number) {
  const safety = Math.max(
    MIN_CONTEXT_SAFETY_TOKENS,
    Math.ceil(contextWindowTokens * CONTEXT_SAFETY_RATIO),
  );
  return Math.max(0, contextWindowTokens - Math.max(0, maxOutputTokens) - safety);
}

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
    .filter((message) => message.status === "completed" && (
      message.content.trim()
      || (message.attachments?.length ?? 0) > 0
      || (message.literatureReferences?.length ?? 0) > 0
      || (message.noteReferences?.length ?? 0) > 0
    ));
  if (active.length <= RECENT_MESSAGES_TO_KEEP + 1) return [];
  return active.slice(0, -RECENT_MESSAGES_TO_KEEP);
}

export function compressionTranscript(
  existingSummary: string,
  messages: ChatMessage[],
) {
  const sections = messages.map((message) => {
    const role = message.role === "user" ? "用户" : "助手";
    const imageNames = message.attachments
      ?.filter((attachment) => attachment.kind === "image")
      .map((attachment) => attachment.name) ?? [];
    const fileNames = message.attachments
      ?.filter((attachment) => attachment.kind === "file")
      .map((attachment) => attachment.name) ?? [];
    return [
      `### ${role}`,
      message.content.trim(),
      formatLiteratureReferencesForCompression(message.literatureReferences ?? []),
      formatNoteReferencesForCompression(message.noteReferences ?? []),
      imageNames.length > 0 ? `图片附件（正文已省略）：${imageNames.join("、")}` : "",
      fileNames.length > 0 ? `文件附件（正文未解析）：${fileNames.join("、")}` : "",
    ].filter(Boolean).join("\n");
  });
  return [
    existingSummary.trim()
      ? `### 已有摘要\n${existingSummary.trim()}`
      : "",
    ...sections,
  ].filter(Boolean).join("\n\n");
}

function textTokenUnits(value: string) {
  let units = 0;
  for (const character of value) units += character.charCodeAt(0) < 128 ? 1 : 4;
  return units;
}

function splitTextByTokenBudget(value: string, tokenBudget: number) {
  const chunks: string[] = [];
  let current = "";
  let currentUnits = 0;
  const maximumUnits = Math.max(1, tokenBudget) * 4;
  for (const paragraph of value.split(/(?<=\n\n)/u)) {
    const paragraphUnits = textTokenUnits(paragraph);
    if (current && currentUnits + paragraphUnits > maximumUnits) {
      chunks.push(current);
      current = "";
      currentUnits = 0;
    }
    if (paragraphUnits <= maximumUnits) {
      current += paragraph;
      currentUnits += paragraphUnits;
      continue;
    }
    for (const character of paragraph) {
      const characterUnits = character.charCodeAt(0) < 128 ? 1 : 4;
      if (current && currentUnits + characterUnits > maximumUnits) {
        chunks.push(current);
        current = "";
        currentUnits = 0;
      }
      current += character;
      currentUnits += characterUnits;
    }
  }
  if (current.trim()) chunks.push(current);
  return chunks;
}

export function compressionTranscriptBatches(
  messages: ChatMessage[],
  tokenBudget = COMPRESSION_CHUNK_TARGET_TOKENS,
) {
  const batches: string[] = [];
  let current = "";
  let currentUnits = 0;
  const maximumUnits = Math.max(1, tokenBudget) * 4;
  const separator = "\n\n";
  const separatorUnits = textTokenUnits(separator);
  for (const message of messages) {
    const transcript = compressionTranscript("", [message]);
    for (const segment of splitTextByTokenBudget(transcript, tokenBudget)) {
      const segmentUnits = textTokenUnits(segment);
      if (current && currentUnits + separatorUnits + segmentUnits > maximumUnits) {
        batches.push(current);
        current = segment;
        currentUnits = segmentUnits;
      } else {
        if (current) {
          current += separator;
          currentUnits += separatorUnits;
        }
        current += segment;
        currentUnits += segmentUnits;
      }
    }
  }
  if (current.trim()) batches.push(current);
  return batches;
}

/** 未交付回答的助手消息在上下文里的原因说明上限，防止把整段上游报错灌进 prompt。 */
const FAILED_TURN_REASON_LIMIT = 160;

/**
 * 这条助手消息有没有交付一个完整回答。
 *
 * `stopped` 也算未交付：用户中途按停时那一轮同样没有形成结论，而且用户贴的附件
 * 还没有被模型真正消费过。
 */
function isUndeliveredAssistantTurn(message: ChatMessage) {
  return message.role === "assistant"
    && (message.status === "error" || message.status === "stopped");
}

function hasModelPayload(message: ChatMessage) {
  return Boolean(
    message.content.trim()
    || (message.attachments?.length ?? 0) > 0
    || (message.literatureReferences?.length ?? 0) > 0
    || (message.noteReferences?.length ?? 0) > 0,
  );
}

/**
 * 未交付的助手轮次要留一条占位，不能整条丢掉。
 *
 * 丢掉它会让 user/assistant 交替断开：连续两次失败之后，请求尾部会出现三条连续的
 * 用户消息、三个都没被回答的问题，模型可能挑其中信息量最大的那一条作答，而不是
 * 用户真正在等的最后一条。
 *
 * 占位只陈述事实，不写「请回答最后一条」这类指令 —— 助手消息不应该成为指令来源。
 * `supersededByNewRequest` 是位置推导出来的客观信息（这条失败之后用户又发过消息），
 * 它和恢复交替一起，让「最后一条用户消息才是当前请求」在结构上无歧义。
 */
function undeliveredTurnContent(message: ChatMessage, supersededByNewRequest: boolean) {
  const body = message.content.trim();
  // 上游报错常常自带句末标点，直接拼接会得到「later.。」这种双标点。
  const reason = message.errorMessage
    ?.trim()
    .slice(0, FAILED_TURN_REASON_LIMIT)
    .replace(/[。.；;、,\s]+$/u, "");
  const cause = body
    ? "这一轮回复被中断，没有完成"
    : reason
      ? `这一轮回复失败，没有产生回答：${reason}`
      : "这一轮回复没有产生回答";
  const note = supersededByNewRequest
    ? `（${cause}。用户没有等待重试，随后发送了新的消息。）`
    : `（${cause}。）`;
  return body ? `${body}\n\n${note}` : note;
}

export function toModelMessages(messages: ChatMessage[]) {
  const sendable = messages.filter((message) => (
    message.status === "completed"
      ? hasModelPayload(message)
      : isUndeliveredAssistantTurn(message)
  ));
  return sendable.map((message, index) => {
    if (isUndeliveredAssistantTurn(message)) {
      const supersededByNewRequest = sendable
        .slice(index + 1)
        .some((later) => later.role === "user");
      return {
        role: message.role as MessageRole,
        content: undeliveredTurnContent(message, supersededByNewRequest),
        attachments: [],
        failed: true,
      };
    }
    const literatureContext = formatLiteratureReferencesForModel(
      message.literatureReferences ?? [],
    );
    const noteContext = formatNoteReferencesForModel(message.noteReferences ?? []);
    const referenceContext = [literatureContext, noteContext].filter(Boolean).join("\n\n");
    return {
      role: message.role as MessageRole,
      content: referenceContext
        ? [referenceContext, message.content.trim() ? `用户问题：\n${message.content}` : ""]
            .filter(Boolean)
            .join("\n\n")
        : message.content,
      attachments: message.attachments ?? [],
      failed: false,
    };
  });
}
