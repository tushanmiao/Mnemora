import { useCallback, useEffect, useRef, useState, type MutableRefObject } from "react";
import {
  cancelChatStream,
  completeChat,
  normalizeModelError,
  startChatStream,
  type ModelStreamEvent,
} from "../api/chat";
import type { AppSettings } from "../../../types/appSettings";
import type {
  ActivatedSkillSnapshot,
  ChatMessage,
  LiteratureReference,
  NoteReference,
} from "../../../types/chat";
import type { ChatAttachment } from "../../../types/attachment";
import type { Conversation } from "../../../types/conversation";
import type { SkillActivationSelection, SkillSummary } from "../../../types/skill";
import type {
  ProviderConfig,
  ProviderModelConfig,
} from "../../../types/modelSettings";
import type { WorkspaceMode } from "../../workspace/types";
import {
  appendStreamingDelta,
  appendStreamingReasoningDelta,
  consumeStreamingMessage,
  resetAllStreamingMessages,
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
import {
  createActivatedSkillSnapshots,
  refreshActivatedSkillSnapshots,
  resolveSkillActivation,
} from "../utils/skillActivation";
import { composeChatSystemPrompt } from "../utils/systemPrompt";

const MAX_TEMPORARY_TITLE_LENGTH = 24;

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
  activatedSkillIds: string[];
  slashSkillId?: string;
};

type UseChatRuntimeOptions = {
  appSettings: AppSettings;
  workspaceMode: WorkspaceMode;
  skills: SkillSummary[];
  currentConversation: Conversation | null;
  currentModel: SelectedModel | null;
  conversationsRef: MutableRefObject<Conversation[]>;
  requestInFlightRef: MutableRefObject<boolean>;
  cacheConversation: (conversation: Conversation, updateSummary?: boolean) => void;
  saveStableConversation: (conversation: Conversation) => void;
  protectConversation: (conversationId: string) => void;
  releaseConversation: (conversationId: string) => void;
};

function createTemporaryTitle(content: string) {
  const characters = Array.from(content.replace(/\s+/g, " ").trim());
  if (characters.length <= MAX_TEMPORARY_TITLE_LENGTH) return characters.join("");
  return `${characters.slice(0, MAX_TEMPORARY_TITLE_LENGTH).join("")}...`;
}

function composeSystemPrompt(settings: AppSettings, conversation: Conversation) {
  return composeChatSystemPrompt({
    globalPrompt: settings.systemPrompt,
    conversationPrompt: conversation.systemPrompt,
    contextSummary: contextSummaryPrompt(conversation),
    responseLanguage: settings.responseLanguage,
  });
}

function createAssistantMessage(
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
    },
    activatedSkills,
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

export function useChatRuntime({
  appSettings,
  workspaceMode,
  skills,
  currentConversation,
  currentModel,
  conversationsRef,
  requestInFlightRef,
  cacheConversation,
  saveStableConversation,
  protectConversation,
  releaseConversation,
}: UseChatRuntimeOptions) {
  const [requestInFlight, setRequestInFlight] = useState(false);
  const [stopRequested, setStopRequested] = useState(false);
  const activeStreamRunRef = useRef<ActiveStreamRun | null>(null);

  useEffect(() => () => {
    const activeRun = activeStreamRunRef.current;
    activeStreamRunRef.current = null;
    requestInFlightRef.current = false;
    resetAllStreamingMessages();
    if (activeRun && !activeRun.terminalReceived) {
      void cancelChatStream(activeRun.runId).catch(() => undefined);
    }
    if (activeRun) releaseConversation(activeRun.conversationId);
  }, [releaseConversation, requestInFlightRef]);

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

  const updateStreamingMessageMetadata = useCallback((
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
      case "toolTrace":
      case "toolApprovalRequested":
        updateStreamingMessageMetadata(run, (message) => {
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
        updateStreamingMessageMetadata(run, (message) => (
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
  }, [finalizeStreamRun, updateStreamingMessageMetadata]);

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
    activatedSkillIds,
    slashSkillId,
  }: PreparedGeneration) => {
    const targetConversationId = initialRunningConversation.id;
    protectConversation(targetConversationId);
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
        activatedSkillIds,
        slashSkillId,
        permissionMode: runningConversation.permissionMode,
        workspaceMode,
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
        const modelActivatedSkills = (response.activatedSkillIds ?? []).flatMap((skillId) => {
          const skill = skills.find((item) => item.id === skillId && item.enabled);
          return skill ? [{
            id: skill.id,
            name: skill.name,
            version: skill.version,
            contentHash: skill.contentHash,
            activation: "model" as const,
          }] : [];
        });
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
                  activatedSkills: [
                    ...(message.activatedSkills ?? []),
                    ...modelActivatedSkills.filter((skill) => (
                      !message.activatedSkills?.some((current) => current.id === skill.id)
                    )),
                  ].slice(0, 3),
                  toolTraces: response.toolTraces,
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
      releaseConversation(targetConversationId);
    }
  }, [
    appSettings,
    cacheConversation,
    conversationsRef,
    finalizeStreamRun,
    handleStreamEvent,
    protectConversation,
    releaseConversation,
    requestInFlightRef,
    saveStableConversation,
    skills,
    workspaceMode,
  ]);

  const sendMessage = useCallback(async (
    rawContent: string,
    attachments: ChatAttachment[] = [],
    skillActivation?: SkillActivationSelection,
    literatureReferences: LiteratureReference[] = [],
    noteReferences: NoteReference[] = [],
  ) => {
    const content = rawContent.trim();
    const targetConversation = currentConversation;
    const selectedModel = currentModel;
    if (
      (!content && attachments.length === 0 && literatureReferences.length === 0 && noteReferences.length === 0)
      || !targetConversation
      || !selectedModel
      || requestInFlightRef.current
    ) return;

    const now = Date.now();
    const userMessage: ChatMessage = {
      id: crypto.randomUUID(),
      conversationId: targetConversation.id,
      role: "user",
      content,
      attachments,
      literatureReferences,
      noteReferences,
      status: "completed",
      createdAt: now,
      updatedAt: now,
    };
    const effectiveActivation = skillActivation ?? {
      skillIds: targetConversation.enabledSkillIds,
    };
    const activatedSkills = createActivatedSkillSnapshots(effectiveActivation, skills);
    const assistantMessage = createAssistantMessage(
      targetConversation.id,
      selectedModel,
      undefined,
      activatedSkills,
    );
    const runningConversation: Conversation = {
      ...targetConversation,
      title: targetConversation.messages.length === 0
        ? createTemporaryTitle(
            content
            || literatureReferences[0]?.title
            || noteReferences[0]?.noteTitle
            || attachments.map((attachment) => attachment.name).join("、"),
          )
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
      activatedSkillIds: activatedSkills.map((skill) => skill.id),
      slashSkillId: effectiveActivation.slashSkillId,
    });
  }, [currentConversation, currentModel, requestInFlightRef, runPreparedGeneration, skills]);

  const regenerateMessage = useCallback(async (messageId: string) => {
    const targetConversation = currentConversation;
    const selectedModel = currentModel;
    if (!targetConversation || !selectedModel || requestInFlightRef.current) return;
    const messageIndex = targetConversation.messages.findIndex((message) => message.id === messageId);
    if (messageIndex < 0 || targetConversation.messages[messageIndex].role !== "assistant") return;
    const history = targetConversation.messages.slice(0, messageIndex);
    if (!history.some((message) => message.role === "user" && (
      message.content.trim()
      || (message.attachments?.length ?? 0) > 0
      || (message.literatureReferences?.length ?? 0) > 0
      || (message.noteReferences?.length ?? 0) > 0
    ))) return;

    const now = Date.now();
    const compressionConversation = resetCompression({
      ...targetConversation,
      messages: history,
      updatedAt: now,
    });
    const originalAssistantMessage = targetConversation.messages[messageIndex];
    const activatedSkills = refreshActivatedSkillSnapshots(
      originalAssistantMessage.activatedSkills ?? [],
      skills,
    );
    const assistantMessage = createAssistantMessage(
      targetConversation.id,
      selectedModel,
      messageId,
      activatedSkills,
    );
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
      activatedSkillIds: activatedSkills.map((skill) => skill.id),
      slashSkillId: activatedSkills.find((skill) => skill.activation === "slash")?.id,
    });
  }, [currentConversation, currentModel, requestInFlightRef, runPreparedGeneration, skills]);

  const editMessage = useCallback(async (messageId: string, rawContent: string) => {
    const content = rawContent.trim();
    const targetConversation = currentConversation;
    if (!targetConversation || requestInFlightRef.current) return;
    const messageIndex = targetConversation.messages.findIndex((message) => message.id === messageId);
    if (messageIndex < 0) return;
    const originalMessage = targetConversation.messages[messageIndex];
    if (
      !content
      && (originalMessage.attachments?.length ?? 0) === 0
      && (originalMessage.literatureReferences?.length ?? 0) === 0
      && (originalMessage.noteReferences?.length ?? 0) === 0
    ) return;
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
      title: messageIndex === 0
        ? createTemporaryTitle(
            content
            || originalMessage.literatureReferences?.[0]?.title
            || originalMessage.noteReferences?.[0]?.noteTitle
            || originalMessage.attachments?.map((attachment) => attachment.name).join("、")
            || targetConversation.title,
          )
        : targetConversation.title,
      messages: history,
      updatedAt: now,
    });
    const selectedModel = currentModel;
    if (!selectedModel) {
      cacheConversation(compressionConversation);
      saveStableConversation(compressionConversation);
      return;
    }

    const activation = resolveSkillActivation(
      content,
      targetConversation.enabledSkillIds,
      skills,
    );
    const activatedSkills = createActivatedSkillSnapshots(activation, skills);
    const assistantMessage = createAssistantMessage(
      targetConversation.id,
      selectedModel,
      undefined,
      activatedSkills,
    );
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
      activatedSkillIds: activatedSkills.map((skill) => skill.id),
      slashSkillId: activation.slashSkillId,
    });
  }, [
    cacheConversation,
    currentConversation,
    currentModel,
    requestInFlightRef,
    runPreparedGeneration,
    saveStableConversation,
    skills,
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

  const compactConversation = useCallback(async (focus = "") => {
    const targetConversation = currentConversation;
    const selectedModel = currentModel;
    if (!targetConversation || !selectedModel) {
      return { executed: false, message: "当前没有可压缩的对话或模型。" };
    }
    if (requestInFlightRef.current) {
      return { executed: false, message: "请先等待当前回复结束。" };
    }
    if (compressionCandidates(targetConversation).length === 0) {
      return { executed: false, message: "当前有效消息太少，暂时不需要压缩。" };
    }

    protectConversation(targetConversation.id);
    requestInFlightRef.current = true;
    setRequestInFlight(true);
    try {
      const compression = await compressConversation(
        appSettings,
        targetConversation,
        selectedModel,
        null,
        { force: true, focus },
      );
      if (!compression?.summary) {
        return { executed: false, message: "模型没有返回可用的压缩摘要。" };
      }
      const current = conversationsRef.current.find((item) => item.id === targetConversation.id);
      if (!current) return { executed: false, message: "对话已切换，未保存压缩结果。" };
      const next: Conversation = {
        ...current,
        contextSummary: compression.summary,
        compressedUntilMessageId: compression.boundaryMessageId,
        contextCompressionCount: current.contextCompressionCount + 1,
        updatedAt: Date.now(),
      };
      cacheConversation(next);
      saveStableConversation(next);
      return { executed: true, message: "上下文已压缩。" };
    } catch (error) {
      const modelError = normalizeModelError(error);
      return { executed: false, message: `压缩失败：${modelError.message}` };
    } finally {
      requestInFlightRef.current = false;
      setRequestInFlight(false);
      releaseConversation(targetConversation.id);
    }
  }, [
    appSettings,
    cacheConversation,
    conversationsRef,
    currentConversation,
    currentModel,
    protectConversation,
    releaseConversation,
    requestInFlightRef,
    saveStableConversation,
  ]);

  return {
    requestInFlight,
    stopRequested,
    sendMessage,
    stopGeneration,
    editMessage,
    regenerateMessage,
    deleteMessage,
    compactConversation,
  };
}
