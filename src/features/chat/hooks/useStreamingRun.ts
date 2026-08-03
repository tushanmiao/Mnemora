import { useCallback, useEffect, useRef, useState, type MutableRefObject } from "react";
import {
  cancelChatStream,
  normalizeModelError,
  startChatStream,
  type ChatCompletionRequest,
  type ModelStreamEvent,
} from "../api/chat";
import type { ChatMessage } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import {
  appendStreamingDelta,
  appendStreamingReasoningDelta,
  consumeStreamingMessage,
  resetAllStreamingMessages,
  startStreamingMessage,
} from "../stores/streamingStore";

type ActiveStreamRun = {
  runId: string;
  conversationId: string;
  messageId: string;
  terminalReceived: boolean;
};

type UseStreamingRunOptions = {
  conversationsRef: MutableRefObject<Conversation[]>;
  cacheConversation: (conversation: Conversation, updateSummary?: boolean) => void;
  saveStableConversation: (conversation: Conversation) => void;
  releaseConversation: (conversationId: string) => void;
};

/** 管理单次流式请求、增量消息元数据和取消清理，不参与消息编排。 */
export function useStreamingRun({
  conversationsRef,
  cacheConversation,
  saveStableConversation,
  releaseConversation,
}: UseStreamingRunOptions) {
  const [stopRequested, setStopRequested] = useState(false);
  const activeRunRef = useRef<ActiveStreamRun | null>(null);

  const finalizeRun = useCallback((
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
              toolTraces: message.toolTraces?.map(({ approvalId: _approvalId, ...trace }) => trace),
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

  const updateMessageMetadata = useCallback((
    run: ActiveStreamRun,
    update: (message: ChatMessage) => ChatMessage,
  ) => {
    const conversation = conversationsRef.current.find((item) => item.id === run.conversationId);
    if (!conversation) return;
    cacheConversation({
      ...conversation,
      messages: conversation.messages.map((message) => (
        message.id === run.messageId ? update(message) : message
      )),
      updatedAt: Date.now(),
    });
  }, [cacheConversation, conversationsRef]);

  const handleEvent = useCallback((event: ModelStreamEvent) => {
    const run = activeRunRef.current;
    if (
      !run
      || event.runId !== run.runId
      || event.conversationId !== run.conversationId
      || event.messageId !== run.messageId
      || run.terminalReceived
    ) return;

    switch (event.type) {
      case "started":
        return;
      case "textDelta":
        appendStreamingDelta(run.messageId, event.delta);
        return;
      case "reasoningDelta":
        appendStreamingReasoningDelta(run.messageId, event.delta);
        return;
      case "toolTrace":
      case "toolApprovalRequested":
        updateMessageMetadata(run, (message) => {
          const nextTrace = {
            ...event.trace,
            approvalId: event.type === "toolApprovalRequested" ? event.approvalId : undefined,
          };
          const traces = message.toolTraces ?? [];
          const existing = traces.findIndex((trace) => trace.callId === nextTrace.callId);
          return {
            ...message,
            toolTraces: existing < 0
              ? [...traces, nextTrace]
              : traces.map((trace, index) => index === existing ? nextTrace : trace),
          };
        });
        return;
      case "skillActivated":
        updateMessageMetadata(run, (message) => (
          message.activatedSkills?.some((skill) => skill.id === event.skillId)
            ? message
            : {
                ...message,
                activatedSkills: [
                  ...(message.activatedSkills ?? []),
                  {
                    id: event.skillId,
                    name: event.name,
                    version: event.version,
                    contentHash: event.contentHash,
                    activation: "model" as const,
                  },
                ],
              }
        ));
        return;
      case "completed":
        run.terminalReceived = true;
        finalizeRun(run, { status: "completed", usage: event.usage });
        return;
      case "stopped":
        run.terminalReceived = true;
        finalizeRun(run, { status: "stopped" });
        return;
      case "error":
        run.terminalReceived = true;
        finalizeRun(run, { status: "error", errorMessage: event.error.message });
    }
  }, [finalizeRun, updateMessageMetadata]);

  const prepareStreamingMessage = useCallback((messageId: string) => {
    startStreamingMessage(messageId);
  }, []);

  const runStream = useCallback(async (
    conversationId: string,
    messageId: string,
    completion: ChatCompletionRequest,
  ) => {
    const run: ActiveStreamRun = {
      runId: crypto.randomUUID(),
      conversationId,
      messageId,
      terminalReceived: false,
    };
    activeRunRef.current = run;
    try {
      await startChatStream({ runId: run.runId, conversationId, messageId, completion }, handleEvent);
      if (!run.terminalReceived) {
        throw new Error("流式请求结束，但没有收到完成、停止或错误事件。");
      }
    } catch (error) {
      if (!run.terminalReceived) {
        run.terminalReceived = true;
        finalizeRun(run, { status: "error", errorMessage: normalizeModelError(error).message });
      }
    } finally {
      if (activeRunRef.current?.runId === run.runId) activeRunRef.current = null;
    }
  }, [finalizeRun, handleEvent]);

  const stopGeneration = useCallback(() => {
    const run = activeRunRef.current;
    if (!run || stopRequested) return;
    setStopRequested(true);
    void cancelChatStream(run.runId).catch(() => setStopRequested(false));
  }, [stopRequested]);

  const resetStopRequested = useCallback(() => setStopRequested(false), []);

  useEffect(() => () => {
    const activeRun = activeRunRef.current;
    activeRunRef.current = null;
    resetAllStreamingMessages();
    if (activeRun && !activeRun.terminalReceived) {
      void cancelChatStream(activeRun.runId).catch(() => undefined);
    }
    if (activeRun) releaseConversation(activeRun.conversationId);
  }, [releaseConversation]);

  return {
    stopRequested,
    prepareStreamingMessage,
    runStream,
    stopGeneration,
    resetStopRequested,
  };
}
