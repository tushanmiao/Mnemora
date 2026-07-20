import { useCallback, useRef, useState, type MutableRefObject } from "react";
import {
  cancelChatStream,
  completeChat,
  normalizeModelError,
  startChatStream,
  type ModelStreamEvent,
} from "../api/chat";
import type { AppSettings, ResponseLanguage } from "../../../types/appSettings";
import type { ChatMessage } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import type {
  ProviderConfig,
  ProviderModelConfig,
} from "../../../types/modelSettings";
import {
  appendStreamingDelta,
  appendStreamingReasoningDelta,
  consumeStreamingMessage,
  startStreamingMessage,
} from "../stores/streamingStore";
import { estimateConversationContext } from "../utils/contextUsage";
import {
  activeContextMessages,
  AUTO_COMPRESSION_RATIO,
  compressionCandidates,
  compressionTranscript,
  contextSummaryPrompt,
  toModelMessages,
} from "../utils/contextCompression";

const MAX_TEMPORARY_TITLE_LENGTH = 24;
const RESPONSE_LANGUAGE_PROMPTS: Partial<Record<ResponseLanguage, string>> = {
  zh: "请使用简体中文回答。",
  zhHant: "請使用繁體中文回答。",
  en: "Please answer in English.",
};

export type SelectedModel = {
  provider: ProviderConfig;
  model: ProviderModelConfig;
};

type ActiveStreamRun = {
  runId: string;
  conversationId: string;
  messageId: string;
  terminalReceived: boolean;
};

type PreparedGeneration = {
  runningConversation: Conversation;
  compressionConversation: Conversation;
  pendingUserMessage: ChatMessage | null;
  assistantMessageId: string;
  selectedModel: SelectedModel;
};

type UseChatRuntimeOptions = {
  appSettings: AppSettings;
  currentConversation: Conversation | null;
  currentModel: SelectedModel | null;
  conversationsRef: MutableRefObject<Conversation[]>;
  requestInFlightRef: MutableRefObject<boolean>;
  cacheConversation: (conversation: Conversation, updateSummary?: boolean) => void;
  saveStableConversation: (conversation: Conversation) => void;
};

function createTemporaryTitle(content: string) {
  const characters = Array.from(content.replace(/\s+/g, " ").trim());
  if (characters.length <= MAX_TEMPORARY_TITLE_LENGTH) return characters.join("");
  return `${characters.slice(0, MAX_TEMPORARY_TITLE_LENGTH).join("")}...`;
}

function composeSystemPrompt(settings: AppSettings, conversation: Conversation) {
  return [
    settings.systemPrompt.trim(),
    conversation.systemPrompt.trim(),
    contextSummaryPrompt(conversation),
    RESPONSE_LANGUAGE_PROMPTS[settings.responseLanguage] ?? "",
  ].filter(Boolean).join("\n\n");
}

function createAssistantMessage(
  conversationId: string,
  selectedModel: SelectedModel,
  messageId: string = crypto.randomUUID(),
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
    },
  };
}

function resetCompression(conversation: Conversation): Conversation {
  return {
    ...conversation,
    contextSummary: "",
    compressedUntilMessageId: null,
    contextCompressionCount: 0,
  };
}

async function compressConversation(
  settings: AppSettings,
  conversation: Conversation,
  selectedModel: SelectedModel,
  pendingUserMessage: ChatMessage | null,
) {
  const contextWindowTokens = selectedModel.model.contextWindowTokens;
  if (!contextWindowTokens) return null;
  const projectedMessages = pendingUserMessage
    ? [...activeContextMessages(conversation), pendingUserMessage]
    : activeContextMessages(conversation);
  const projected = estimateConversationContext(
    projectedMessages,
    composeSystemPrompt(settings, conversation),
  );
  if (projected.tokens / contextWindowTokens < AUTO_COMPRESSION_RATIO) return null;

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
      "保留事实、用户偏好、约束、关键结论、代码或文件名称、待办事项和未解决问题。",
      "删除寒暄、重复内容和无关细节。不要回答对话中的问题，只输出可供后续模型继续工作的中文摘要。",
    ].join("\n"),
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

export function useChatRuntime({
  appSettings,
  currentConversation,
  currentModel,
  conversationsRef,
  requestInFlightRef,
  cacheConversation,
  saveStableConversation,
}: UseChatRuntimeOptions) {
  const [requestInFlight, setRequestInFlight] = useState(false);
  const [stopRequested, setStopRequested] = useState(false);
  const activeStreamRunRef = useRef<ActiveStreamRun | null>(null);

  const finalizeStreamRun = useCallback((
    run: ActiveStreamRun,
    terminal: {
      status: "completed" | "stopped" | "error";
      usage?: ChatMessage["usage"];
      errorMessage?: string;
    },
  ) => {
    const streamedMessage = consumeStreamingMessage(run.messageId);
    const conversation = conversationsRef.current.find((item) => item.id === run.conversationId);
    if (!conversation) return;
    const updatedAt = Date.now();
    const nextConversation: Conversation = {
      ...conversation,
      messages: conversation.messages.map((message) => (
        message.id === run.messageId
          ? {
              ...message,
              content: streamedMessage?.content ?? message.content,
              reasoning: streamedMessage?.reasoning || message.reasoning,
              status: terminal.status,
              usage: terminal.usage ?? message.usage,
              errorMessage: terminal.errorMessage,
              updatedAt,
            }
          : message
      )),
      updatedAt,
    };
    cacheConversation(nextConversation);
    saveStableConversation(nextConversation);
  }, [cacheConversation, conversationsRef, saveStableConversation]);

  const handleStreamEvent = useCallback((event: ModelStreamEvent) => {
    const run = activeStreamRunRef.current;
    if (
      !run
      || event.runId !== run.runId
      || event.conversationId !== run.conversationId
      || event.messageId !== run.messageId
    ) return;
    if (run.terminalReceived) return;

    switch (event.type) {
      case "started":
        return;
      case "textDelta":
        appendStreamingDelta(run.messageId, event.delta);
        return;
      case "reasoningDelta":
        appendStreamingReasoningDelta(run.messageId, event.delta);
        return;
      case "completed":
        run.terminalReceived = true;
        finalizeStreamRun(run, { status: "completed", usage: event.usage });
        return;
      case "stopped":
        run.terminalReceived = true;
        finalizeStreamRun(run, { status: "stopped" });
        return;
      case "error":
        run.terminalReceived = true;
        finalizeStreamRun(run, { status: "error", errorMessage: event.error.message });
    }
  }, [finalizeStreamRun]);

  const stopGeneration = useCallback(() => {
    const run = activeStreamRunRef.current;
    if (!run || stopRequested) return;
    setStopRequested(true);
    void cancelChatStream(run.runId).catch(() => setStopRequested(false));
  }, [stopRequested]);

  const runPreparedGeneration = useCallback(async ({
    runningConversation: initialRunningConversation,
    compressionConversation,
    pendingUserMessage,
    assistantMessageId,
    selectedModel,
  }: PreparedGeneration) => {
    const targetConversationId = initialRunningConversation.id;
    if (appSettings.streamEnabled) {
      startStreamingMessage(assistantMessageId);
    }
    requestInFlightRef.current = true;
    setRequestInFlight(true);
    setStopRequested(false);
    let runningConversation = initialRunningConversation;
    cacheConversation(runningConversation);
    saveStableConversation({
      ...runningConversation,
      messages: runningConversation.messages.filter((message) => message.id !== assistantMessageId),
    });

    let streamRun: ActiveStreamRun | null = null;
    try {
      const compression = await compressConversation(
        appSettings,
        compressionConversation,
        selectedModel,
        pendingUserMessage,
      );
      if (compression?.summary) {
        runningConversation = {
          ...runningConversation,
          contextSummary: compression.summary,
          compressedUntilMessageId: compression.boundaryMessageId,
          contextCompressionCount: runningConversation.contextCompressionCount + 1,
          updatedAt: Date.now(),
        };
        cacheConversation(runningConversation);
        saveStableConversation({
          ...runningConversation,
          messages: runningConversation.messages.filter((message) => message.id !== assistantMessageId),
        });
      }
      const modelMessages = toModelMessages(activeContextMessages(runningConversation));
      const completionRequest = {
        providerId: selectedModel.provider.id,
        modelId: selectedModel.model.id,
        conversationId: targetConversationId,
        messageId: assistantMessageId,
        systemPrompt: composeSystemPrompt(appSettings, runningConversation),
        messages: modelMessages,
        options: {
          maxOutputTokens: appSettings.maxOutputTokens,
          thinkingEnabled: appSettings.thinkingEnabled,
        },
      };

      if (appSettings.streamEnabled) {
        streamRun = {
          runId: crypto.randomUUID(),
          conversationId: targetConversationId,
          messageId: assistantMessageId,
          terminalReceived: false,
        };
        activeStreamRunRef.current = streamRun;
        await startChatStream({
          runId: streamRun.runId,
          conversationId: streamRun.conversationId,
          messageId: streamRun.messageId,
          completion: completionRequest,
        }, handleStreamEvent);
        if (!streamRun.terminalReceived) {
          throw new Error("流式请求结束，但没有收到完成、停止或错误事件。");
        }
        return;
      }

      const response = await completeChat(completionRequest);
      const conversation = conversationsRef.current.find((item) => item.id === targetConversationId);
      if (conversation) {
        const completedAt = Date.now();
        const completedConversation: Conversation = {
          ...conversation,
          messages: conversation.messages.map((message) => (
            message.id === assistantMessageId
              ? {
                  ...message,
                  content: response.text,
                  reasoning: response.reasoning,
                  status: "completed",
                  usage: response.usage,
                  updatedAt: completedAt,
                }
              : message
          )),
          updatedAt: completedAt,
        };
        cacheConversation(completedConversation);
        saveStableConversation(completedConversation);
      }
    } catch (error) {
      const modelError = normalizeModelError(error);
      if (streamRun) {
        if (!streamRun.terminalReceived) {
          streamRun.terminalReceived = true;
          finalizeStreamRun(streamRun, { status: "error", errorMessage: modelError.message });
        }
        return;
      }
      if (appSettings.streamEnabled) {
        consumeStreamingMessage(assistantMessageId);
      }
      const conversation = conversationsRef.current.find((item) => item.id === targetConversationId);
      if (conversation) {
        const failedAt = Date.now();
        const failedConversation: Conversation = {
          ...conversation,
          messages: conversation.messages.map((message) => (
            message.id === assistantMessageId
              ? { ...message, status: "error", errorMessage: modelError.message, updatedAt: failedAt }
              : message
          )),
          updatedAt: failedAt,
        };
        cacheConversation(failedConversation);
        saveStableConversation(failedConversation);
      }
    } finally {
      if (activeStreamRunRef.current?.runId === streamRun?.runId) activeStreamRunRef.current = null;
      requestInFlightRef.current = false;
      setRequestInFlight(false);
      setStopRequested(false);
    }
  }, [
    appSettings,
    cacheConversation,
    conversationsRef,
    finalizeStreamRun,
    handleStreamEvent,
    requestInFlightRef,
    saveStableConversation,
  ]);

  const sendMessage = useCallback(async (rawContent: string) => {
    const content = rawContent.trim();
    const targetConversation = currentConversation;
    const selectedModel = currentModel;
    if (!content || !targetConversation || !selectedModel || requestInFlightRef.current) return;

    const now = Date.now();
    const userMessage: ChatMessage = {
      id: crypto.randomUUID(),
      conversationId: targetConversation.id,
      role: "user",
      content,
      status: "completed",
      createdAt: now,
      updatedAt: now,
    };
    const assistantMessage = createAssistantMessage(targetConversation.id, selectedModel);
    const runningConversation: Conversation = {
      ...targetConversation,
      title: targetConversation.messages.length === 0
        ? createTemporaryTitle(content)
        : targetConversation.title,
      messages: [...targetConversation.messages, userMessage, assistantMessage],
      providerId: selectedModel.provider.id,
      modelId: selectedModel.model.id,
      updatedAt: now,
    };
    await runPreparedGeneration({
      runningConversation,
      compressionConversation: targetConversation,
      pendingUserMessage: userMessage,
      assistantMessageId: assistantMessage.id,
      selectedModel,
    });
  }, [currentConversation, currentModel, requestInFlightRef, runPreparedGeneration]);

  const regenerateMessage = useCallback(async (messageId: string) => {
    const targetConversation = currentConversation;
    const selectedModel = currentModel;
    if (!targetConversation || !selectedModel || requestInFlightRef.current) return;
    const messageIndex = targetConversation.messages.findIndex((message) => message.id === messageId);
    if (messageIndex < 0 || targetConversation.messages[messageIndex].role !== "assistant") return;
    const history = targetConversation.messages.slice(0, messageIndex);
    if (!history.some((message) => message.role === "user" && message.content.trim())) return;

    const now = Date.now();
    const compressionConversation = resetCompression({
      ...targetConversation,
      messages: history,
      updatedAt: now,
    });
    const assistantMessage = createAssistantMessage(targetConversation.id, selectedModel, messageId);
    const runningConversation: Conversation = {
      ...compressionConversation,
      messages: [...history, assistantMessage],
      providerId: selectedModel.provider.id,
      modelId: selectedModel.model.id,
    };
    await runPreparedGeneration({
      runningConversation,
      compressionConversation,
      pendingUserMessage: null,
      assistantMessageId: assistantMessage.id,
      selectedModel,
    });
  }, [currentConversation, currentModel, requestInFlightRef, runPreparedGeneration]);

  const editMessage = useCallback(async (messageId: string, rawContent: string) => {
    const content = rawContent.trim();
    const targetConversation = currentConversation;
    if (!content || !targetConversation || requestInFlightRef.current) return;
    const messageIndex = targetConversation.messages.findIndex((message) => message.id === messageId);
    if (messageIndex < 0) return;
    const originalMessage = targetConversation.messages[messageIndex];
    const now = Date.now();

    if (originalMessage.role === "assistant") {
      const editedConversation = resetCompression({
        ...targetConversation,
        messages: targetConversation.messages.map((message) => (
          message.id === messageId
            ? {
                ...message,
                content,
                reasoning: undefined,
                status: "completed",
                usage: undefined,
                errorMessage: undefined,
                updatedAt: now,
              }
            : message
        )),
        updatedAt: now,
      });
      cacheConversation(editedConversation);
      saveStableConversation(editedConversation);
      return;
    }

    const editedUserMessage: ChatMessage = {
      ...originalMessage,
      content,
      status: "completed",
      errorMessage: undefined,
      updatedAt: now,
    };
    const history = [
      ...targetConversation.messages.slice(0, messageIndex),
      editedUserMessage,
    ];
    const compressionConversation = resetCompression({
      ...targetConversation,
      title: messageIndex === 0 ? createTemporaryTitle(content) : targetConversation.title,
      messages: history,
      updatedAt: now,
    });
    const selectedModel = currentModel;
    if (!selectedModel) {
      cacheConversation(compressionConversation);
      saveStableConversation(compressionConversation);
      return;
    }

    const assistantMessage = createAssistantMessage(targetConversation.id, selectedModel);
    const runningConversation: Conversation = {
      ...compressionConversation,
      messages: [...history, assistantMessage],
      providerId: selectedModel.provider.id,
      modelId: selectedModel.model.id,
    };
    await runPreparedGeneration({
      runningConversation,
      compressionConversation,
      pendingUserMessage: null,
      assistantMessageId: assistantMessage.id,
      selectedModel,
    });
  }, [
    cacheConversation,
    currentConversation,
    currentModel,
    requestInFlightRef,
    runPreparedGeneration,
    saveStableConversation,
  ]);

  const deleteMessage = useCallback((messageId: string) => {
    const targetConversation = currentConversation;
    if (!targetConversation || requestInFlightRef.current) return;
    if (!targetConversation.messages.some((message) => message.id === messageId)) return;
    const now = Date.now();
    const nextConversation = resetCompression({
      ...targetConversation,
      messages: targetConversation.messages.filter((message) => message.id !== messageId),
      updatedAt: now,
    });
    cacheConversation(nextConversation);
    saveStableConversation(nextConversation);
  }, [cacheConversation, currentConversation, requestInFlightRef, saveStableConversation]);

  return {
    requestInFlight,
    stopRequested,
    sendMessage,
    stopGeneration,
    editMessage,
    regenerateMessage,
    deleteMessage,
  };
}
