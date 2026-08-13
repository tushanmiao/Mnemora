import { useCallback, useEffect, useRef, useState } from "react";
import {
  loadStoredConversation,
  saveStoredConversationAsNote,
} from "../../features/conversations/api/conversations";
import {
  saveMessageAsNote,
  summarizeConversationToNote,
} from "../../features/chat/utils/noteGeneration";
import { listLibraryNotes } from "../../features/library/api/library";
import type { Conversation } from "../../types/conversation";
import type { AppSettings } from "../../types/appSettings";
import type { ModelSettings } from "../../types/modelSettings";
import { resolveConversationModel } from "../../types/modelSettings";
import {
  adjustNotePipeline,
  cancelNotePipeline,
  confirmNotePipeline,
  getNotePipelineDetail,
  getNotePipeline,
  listResumableNotePipelines,
  prepareNoteEdit,
  resolveNoteEdit,
  resumeNotePipeline,
  startNotePipeline,
  type NoteEditDialogRequest,
  type NoteEditPrepareResult,
  type NotePipelineEvent,
  type DeepNoteRunDetail,
} from "../../features/chat/api/notePipeline";
import {
  parseDeepNoteOutline,
  selectOutlineSections,
  type DeepNoteOutline,
} from "../../features/chat/notePipeline/outlineSchema";

type NoteFeedback = {
  kind: "progress" | "success" | "error";
  text: string;
};

export type DeepNoteReview = {
  runId: string;
  outline: DeepNoteOutline;
};

function parsePersistedOutline(raw: string): DeepNoteOutline {
  const value: unknown = JSON.parse(raw);
  const messageIds = new Set<string>();
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const sections = (value as { sections?: unknown }).sections;
    if (Array.isArray(sections)) {
      for (const section of sections) {
        if (!section || typeof section !== "object" || Array.isArray(section)) continue;
        const ids = (section as { sourceMessageIds?: unknown }).sourceMessageIds;
        if (Array.isArray(ids)) {
          for (const id of ids) if (typeof id === "string") messageIds.add(id);
        }
      }
    }
  }
  return parseDeepNoteOutline(raw, messageIds);
}

function noteErrorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return String(error);
}

export function useNoteActions({
  currentConversation,
  currentConversationId,
  modelSettings,
  appSettings,
}: {
  currentConversation: Conversation | null;
  currentConversationId: string | null;
  modelSettings: ModelSettings;
  appSettings: AppSettings;
}) {
  const [feedback, setFeedback] = useState<NoteFeedback | null>(null);
  const feedbackTimerRef = useRef<number | null>(null);
  const summaryBusyRef = useRef(false);
  const deepNoteRunRef = useRef<{
    conversationId: string;
    runId: string | null;
    cancelRequested: boolean;
  } | null>(null);
  const recoveryCheckedRef = useRef(false);
  const [deepNoteActive, setDeepNoteActive] = useState(false);
  const [deepNoteReview, setDeepNoteReview] = useState<DeepNoteReview | null>(null);
  const [deepNoteReviewBusy, setDeepNoteReviewBusy] = useState(false);
  const [deepNoteDetail, setDeepNoteDetail] = useState<DeepNoteRunDetail | null>(null);
  const [noteEditRequest, setNoteEditRequest] = useState<NoteEditDialogRequest | null>(null);
  const [noteEditResult, setNoteEditResult] = useState<NoteEditPrepareResult | null>(null);
  const [noteEditBusy, setNoteEditBusy] = useState(false);

  const showFeedback = useCallback((kind: NoteFeedback["kind"], text: string) => {
    if (feedbackTimerRef.current !== null) window.clearTimeout(feedbackTimerRef.current);
    feedbackTimerRef.current = null;
    setFeedback({ kind, text });
    if (kind !== "progress") {
      feedbackTimerRef.current = window.setTimeout(() => setFeedback(null), 4_000);
    }
  }, []);

  useEffect(() => () => {
    if (feedbackTimerRef.current !== null) window.clearTimeout(feedbackTimerRef.current);
  }, []);

  const finishDeepNoteRun = useCallback(() => {
    deepNoteRunRef.current = null;
    setDeepNoteActive(false);
    setDeepNoteReview(null);
    setDeepNoteReviewBusy(false);
    setDeepNoteDetail(null);
  }, []);

  const handleNotePipelineEvent = useCallback((event: NotePipelineEvent) => {
    if (event.type === "progress") {
      showFeedback("progress", event.message);
      return;
    }
    if (event.type === "outlineReady") {
      try {
        deepNoteRunRef.current = {
          conversationId: event.run.conversationId,
          runId: event.run.id,
          cancelRequested: false,
        };
        setDeepNoteActive(true);
        setDeepNoteReview({
          runId: event.run.id,
          outline: parsePersistedOutline(event.run.outlineJson),
        });
        void getNotePipelineDetail(event.run.id).then(setDeepNoteDetail).catch(() => undefined);
        setDeepNoteReviewBusy(false);
        setFeedback(null);
      } catch (error) {
        finishDeepNoteRun();
        showFeedback("error", `读取深度笔记提纲失败：${noteErrorText(error)}`);
      }
      return;
    }
    if (event.type === "done") {
      finishDeepNoteRun();
      if (event.run.warnings.length > 0) {
        showFeedback("success", `已生成深度笔记，有 ${event.run.warnings.length} 项检查提示。`);
      } else {
        showFeedback("success", "已生成深度笔记。");
      }
      return;
    }
    if (event.type === "cancelled") {
      finishDeepNoteRun();
      showFeedback(
        "success",
        event.run.noteId ? "已取消并保存完成章节为草稿。" : "已取消深度笔记生成。",
      );
      return;
    }
    finishDeepNoteRun();
    showFeedback("error", `生成深度笔记失败：${event.message}`);
  }, [finishDeepNoteRun, showFeedback]);

  useEffect(() => {
    if (recoveryCheckedRef.current) return;
    recoveryCheckedRef.current = true;
    let disposed = false;
    void listResumableNotePipelines()
      .then(async (runs) => {
        if (disposed || runs.length === 0 || deepNoteRunRef.current) return;
        const run = runs[0];
        deepNoteRunRef.current = {
          conversationId: run.conversationId,
          runId: run.id,
          cancelRequested: false,
        };
        setDeepNoteActive(true);
        showFeedback("progress", "正在恢复未完成的深度笔记任务…");
        try {
          await resumeNotePipeline(run.id, handleNotePipelineEvent);
        } catch (error) {
          if (!noteErrorText(error).includes("已经在运行")) throw error;
          while (!disposed) {
            await new Promise((resolve) => window.setTimeout(resolve, 1_200));
            if (disposed) return;
            const current = await getNotePipeline(run.id);
            if (current.phase === "awaitingOutline") {
              handleNotePipelineEvent({ type: "outlineReady", run: current });
              return;
            }
            if (current.phase === "done") {
              handleNotePipelineEvent({ type: "done", run: current, degraded: false });
              return;
            }
            if (current.phase === "cancelled") {
              handleNotePipelineEvent({ type: "cancelled", run: current });
              return;
            }
            if (current.phase === "error") {
              handleNotePipelineEvent({
                type: "error",
                runId: current.id,
                message: current.errorMessage ?? "后台深度笔记任务失败。",
              });
              return;
            }
            const completed = current.completedSectionIds.length + current.failedSectionIds.length;
            const total = current.selectedSectionIds.length;
            showFeedback(
              "progress",
              total > 0 ? `后台正在生成深度笔记 ${completed}/${total}…` : "后台正在分析深度笔记…",
            );
          }
        }
      })
      .catch((error) => {
        if (!disposed) showFeedback("error", `恢复深度笔记失败：${noteErrorText(error)}`);
      });
    return () => { disposed = true; };
  }, [handleNotePipelineEvent, showFeedback]);

  const saveConversationAsNote = useCallback((conversationId: string) => {
    void saveStoredConversationAsNote(conversationId)
      .then((note) => showFeedback("success", `已保存为笔记「${note.title}」`))
      .catch((error) => showFeedback("error", `保存笔记失败：${noteErrorText(error)}`));
  }, [showFeedback]);

  const summarizeConversationAsNote = useCallback(async (conversationId: string) => {
    if (summaryBusyRef.current) {
      showFeedback("error", "已有一个总结任务正在进行，请稍候再试。");
      return;
    }
    summaryBusyRef.current = true;
    showFeedback("progress", "正在用模型总结对话…");
    try {
      const conversation = conversationId === currentConversationId && currentConversation
        ? currentConversation
        : await loadStoredConversation(conversationId);
      const model = resolveConversationModel(
        modelSettings,
        conversation.providerId,
        conversation.modelId,
      );
      if (!model) {
        showFeedback("error", "请先在设置中配置可用的默认模型。");
        return;
      }
      const note = await summarizeConversationToNote(conversation, model, {
        maxOutputTokens: appSettings.maxOutputTokens,
        thinkingEnabled: appSettings.thinkingEnabled,
      });
      showFeedback("success", `已生成总结笔记「${note.title}」`);
    } catch (error) {
      showFeedback("error", `总结失败：${noteErrorText(error)}`);
    } finally {
      summaryBusyRef.current = false;
    }
  }, [
    appSettings.maxOutputTokens,
    appSettings.thinkingEnabled,
    currentConversation,
    currentConversationId,
    modelSettings,
    showFeedback,
  ]);

  const startDeepNote = useCallback(async (conversationId: string) => {
    if (deepNoteRunRef.current) {
      showFeedback("error", deepNoteRunRef.current.conversationId === conversationId
        ? "这个对话已有一个深度笔记任务正在进行。"
        : "已有一个深度笔记任务正在进行，请完成或取消后再试。");
      return;
    }
    deepNoteRunRef.current = { conversationId, runId: null, cancelRequested: false };
    setDeepNoteActive(true);
    setDeepNoteDetail(null);
    showFeedback("progress", "正在启动深度笔记分析…");
    try {
      const run = await startNotePipeline(conversationId, handleNotePipelineEvent);
      const active = deepNoteRunRef.current;
      if (active) {
        active.runId = run.id;
        void getNotePipelineDetail(run.id).then(setDeepNoteDetail).catch(() => undefined);
        if (active.cancelRequested) await cancelNotePipeline(run.id);
      }
    } catch (error) {
      finishDeepNoteRun();
      showFeedback("error", `生成深度笔记失败：${noteErrorText(error)}`);
    }
  }, [
    finishDeepNoteRun,
    handleNotePipelineEvent,
    showFeedback,
  ]);

  const adjustDeepNoteOutline = useCallback(async (requirement: string) => {
    if (!deepNoteReview || deepNoteReviewBusy) return;
    setDeepNoteReviewBusy(true);
    showFeedback("progress", "正在按补充要求调整提纲…");
    try {
      await adjustNotePipeline(deepNoteReview.runId, requirement.trim(), handleNotePipelineEvent);
    } catch (error) {
      showFeedback("error", `调整提纲失败：${noteErrorText(error)}`);
      setDeepNoteReviewBusy(false);
    }
  }, [deepNoteReview, deepNoteReviewBusy, handleNotePipelineEvent, showFeedback]);

  const confirmDeepNoteOutline = useCallback(async (selectedSectionIds: ReadonlySet<string>) => {
    if (!deepNoteReview || deepNoteReviewBusy) return;
    let outline: DeepNoteOutline;
    try {
      outline = selectOutlineSections(deepNoteReview.outline, selectedSectionIds);
    } catch (error) {
      showFeedback("error", noteErrorText(error));
      return;
    }
    setDeepNoteReviewBusy(true);
    setDeepNoteReview(null);
    showFeedback("progress", `正在扩写 0/${outline.sections.length}…`);
    try {
      await confirmNotePipeline(
        deepNoteReview.runId,
        outline.sections.map((section) => section.id),
        handleNotePipelineEvent,
      );
    } catch (error) {
      finishDeepNoteRun();
      showFeedback("error", `生成深度笔记失败：${noteErrorText(error)}`);
    }
  }, [deepNoteReview, deepNoteReviewBusy, finishDeepNoteRun, handleNotePipelineEvent, showFeedback]);

  const cancelDeepNote = useCallback(() => {
    const run = deepNoteRunRef.current;
    if (!run) return;
    if (!run.runId) {
      run.cancelRequested = true;
      showFeedback("progress", "正在等待任务启动后取消…");
      return;
    }
    void cancelNotePipeline(run.runId).catch((error) => {
      showFeedback("error", `取消深度笔记失败：${noteErrorText(error)}`);
    });
    if (deepNoteReview) {
      finishDeepNoteRun();
      showFeedback("success", "已取消深度笔记生成。");
    } else {
      showFeedback("progress", "正在停止；当前请求结束后会保存已完成章节为草稿…");
    }
  }, [deepNoteReview, finishDeepNoteRun, showFeedback]);

  const openConversationNoteEdit = useCallback(async (conversationId: string) => {
    if (noteEditBusy) return;
    setNoteEditBusy(true);
    try {
      const notes = await listLibraryNotes();
      if (notes.length === 0) throw new Error("还没有可更新的笔记。");
      setNoteEditRequest({
        conversationId,
        notes,
        noteId: null,
        selectedText: "",
        sectionHeading: "",
      });
    } catch (error) {
      showFeedback("error", `读取笔记失败：${noteErrorText(error)}`);
    } finally {
      setNoteEditBusy(false);
    }
  }, [noteEditBusy, showFeedback]);

  const openSelectionNoteEdit = useCallback(async ({
    noteId,
    selectedText,
    sectionHeading,
  }: {
    noteId: string;
    selectedText: string;
    sectionHeading: string;
  }) => {
    if (!currentConversationId) {
      showFeedback("error", "请先打开一个对话，再使用 AI 修改选中文本。");
      return;
    }
    setNoteEditBusy(true);
    try {
      const notes = await listLibraryNotes();
      setNoteEditRequest({
        conversationId: currentConversationId,
        notes,
        noteId,
        selectedText,
        sectionHeading,
      });
    } catch (error) {
      showFeedback("error", `读取笔记失败：${noteErrorText(error)}`);
    } finally {
      setNoteEditBusy(false);
    }
  }, [currentConversationId, showFeedback]);

  const prepareExistingNoteEdit = useCallback(async (noteId: string, requirement: string) => {
    if (!noteEditRequest || noteEditBusy) return;
    setNoteEditBusy(true);
    showFeedback("progress", "正在分析增量并生成笔记补丁…");
    try {
      const result = await prepareNoteEdit({
        noteId,
        conversationId: noteEditRequest.conversationId,
        selectedText: noteEditRequest.selectedText,
        sectionHeading: noteEditRequest.sectionHeading,
        requirement: requirement.trim(),
      });
      setNoteEditRequest(null);
      setNoteEditResult(result);
      setFeedback(null);
    } catch (error) {
      showFeedback("error", `生成笔记修改失败：${noteErrorText(error)}`);
    } finally {
      setNoteEditBusy(false);
    }
  }, [noteEditBusy, noteEditRequest, showFeedback]);

  const closeNoteEdit = useCallback(async () => {
    const proposal = noteEditResult?.proposal;
    setNoteEditRequest(null);
    setNoteEditResult(null);
    if (!proposal) return;
    try {
      await resolveNoteEdit(proposal.id, false);
    } catch (error) {
      showFeedback("error", `放弃修改失败：${noteErrorText(error)}`);
    }
  }, [noteEditResult, showFeedback]);

  const applyNoteEdit = useCallback(async () => {
    if (!noteEditResult || noteEditBusy) return;
    setNoteEditBusy(true);
    try {
      const updated = await resolveNoteEdit(noteEditResult.proposal.id, true);
      if (!updated) throw new Error("修改提案已失效。");
      setNoteEditResult(null);
      showFeedback("success", `已更新笔记「${updated.title}」，旧版本已自动备份。`);
    } catch (error) {
      showFeedback("error", `应用笔记修改失败：${noteErrorText(error)}`);
    } finally {
      setNoteEditBusy(false);
    }
  }, [noteEditBusy, noteEditResult, showFeedback]);

  const saveMessage = useCallback(async (messageId: string) => {
    if (!currentConversation) return false;
    try {
      const note = await saveMessageAsNote(currentConversation, messageId);
      showFeedback("success", `已保存为笔记「${note.title}」`);
      return true;
    } catch (error) {
      showFeedback("error", `保存笔记失败：${noteErrorText(error)}`);
      return false;
    }
  }, [currentConversation, showFeedback]);

  return {
    feedback,
    deepNoteActive,
    deepNoteReview,
    deepNoteReviewBusy,
    deepNoteDetail,
    noteEditRequest,
    noteEditResult,
    noteEditBusy,
    saveConversationAsNote,
    summarizeConversationAsNote,
    startDeepNote,
    adjustDeepNoteOutline,
    confirmDeepNoteOutline,
    cancelDeepNote,
    openConversationNoteEdit,
    openSelectionNoteEdit,
    prepareExistingNoteEdit,
    closeNoteEdit,
    applyNoteEdit,
    saveMessage,
  };
}
