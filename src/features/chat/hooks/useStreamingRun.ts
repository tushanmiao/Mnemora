import { useCallback, useEffect, useRef, useState, type MutableRefObject } from "react";
import {
  cancelChatStream,
  normalizeModelError,
  startChatStream,
  type ChatCompletionRequest,
  type ModelStreamEvent,
} from "../api/chat";
import type { ChatMessage, ToolTrace } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import {
  appendStreamingDelta,
  appendStreamingReasoningDelta,
  consumeStreamingMessage,
  resetAllStreamingMessages,
  startStreamingMessage,
} from "../stores/streamingStore";
import { workflowSummaryForMessage } from "../agent/projections/workflowProjection";

type ActiveStreamRun = {
  runId: string;
  conversationId: string;
  messageId: string;
  terminalReceived: boolean;
  nextSequence: number;
  reasoningEventId: string | null;
  reasoningLength: number;
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
      messages: conversation.messages.map((message) => {
        if (message.id !== run.messageId) return message;
        const finalized: ChatMessage = {
          ...message,
          content: streamedMessage?.content ?? message.content,
          reasoning: streamedMessage?.reasoning || message.reasoning,
          status: terminal.status,
          usage: terminal.usage ?? message.usage,
          // 两个字段都只在等待期间有意义：留着会让历史消息渲染出一个永远无人应答的弹窗。
          toolTraces: message.toolTraces?.map(
            ({ approvalId: _approvalId, interrupt: _interrupt, ...trace }) => trace,
          ),
          agentEvents: message.agentEvents?.slice(-256),
          agentRunId: run.runId,
          errorMessage: terminal.errorMessage,
          updatedAt,
        };
        return { ...finalized, workflowSummary: workflowSummaryForMessage(finalized) };
      }),
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
        updateMessageMetadata(run, (message) => ({
          ...message,
          agentRunId: run.runId,
        }));
        return;
      case "textDelta":
        appendStreamingDelta(run.messageId, event.delta);
        return;
      case "reasoningDelta":
        appendStreamingReasoningDelta(run.messageId, event.delta);
        updateMessageMetadata(run, (message) => {
          const nextLength = run.reasoningLength + event.delta.length;
          run.reasoningLength = nextLength;
          const reasoningEventId = run.reasoningEventId ?? crypto.randomUUID();
          if (!run.reasoningEventId) {
            run.reasoningEventId = reasoningEventId;
            const events = [...(message.agentEvents ?? [])];
            events.push({
              id: reasoningEventId,
              sequence: run.nextSequence++,
              createdAt: Date.now(),
              kind: "reasoning",
              startOffset: run.reasoningLength - event.delta.length,
              endOffset: nextLength,
              reasoningLabel: message.modelSnapshot?.protocol === "openAiResponses" ? "summary" : "reasoning",
            });
            return { ...message, agentEvents: events.slice(-256) };
          }
          return {
            ...message,
              agentEvents: (message.agentEvents ?? []).map((item) => item.id === reasoningEventId && item.kind === "reasoning"
                ? { ...item, endOffset: nextLength }
                : item),
          };
        });
        return;
      case "toolTrace":
      case "toolApprovalRequested":
        // 一个 Tool/Approval 事件结束当前 reasoning 片段；后续模型再次输出
        // reasoning 时创建新片段，才能在投影中恢复“思考 → 工具 → 思考”。
        run.reasoningEventId = null;
        updateMessageMetadata(run, (message) => {
          const nextTrace: ToolTrace = {
            ...event.trace,
            approvalId: event.type === "toolApprovalRequested" ? event.approvalId : undefined,
            // 中断种类只在事件上，轨迹本身不持久化它：等待结束后弹窗就该消失。
            interrupt: event.type !== "toolApprovalRequested"
              ? undefined
              : event.kind === "question"
                ? { kind: "question", questions: event.questions }
                : { kind: "approval" },
          };
          const traces = message.toolTraces ?? [];
          const existing = traces.findIndex((trace) => trace.callId === nextTrace.callId);
          return {
            ...message,
            toolTraces: existing < 0
              ? [...traces, nextTrace]
              : traces.map((trace, index) => index === existing ? nextTrace : trace),
            agentEvents: existing < 0
              ? [...(message.agentEvents ?? []), {
                  id: crypto.randomUUID(),
                  sequence: run.nextSequence++,
                  createdAt: Date.now(),
                  kind: "tool" as const,
                  callId: nextTrace.callId,
                }].slice(-256)
              : message.agentEvents,
          };
        });
        return;
      case "skillActivated":
        run.reasoningEventId = null;
        updateMessageMetadata(run, (message) => {
          const hasSnapshot = message.activatedSkills?.some((skill) => skill.id === event.skillId) === true;
          const alreadyRecorded = message.agentEvents?.some((item) => item.kind === "skill" && item.skillId === event.skillId) === true;
          return {
            ...message,
            activatedSkills: hasSnapshot
              ? message.activatedSkills
              : [
                  ...(message.activatedSkills ?? []),
                  {
                    id: event.skillId,
                    name: event.name,
                    version: event.version,
                    contentHash: event.contentHash,
                    activation: "model" as const,
                  },
                ],
            agentEvents: alreadyRecorded
              ? message.agentEvents
              : [...(message.agentEvents ?? []), {
                  id: crypto.randomUUID(),
                  sequence: run.nextSequence++,
                  createdAt: Date.now(),
                  kind: "skill" as const,
                  skillId: event.skillId,
                }].slice(-256),
          };
        });
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
      nextSequence: 1,
      reasoningEventId: null,
      reasoningLength: 0,
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
  }, [finalizeRun, handleEvent, updateMessageMetadata]);

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
