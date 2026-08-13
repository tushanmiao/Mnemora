import { completeChat } from "../api/chat";
import type { AppSettings } from "../../../types/appSettings";
import type { ActivatedSkillSnapshot, ChatMessage } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import type { ProviderConfig, ProviderModelConfig } from "../../../types/modelSettings";
import { estimateConversationContext } from "../utils/contextUsage";
import {
  activeContextMessages,
  AUTO_COMPRESSION_RATIO,
  compressionCandidates,
  compressionTranscript,
  contextSummaryPrompt,
} from "../utils/contextCompression";
import { composeChatSystemPrompt } from "../utils/systemPrompt";

const MAX_TEMPORARY_TITLE_LENGTH = 24;

export type SelectedModel = {
  provider: ProviderConfig;
  model: ProviderModelConfig;
};

export function createTemporaryTitle(content: string) {
  const characters = Array.from(content.replace(/\s+/g, " ").trim());
  if (characters.length <= MAX_TEMPORARY_TITLE_LENGTH) return characters.join("");
  return `${characters.slice(0, MAX_TEMPORARY_TITLE_LENGTH).join("")}...`;
}

export function composeSystemPrompt(settings: AppSettings, conversation: Conversation) {
  return composeChatSystemPrompt({
    globalPrompt: settings.systemPrompt,
    conversationPrompt: conversation.systemPrompt,
    contextSummary: contextSummaryPrompt(conversation),
    responseLanguage: settings.responseLanguage,
  });
}

export function createAssistantMessage(
  conversationId: string,
  selectedModel: SelectedModel,
  messageId: string = crypto.randomUUID(),
  activatedSkills: ActivatedSkillSnapshot[] = [],
): ChatMessage {
  const now = Date.now();
  return {
    id: messageId,
    conversationId,
    role: "assistant",
    content: "",
    status: "pending",
    createdAt: now,
    updatedAt: now,
    modelId: selectedModel.model.id,
    modelSnapshot: {
      id: selectedModel.model.id,
      apiModel: selectedModel.model.apiModel,
      displayName: selectedModel.model.displayName,
      providerId: selectedModel.provider.id,
      providerName: selectedModel.provider.name,
      protocol: selectedModel.provider.protocol,
    },
    activatedSkills,
  };
}

export function resetCompression(conversation: Conversation): Conversation {
  return {
    ...conversation,
    contextSummary: "",
    compressedUntilMessageId: null,
    contextCompressionCount: 0,
  };
}

export async function compressConversation(
  settings: AppSettings,
  conversation: Conversation,
  selectedModel: SelectedModel,
  pendingUserMessage: ChatMessage | null,
  options: { force?: boolean; focus?: string } = {},
) {
  const contextWindowTokens = selectedModel.model.contextWindowTokens;
  if (!options.force) {
    if (!contextWindowTokens) return null;
    const projectedMessages = pendingUserMessage
      ? [...activeContextMessages(conversation), pendingUserMessage]
      : activeContextMessages(conversation);
    const projected = estimateConversationContext(
      projectedMessages,
      composeSystemPrompt(settings, conversation),
    );
    if (projected.tokens / contextWindowTokens < AUTO_COMPRESSION_RATIO) return null;
  }

  const candidates = compressionCandidates(conversation);
  const boundary = candidates[candidates.length - 1];
  if (!boundary) return null;
  const response = await completeChat({
    providerId: selectedModel.provider.id,
    modelId: selectedModel.model.id,
    conversationId: conversation.id,
    messageId: crypto.randomUUID(),
    operation: "contextCompression",
    systemPrompt: [
      "你负责压缩对话上下文。",
      "保留事实、用户偏好、约束、关键结论、文献名称与页码、代码或文件名称、待办事项和未解决问题。",
      "删除寒暄、重复内容和无关细节。不要回答对话中的问题，只输出可供后续模型继续工作的中文摘要。",
      options.focus?.trim() ? `用户要求本次压缩重点保留：${options.focus.trim()}` : "",
    ].filter(Boolean).join("\n"),
    messages: [{
      role: "user",
      content: compressionTranscript(conversation.contextSummary, candidates),
    }],
    options: {
      maxOutputTokens: Math.min(4_096, settings.maxOutputTokens),
      thinkingEnabled: false,
    },
  });
  return {
    summary: response.text.trim(),
    boundaryMessageId: boundary.id,
  };
}
