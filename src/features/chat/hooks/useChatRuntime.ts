import { useCallback, useState, type MutableRefObject } from "react";
import {
  completeChat,
  normalizeModelError,
} from "../api/chat";
import type { AppSettings } from "../../../types/appSettings";
import type {
  ChatMessage,
  LiteratureReference,
  NoteReference,
} from "../../../types/chat";
import type { ChatAttachment } from "../../../types/attachment";
import type { Conversation } from "../../../types/conversation";
import type { SkillActivationSelection, SkillSummary } from "../../../types/skill";
import type { WorkspaceMode } from "../../workspace/types";
import type { ActiveWorkNoteContext } from "../../workspace/types";
import {
  activeContextMessages,
  compressionCandidates,
  toModelMessages,
} from "../utils/contextCompression";
import {
  createActivatedSkillSnapshots,
  refreshActivatedSkillSnapshots,
  resolveSkillActivation,
} from "../utils/skillActivation";
import {
  compressConversation,
  composeSystemPrompt,
  createAssistantMessage,
  createTemporaryTitle,
  resetCompression,
  type SelectedModel,
} from "../runtime/generationHelpers";
import { useStreamingRun } from "./useStreamingRun";
import { workflowSummaryForMessage } from "../agent/projections/workflowProjection";

export type { SelectedModel } from "../runtime/generationHelpers";

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
  activeWorkNoteContext?: ActiveWorkNoteContext | null;
};

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
  activeWorkNoteContext = null,
}: UseChatRuntimeOptions) {
  const [requestInFlight, setRequestInFlight] = useState(false);
  const streaming = useStreamingRun({
    conversationsRef,
    cacheConversation,
    saveStableConversation,
    releaseConversation,
  });

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
      streaming.prepareStreamingMessage(assistantMessageId);
    }
    requestInFlightRef.current = true;
    setRequestInFlight(true);
    streaming.resetStopRequested();
    let runningConversation = initialRunningConversation;
    cacheConversation(runningConversation);
    saveStableConversation({
      ...runningConversation,
      messages: runningConversation.messages.filter((message) => message.id !== assistantMessageId),
    });

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
      // 总览只是聚合页面，不会发起 Chat 请求；若运行时合同被复用，按普通 Chat 处理。
      const completionWorkspaceMode = workspaceMode === "overview"
        || workspaceMode === "english"
        || workspaceMode === "deepNote"
        ? "chat"
        : workspaceMode;
      const completionRequest = {
        providerId: selectedModel.provider.id,
        modelId: selectedModel.model.id,
        conversationId: targetConversationId,
        messageId: assistantMessageId,
        systemPrompt: composeSystemPrompt(appSettings, runningConversation),
        activatedSkillIds,
        slashSkillId,
        permissionMode: runningConversation.permissionMode,
        workspaceMode: completionWorkspaceMode,
        workspaceContext: completionWorkspaceMode === "work" && activeWorkNoteContext
          ? {
              kind: "note" as const,
              noteId: activeWorkNoteContext.noteId,
              noteTitle: activeWorkNoteContext.noteTitle,
              noteRevisionHash: activeWorkNoteContext.revisionHash,
              noteSnapshot: activeWorkNoteContext.noteSnapshot,
              sourcePdfId: activeWorkNoteContext.source?.sourcePdfId,
              sourcePdfTitle: activeWorkNoteContext.source?.sourcePdfTitle,
              sourcePageIndex: activeWorkNoteContext.source?.sourcePageIndex ?? undefined,
            }
          : undefined,
        messages: modelMessages,
        options: {
          maxOutputTokens: appSettings.maxOutputTokens,
          thinkingEnabled: runningConversation.thinkingEnabled ?? appSettings.thinkingEnabled,
          reasoningEffort: runningConversation.reasoningEffort ?? undefined,
        },
      };

      if (appSettings.streamEnabled) {
        await streaming.runStream(targetConversationId, assistantMessageId, completionRequest);
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
          messages: conversation.messages.map((message) => {
            if (message.id !== assistantMessageId) return message;
            const completedMessage: ChatMessage = {
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
                  ].slice(0, 12),
                  toolTraces: response.toolTraces,
                  agentEvents: buildResponseAgentEvents(
                    assistantMessageId,
                    response.reasoning ?? "",
                    (message.activatedSkills ?? []).filter((skill) => (
                      completionRequest.activatedSkillIds?.includes(skill.id)
                    )),
                    modelActivatedSkills,
                    response.toolTraces ?? [],
                    completedAt,
                  ),
                  updatedAt: completedAt,
                };
            return {
              ...completedMessage,
              workflowSummary: workflowSummaryForMessage(completedMessage),
            };
          }),
          updatedAt: completedAt,
        };
        cacheConversation(completedConversation);
        saveStableConversation(completedConversation);
      }
    } catch (error) {
      const modelError = normalizeModelError(error);
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
      requestInFlightRef.current = false;
      setRequestInFlight(false);
      streaming.resetStopRequested();
      releaseConversation(targetConversationId);
    }
  }, [
    appSettings,
    activeWorkNoteContext,
    cacheConversation,
    conversationsRef,
    protectConversation,
    releaseConversation,
    requestInFlightRef,
    saveStableConversation,
    skills,
    streaming,
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
      skillIds: [],
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
    stopRequested: streaming.stopRequested,
    sendMessage,
    stopGeneration: streaming.stopGeneration,
    editMessage,
    regenerateMessage,
    deleteMessage,
    compactConversation,
  };
}

function buildResponseAgentEvents(
  messageId: string,
  reasoning: string,
  manualSkills: ChatMessage["activatedSkills"],
  modelSkills: ChatMessage["activatedSkills"],
  tools: ChatMessage["toolTraces"],
  createdAt: number,
): ChatMessage["agentEvents"] {
  const events: NonNullable<ChatMessage["agentEvents"]> = [];
  let sequence = 1;
  for (const skill of manualSkills ?? []) {
    events.push({ id: `${messageId}:skill:${skill.id}`, sequence: sequence++, createdAt, kind: "skill", skillId: skill.id });
  }
  if (reasoning.trim()) {
    events.push({
      id: `${messageId}:reasoning`, sequence: sequence++, createdAt, kind: "reasoning",
      startOffset: 0, endOffset: reasoning.length, reasoningLabel: "reasoning",
    });
  }
  for (const skill of modelSkills ?? []) {
    events.push({ id: `${messageId}:skill:${skill.id}`, sequence: sequence++, createdAt, kind: "skill", skillId: skill.id });
  }
  for (const tool of tools ?? []) {
    events.push({ id: `${messageId}:tool:${tool.callId}`, sequence: sequence++, createdAt, kind: "tool", callId: tool.callId });
  }
  return events;
}
