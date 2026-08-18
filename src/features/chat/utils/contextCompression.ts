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

export function toModelMessages(messages: ChatMessage[]) {
  return messages
    .filter((message) => message.status === "completed" && (
      message.content.trim()
      || (message.attachments?.length ?? 0) > 0
      || (message.literatureReferences?.length ?? 0) > 0
      || (message.noteReferences?.length ?? 0) > 0
    ))
    .map((message) => {
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
      };
    });
}
