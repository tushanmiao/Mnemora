import { completeChat } from "../api/chat";
import type { AppSettings } from "../../../types/appSettings";
import type { ActivatedSkillSnapshot, ChatMessage } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import type { ProviderConfig, ProviderModelConfig } from "../../../types/modelSettings";
import { estimateConversationContext } from "../utils/contextUsage";
import {
  activeContextMessages,
  COMPRESSION_CHUNK_TARGET_TOKENS,
  compressionCandidates,
  compressionTranscriptBatches,
  contextInputBudget,
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
    // 空事件账本表示“本轮尚未发生真实 Agent 活动”。它与旧消息缺少该字段
    // 有意区分，避免用户预选 Skill 在模型尚未加载/执行前就被显示为工作流。
    agentEvents: [],
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
    const inputBudget = contextInputBudget(contextWindowTokens, settings.maxOutputTokens);
    if (projected.tokens <= inputBudget) return null;
  }

  const candidates = compressionCandidates(conversation);
  const boundary = candidates[candidates.length - 1];
  if (!boundary) return null;
  const compressionOutputTokens = Math.min(4_096, settings.maxOutputTokens);
  const compressionInputBudget = contextWindowTokens
    ? contextInputBudget(contextWindowTokens, compressionOutputTokens)
    : COMPRESSION_CHUNK_TARGET_TOKENS;
  const summaryAndPromptReserve = 6_144;
  if (compressionInputBudget <= summaryAndPromptReserve + 512) {
    throw new Error(
      "当前模型的可用输入预算不足以安全压缩上下文。请降低最大输出 Token，或在模型设置中确认上下文窗口配置。",
    );
  }
  const batchTarget = Math.min(
    COMPRESSION_CHUNK_TARGET_TOKENS,
    compressionInputBudget - summaryAndPromptReserve,
  );
  const batches = compressionTranscriptBatches(candidates, batchTarget);
  let summary = conversation.contextSummary.trim();
  for (const [index, batch] of batches.entries()) {
    const response = await completeChat({
      providerId: selectedModel.provider.id,
      modelId: selectedModel.model.id,
      conversationId: conversation.id,
      messageId: crypto.randomUUID(),
      operation: "contextCompression",
      systemPrompt: [
        "你负责分块压缩对话上下文。",
        "保留事实、用户偏好、约束、关键结论、文献名称与页码、代码或文件名称、待办事项和未解决问题。",
        "合并已有摘要与当前分块，删除寒暄、重复内容和无关细节。不要回答对话中的问题，只输出可供后续模型继续工作的中文摘要。",
        `当前处理第 ${index + 1}/${batches.length} 个分块。`,
        options.focus?.trim() ? `用户要求本次压缩重点保留：${options.focus.trim()}` : "",
      ].filter(Boolean).join("\n"),
      messages: [{
        role: "user",
        content: [
          summary ? `### 已有摘要\n${summary}` : "",
          `### 当前分块\n${batch}`,
        ].filter(Boolean).join("\n\n"),
      }],
      options: {
        maxOutputTokens: compressionOutputTokens,
        thinkingEnabled: false,
      },
    });
    summary = response.text.trim();
    if (!summary) throw new Error(`上下文压缩第 ${index + 1}/${batches.length} 个分块返回空摘要。`);
  }
  return {
    summary,
    boundaryMessageId: boundary.id,
  };
}
