import { useCallback, useRef, useState, type MutableRefObject } from "react";
import {
  cancelChatStream,
  completeChat,
  normalizeModelError,
  startChatStream,
  type ModelStreamEvent,
} from "../api/chat";
import type { AppSettings, ResponseLanguage } from "../../../types/appSettings";
import type { ChatMessage, MessageRole } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import type {
  ProviderConfig,
  ProviderModelConfig,
} from "../../../types/modelSettings";
import {
  appendStreamingDelta,
  consumeStreamingMessage,
  startStreamingMessage,
} from "../stores/streamingStore";

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

function composeSystemPrompt(settings: AppSettings, conversationPrompt: string) {
  return [
    settings.systemPrompt.trim(),
    conversationPrompt.trim(),
    RESPONSE_LANGUAGE_PROMPTS[settings.responseLanguage] ?? "",
  ].filter(Boolean).join("\n\n");
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
    const streamedContent = consumeStreamingMessage(run.messageId);
    const conversation = conversationsRef.current.find((item) => item.id === run.conversationId);
    if (!conversation) return;
    const updatedAt = Date.now();
    const nextConversation: Conversation = {
      ...conversation,
      messages: conversation.messages.map((message) => (
        message.id === run.messageId
          ? {
              ...message,
              content: streamedContent ?? message.content,
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

  const sendMessage = useCallback(async (rawContent: string) => {
    const content = rawContent.trim();
    const targetConversation = currentConversation;
    const selectedModel = currentModel;
    if (!content || !targetConversation || !selectedModel || requestInFlightRef.current) return;

    const now = Date.now();
    const targetConversationId = targetConversation.id;
    const userMessage: ChatMessage = {
      id: crypto.randomUUID(),
      conversationId: targetConversationId,
      role: "user",
      content,
      status: "completed",
      createdAt: now,
      updatedAt: now,
    };
    const assistantMessageId = crypto.randomUUID();
    const assistantMessage: ChatMessage = {
      id: assistantMessageId,
      conversationId: targetConversationId,
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
    const modelMessages = [...targetConversation.messages, userMessage]
      .filter((message) => message.content.trim() && message.status === "completed")
      .map((message) => ({ role: message.role as MessageRole, content: message.content }));

    requestInFlightRef.current = true;
    setRequestInFlight(true);
    setStopRequested(false);
    const title = targetConversation.messages.length === 0
      ? createTemporaryTitle(content)
      : targetConversation.title;
    const runningConversation: Conversation = {
      ...targetConversation,
      title,
      messages: [...targetConversation.messages, userMessage, assistantMessage],
      providerId: selectedModel.provider.id,
      modelId: selectedModel.model.id,
      updatedAt: now,
    };
    cacheConversation(runningConversation);
    saveStableConversation({
      ...runningConversation,
      messages: [...targetConversation.messages, userMessage],
    });

    let streamRun: ActiveStreamRun | null = null;
    try {
      const completionRequest = {
        providerId: selectedModel.provider.id,
        modelId: selectedModel.model.id,
        conversationId: targetConversationId,
        messageId: assistantMessageId,
        systemPrompt: composeSystemPrompt(appSettings, targetConversation.systemPrompt),
        messages: modelMessages,
        options: { maxOutputTokens: appSettings.maxOutputTokens },
      };

      if (appSettings.streamEnabled) {
        streamRun = {
          runId: crypto.randomUUID(),
          conversationId: targetConversationId,
          messageId: assistantMessageId,
          terminalReceived: false,
        };
        activeStreamRunRef.current = streamRun;
        startStreamingMessage(streamRun.messageId);
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
              ? { ...message, content: response.text, status: "completed", usage: response.usage, updatedAt: completedAt }
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
    currentConversation,
    currentModel,
    finalizeStreamRun,
    handleStreamEvent,
    requestInFlightRef,
    saveStableConversation,
  ]);

  return { requestInFlight, stopRequested, sendMessage, stopGeneration };
}
