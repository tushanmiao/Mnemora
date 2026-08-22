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
  abandonNotePipeline,
  abandonNotePipelinesForConversation,
  cancelNotePipeline,
  confirmNotePipeline,
  getNotePipelineDetail,
  getNotePipeline,
  inspectNotePipelineStart,
  listResumableNotePipelines,
  pauseNotePipeline,
  prepareNoteEdit,
  resolveNoteEdit,
  resolveNoteEditContent,
  restartNotePipeline,
  resumeNotePipeline,
  retryNotePipeline,
  startNotePipeline,
  type NoteEditDialogRequest,
  type NoteEditPrepareResult,
  type NotePipelineEvent,
  type NotePipelineRun,
  type DeepNoteRunDetail,
} from "../../features/chat/api/notePipeline";
import {
  parseDeepNoteOutline,
  selectOutlineSections,
  type DeepNoteOutline,
} from "../../features/chat/notePipeline/outlineSchema";
import { discardLocalNoteSource, prepareLocalNoteSource } from "../../features/notes/api/localNoteSource";
import type { DeepNoteProgress } from "../../features/workspace/runtime/DeepNoteViewRuntime";

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

function recoverableRunProgress(run: NotePipelineRun): Omit<DeepNoteProgress, "updatedAt"> {
  const completed = run.completedSectionIds.length + run.failedSectionIds.length;
  const total = run.selectedSectionIds.length || null;
  if (run.phase === "paused") {
    return {
      runId: run.id, phase: run.phase, current: completed, total,
      message: "任务保持暂停。已完成章节已保存，点击继续后从断点恢复。",
      terminal: false, degraded: false, activity: null,
    };
  }
  if (run.phase === "cancelling") {
    return {
      runId: run.id, phase: run.phase, current: completed, total,
      message: "停止请求已发送；若后台任务未及时退出，系统会自动强制终止。",
      terminal: false, degraded: false, activity: null,
    };
  }
  if (run.phase === "cancelled") {
    return {
      runId: run.id, phase: run.phase, current: completed, total,
      message: "任务已停止，检查点仍然保留。可以从检查点继续或重新生成。",
      terminal: true, degraded: false, activity: null,
    };
  }
  if (run.phase === "error" || run.phase === "blocked") {
    return {
      runId: run.id, phase: run.phase, current: completed, total,
      message: run.errorMessage ?? "任务未能完成，可以从失败步骤重试或重新生成。",
      terminal: run.phase === "error", degraded: false, activity: null,
    };
  }
  return {
    runId: run.id, phase: run.phase, current: completed, total,
    message: run.phase === "awaitingOutline"
      ? "计划已生成，等待确认后开始执行。"
      : "已重新连接到未完成的深度笔记任务。",
    terminal: false, degraded: false, activity: null,
  };
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
  const deepNotePausedRunIdRef = useRef<string | null>(null);
  const [deepNoteActive, setDeepNoteActive] = useState(false);
  const [deepNoteReview, setDeepNoteReview] = useState<DeepNoteReview | null>(null);
  const [deepNoteReviewBusy, setDeepNoteReviewBusy] = useState(false);
  const [deepNoteDetail, setDeepNoteDetail] = useState<DeepNoteRunDetail | null>(null);
  const [deepNoteProgress, setDeepNoteProgress] = useState<DeepNoteProgress | null>(null);
  const [deepNoteControlBusy, setDeepNoteControlBusy] = useState(false);
  const detailRequestSequenceRef = useRef(0);
  const detailRefreshTimerRef = useRef<number | null>(null);
  const latestDeepNoteRunIdRef = useRef<string | null>(null);
  // 取消/遗弃后，通道仍可能把已经排队的终态事件送到前端。记录这些 run id，
  // 防止旧事件把已经关闭的任务重新渲染出来。
  const ignoredDeepNoteRunIdsRef = useRef<Set<string>>(new Set());
  const cancellingDeepNoteRunIdsRef = useRef<Set<string>>(new Set());
  const [noteEditRequest, setNoteEditRequest] = useState<NoteEditDialogRequest | null>(null);
  const [noteEditResult, setNoteEditResult] = useState<NoteEditPrepareResult | null>(null);
  const [noteEditBusy, setNoteEditBusy] = useState(false);
  const [noteEditRefresh, setNoteEditRefresh] = useState<{ noteId: string; version: number } | null>(null);

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
    if (detailRefreshTimerRef.current !== null) window.clearTimeout(detailRefreshTimerRef.current);
  }, []);

  const refreshDeepNoteDetail = useCallback((runId: string, immediate = false) => {
    const load = () => {
      detailRefreshTimerRef.current = null;
      const sequence = ++detailRequestSequenceRef.current;
      void getNotePipelineDetail(runId)
        .then((detail) => {
          if (sequence === detailRequestSequenceRef.current) setDeepNoteDetail(detail);
        })
        .catch(() => undefined);
    };
    if (immediate) {
      if (detailRefreshTimerRef.current !== null) window.clearTimeout(detailRefreshTimerRef.current);
      load();
      return;
    }
    if (detailRefreshTimerRef.current === null) {
      detailRefreshTimerRef.current = window.setTimeout(load, 250);
    }
  }, []);

  const setProgress = useCallback((value: Omit<DeepNoteProgress, "updatedAt">) => {
    setDeepNoteProgress({ ...value, updatedAt: Date.now() });
  }, []);

  const finishDeepNoteRun = useCallback(() => {
    cancellingDeepNoteRunIdsRef.current.clear();
    deepNoteRunRef.current = null;
    deepNotePausedRunIdRef.current = null;
    setDeepNoteActive(false);
    setDeepNoteReview(null);
    setDeepNoteReviewBusy(false);
    setDeepNoteControlBusy(false);
  }, []);

  const handleNotePipelineEvent = useCallback((event: NotePipelineEvent) => {
    const eventRunId = event.type === "progress" || event.type === "error"
      ? event.runId
      : event.run.id;
    if (ignoredDeepNoteRunIdsRef.current.has(eventRunId)) return;
    if (latestDeepNoteRunIdRef.current && latestDeepNoteRunIdRef.current !== eventRunId) return;
    if (
      cancellingDeepNoteRunIdsRef.current.has(eventRunId)
      && (event.type === "progress" || event.type === "outlineReady" || event.type === "paused")
    ) return;
    latestDeepNoteRunIdRef.current = eventRunId;
    if (event.type === "progress") {
      if (deepNotePausedRunIdRef.current === event.runId) return;
      setProgress({
        runId: event.runId,
        phase: event.phase,
        current: event.current,
        total: event.total,
        message: event.message,
        terminal: false,
        degraded: false,
        activity: event.activity,
      });
      refreshDeepNoteDetail(event.runId);
      showFeedback("progress", event.message);
      return;
    }
    if (event.type === "outlineReady") {
      try {
        deepNotePausedRunIdRef.current = null;
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
        setProgress({
          runId: event.run.id,
          phase: event.run.phase,
          current: null,
          total: event.run.selectedSectionIds.length || null,
          message: "计划已生成，等待确认后开始执行。",
          terminal: false,
          degraded: false,
        });
        refreshDeepNoteDetail(event.run.id, true);
        setDeepNoteReviewBusy(false);
        setFeedback(null);
      } catch (error) {
        finishDeepNoteRun();
        showFeedback("error", `读取深度笔记提纲失败：${noteErrorText(error)}`);
      }
      return;
    }
    if (event.type === "paused") {
      deepNotePausedRunIdRef.current = event.run.id;
      deepNoteRunRef.current = {
        conversationId: event.run.conversationId,
        runId: event.run.id,
        cancelRequested: false,
      };
      setDeepNoteActive(true);
      setProgress({
        runId: event.run.id,
        phase: "paused",
        current: event.run.completedSectionIds.length + event.run.failedSectionIds.length,
        total: event.run.selectedSectionIds.length || null,
        message: "任务已暂停。已完成章节和运行预算已保存，可以稍后继续。",
        terminal: false,
        degraded: false,
        activity: null,
      });
      setDeepNoteControlBusy(false);
      refreshDeepNoteDetail(event.run.id, true);
      showFeedback("success", "深度笔记已暂停。");
      return;
    }
    if (event.type === "done") {
      if (cancellingDeepNoteRunIdsRef.current.has(event.run.id)) return;
      cancellingDeepNoteRunIdsRef.current.delete(event.run.id);
      setProgress({
        runId: event.run.id,
        phase: event.run.phase,
        current: event.run.completedSectionIds.length,
        total: event.run.selectedSectionIds.length || null,
        message: event.degraded ? "已取消并保存已完成章节为草稿。" : "深度笔记已生成完成。",
        terminal: true,
        degraded: event.degraded,
      });
      refreshDeepNoteDetail(event.run.id, true);
      finishDeepNoteRun();
      if (event.run.warnings.length > 0) {
        showFeedback("success", `已生成深度笔记，有 ${event.run.warnings.length} 项检查提示。`);
      } else {
        showFeedback("success", "已生成深度笔记。");
      }
      return;
    }
    if (event.type === "cancelled") {
      cancellingDeepNoteRunIdsRef.current.delete(event.run.id);
      deepNotePausedRunIdRef.current = null;
      const existing = deepNoteRunRef.current;
      deepNoteRunRef.current = {
        conversationId: existing?.conversationId ?? deepNoteDetail?.run.conversationId ?? "",
        runId: event.run.id,
        cancelRequested: false,
      };
      setDeepNoteActive(true);
      setProgress({
        runId: event.run.id,
        phase: event.run.phase,
        current: event.run.completedSectionIds.length,
        total: event.run.selectedSectionIds.length || null,
        message: event.run.noteId ? "任务已取消，已完成章节已保存为草稿。" : "深度笔记任务已取消。",
        terminal: true,
        degraded: Boolean(event.run.noteId),
      });
      refreshDeepNoteDetail(event.run.id, true);
      setDeepNoteControlBusy(false);
      showFeedback(
        "success",
        event.run.noteId ? "已取消并保存完成章节为草稿。" : "已取消深度笔记生成。",
      );
      return;
    }
    cancellingDeepNoteRunIdsRef.current.delete(event.runId);
    setProgress({
      runId: event.runId,
      phase: "error",
      current: null,
      total: null,
      message: event.message,
      terminal: true,
      degraded: false,
    });
    const existing = deepNoteRunRef.current;
    if (existing) existing.runId = event.runId;
    setDeepNoteActive(true);
    setDeepNoteControlBusy(false);
    refreshDeepNoteDetail(event.runId, true);
    showFeedback("error", `生成深度笔记失败：${event.message}`);
  }, [deepNoteDetail?.run.conversationId, finishDeepNoteRun, refreshDeepNoteDetail, setProgress, showFeedback]);

  useEffect(() => {
    if (recoveryCheckedRef.current) return;
    recoveryCheckedRef.current = true;
    let disposed = false;
    void listResumableNotePipelines()
      .then(async (runs) => {
        if (disposed || runs.length === 0 || deepNoteRunRef.current) return;
        const run = runs[0];
        latestDeepNoteRunIdRef.current = run.id;
        deepNoteRunRef.current = {
          conversationId: run.conversationId,
          runId: run.id,
          cancelRequested: false,
        };
        setDeepNoteActive(true);
        if (run.phase === "paused") {
          deepNotePausedRunIdRef.current = run.id;
          setProgress({
            runId: run.id,
            phase: "paused",
            current: run.completedSectionIds.length + run.failedSectionIds.length,
            total: run.selectedSectionIds.length || null,
            message: "任务保持暂停。已完成章节已保存，点击继续后从断点恢复。",
            terminal: false,
            degraded: false,
            activity: null,
          });
          refreshDeepNoteDetail(run.id, true);
          setFeedback(null);
          return;
        }
        if (run.phase === "cancelled") {
          setProgress(recoverableRunProgress(run));
          refreshDeepNoteDetail(run.id, true);
          setFeedback(null);
          return;
        }
        if (run.phase === "error" || run.phase === "blocked") {
          setProgress({
            runId: run.id,
            phase: run.phase,
            current: run.completedSectionIds.length + run.failedSectionIds.length,
            total: run.selectedSectionIds.length || null,
            message: run.errorMessage ?? "任务未能完成，可以从失败步骤重试或重新生成。",
            terminal: run.phase === "error",
            degraded: false,
            activity: null,
          });
          refreshDeepNoteDetail(run.id, true);
          setFeedback(null);
          return;
        }
        setProgress({
          runId: run.id,
          phase: run.phase,
          current: run.completedSectionIds.length + run.failedSectionIds.length,
          total: run.selectedSectionIds.length || null,
          message: "正在恢复未完成的深度笔记任务…",
          terminal: false,
          degraded: false,
        });
        refreshDeepNoteDetail(run.id, true);
        showFeedback("progress", "正在恢复未完成的深度笔记任务…");
        try {
          await resumeNotePipeline(run.id, handleNotePipelineEvent);
        } catch (error) {
          if (!noteErrorText(error).includes("已经在运行")) throw error;
          while (!disposed) {
            await new Promise((resolve) => window.setTimeout(resolve, 1_200));
            if (disposed) return;
            const current = await getNotePipeline(run.id);
            refreshDeepNoteDetail(run.id);
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
            setProgress({
              runId: current.id,
              phase: current.phase,
              current: completed,
              total: total || null,
              message: total > 0
                ? `后台正在生成深度笔记 ${completed}/${total}…`
                : "后台正在分析深度笔记…",
              terminal: false,
              degraded: false,
            });
            showFeedback(
              "progress",
              total > 0 ? `后台正在生成深度笔记 ${completed}/${total}…` : "后台正在分析深度笔记…",
            );
          }
        }
      })
      .catch((error) => {
        if (!disposed) {
          const runId = latestDeepNoteRunIdRef.current;
          setProgress({
            runId,
            phase: "error",
            current: null,
            total: null,
            message: noteErrorText(error),
            terminal: true,
            degraded: false,
          });
          if (runId) {
            setDeepNoteActive(true);
            refreshDeepNoteDetail(runId, true);
          }
        }
        if (!disposed) showFeedback("error", `恢复深度笔记失败：${noteErrorText(error)}`);
      });
    return () => { disposed = true; };
  }, [handleNotePipelineEvent, refreshDeepNoteDetail, setProgress, showFeedback]);

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
      return false;
    }
    let inspection;
    try {
      inspection = await inspectNotePipelineStart(conversationId);
    } catch (error) {
      showFeedback("error", `检查已有深度笔记失败：${noteErrorText(error)}`);
      return false;
    }
    if (inspection.unsupportedAttachmentNames.length > 0) {
      showFeedback(
        "error",
        `深度笔记无法安全读取这些附件：${inspection.unsupportedAttachmentNames.join("、")}。请先转换为当前支持的文本、PDF、DOCX、XLSX 或图片格式。`,
      );
      return false;
    }
    if (inspection.status === "upToDate") {
      showFeedback("success", inspection.message);
      return false;
    }
    let replaceInvalidated = false;
    let forceRebuild = false;
    if (inspection.status === "invalidated") {
      replaceInvalidated = window.confirm(
        `${inspection.message}\n\n是否基于当前对话重新生成一份新的深度笔记？原笔记会保留。`,
      );
      if (!replaceInvalidated) {
        showFeedback("error", "已有笔记的覆盖快照已失效，未启动重新生成。");
        return false;
      }
    }
    if (inspection.status === "updateAvailable" && inspection.noteId) {
      if (inspection.requiresFullRebuild || inspection.newAttachmentCount > 0) {
        const shouldUpdate = window.confirm(
          inspection.message + "\n\n现在可以只读取新增附件并生成增量更新提案。是否继续？",
        );
        if (!shouldUpdate) {
          showFeedback("success", "已保留现有笔记，没有读取新增附件。");
          return false;
        }
        setNoteEditBusy(true);
        showFeedback("progress", "正在读取新增附件并生成增量更新提案…");
        try {
          const result = await prepareNoteEdit({
            noteId: inspection.noteId,
            conversationId,
            selectedText: "",
            sectionHeading: "",
            requirement: "只合入覆盖锚点之后的新消息和新增附件；必须使用本地 Reader/Vision 的真实 Source Chunk，保留原笔记无关内容。",
            operationId: crypto.randomUUID(),
          });
          setNoteEditRequest(null);
          setNoteEditResult(result);
          setFeedback(null);
        } catch (error) {
          showFeedback("error", "生成附件增量更新提案失败：" + noteErrorText(error));
        } finally {
          setNoteEditBusy(false);
        }
        return true;
      } else {
        const shouldUpdate = window.confirm(
          `${inspection.message}\n\n已有笔记「${inspection.noteTitle ?? "未命名"}」。是否只合入新增消息并更新这份笔记？`,
        );
        if (!shouldUpdate) {
          showFeedback("success", "已保留现有笔记，没有启动新的生成任务。");
          return false;
        }
        setNoteEditBusy(true);
        showFeedback("progress", "正在只用新增消息生成增量更新提案…");
        try {
          const result = await prepareNoteEdit({
            noteId: inspection.noteId,
            conversationId,
            selectedText: "",
            sectionHeading: "",
            requirement: "只使用已有深度笔记覆盖锚点之后新增的对话消息，保留原有内容；生成增量合并提案，不要引入新增消息之外的来源。",
            operationId: crypto.randomUUID(),
          });
          setNoteEditRequest(null);
          setNoteEditResult(result);
          setFeedback(null);
        } catch (error) {
          showFeedback("error", `生成增量更新提案失败：${noteErrorText(error)}`);
        } finally {
          setNoteEditBusy(false);
        }
        return true;
      }
    }
    deepNoteRunRef.current = { conversationId, runId: null, cancelRequested: false };
    deepNotePausedRunIdRef.current = null;
    latestDeepNoteRunIdRef.current = null;
    setDeepNoteControlBusy(false);
    setDeepNoteActive(true);
    if (detailRefreshTimerRef.current !== null) window.clearTimeout(detailRefreshTimerRef.current);
    detailRefreshTimerRef.current = null;
    detailRequestSequenceRef.current += 1;
    setDeepNoteDetail(null);
    setProgress({
      runId: null,
      phase: "preflight",
      current: null,
      total: null,
      message: "正在启动深度笔记分析…",
      terminal: false,
      degraded: false,
    });
    showFeedback("progress", "正在启动深度笔记分析…");
    try {
      const run = await startNotePipeline(
        conversationId,
        handleNotePipelineEvent,
        replaceInvalidated,
        forceRebuild,
      );
      const active = deepNoteRunRef.current;
      if (active) {
        active.runId = run.id;
        refreshDeepNoteDetail(run.id, true);
        if (active.cancelRequested) await cancelNotePipeline(run.id);
      }
      return true;
    } catch (error) {
      const message = noteErrorText(error);
      try {
        const runs = await listResumableNotePipelines();
        const recoverable = runs.find((run) => run.conversationId === conversationId);
        if (recoverable) {
          latestDeepNoteRunIdRef.current = recoverable.id;
          deepNoteRunRef.current = {
            conversationId: recoverable.conversationId,
            runId: recoverable.id,
            cancelRequested: false,
          };
          deepNotePausedRunIdRef.current = recoverable.phase === "paused" ? recoverable.id : null;
          setDeepNoteActive(true);
          setProgress(recoverableRunProgress(recoverable));
          if (recoverable.phase === "awaitingOutline" && recoverable.outlineJson) {
            try {
              setDeepNoteReview({
                runId: recoverable.id,
                outline: parsePersistedOutline(recoverable.outlineJson),
              });
            } catch {
              setDeepNoteReview(null);
            }
          }
          refreshDeepNoteDetail(recoverable.id, true);
          showFeedback("success", "已找回这个对话中保存的深度笔记任务。可继续处理，无需重新启动。");
          return true;
        }
      } catch {
        // Keep the original start error when recovery discovery is also unavailable.
      }
      setProgress({
        runId: deepNoteRunRef.current?.runId ?? null,
        phase: "error",
        current: null,
        total: null,
        message,
        terminal: true,
        degraded: false,
      });
      finishDeepNoteRun();
      showFeedback("error", `生成深度笔记失败：${message}`);
      return false;
    }
  }, [
    finishDeepNoteRun,
    handleNotePipelineEvent,
    refreshDeepNoteDetail,
    setProgress,
    showFeedback,
  ]);

  const startLocalFilesDeepNote = useCallback(async (paths: string[]) => {
    if (deepNoteRunRef.current) {
      showFeedback("error", "已有一个深度笔记任务正在进行，请完成或取消后再试。");
      return false;
    }
    showFeedback("progress", "正在安全复制本地文件并创建来源快照…");
    let source: Awaited<ReturnType<typeof prepareLocalNoteSource>>;
    try {
      source = await prepareLocalNoteSource(paths);
    } catch (error) {
      showFeedback("error", `准备本地文件失败：${noteErrorText(error)}`);
      return false;
    }
    const started = await startDeepNote(source.conversationId);
    if (!started) await discardLocalNoteSource(source.conversationId).catch(() => undefined);
    return started;
  }, [showFeedback, startDeepNote]);

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
      setProgress({
        runId: deepNoteReview.runId,
        phase: "error",
        current: 0,
        total: outline.sections.length,
        message: noteErrorText(error),
        terminal: true,
        degraded: false,
      });
      refreshDeepNoteDetail(deepNoteReview.runId, true);
      setDeepNoteControlBusy(false);
      showFeedback("error", `生成深度笔记失败：${noteErrorText(error)}`);
    }
  }, [
    deepNoteReview,
    deepNoteReviewBusy,
    handleNotePipelineEvent,
    refreshDeepNoteDetail,
    setProgress,
    showFeedback,
  ]);

  const pauseDeepNote = useCallback(async () => {
    const run = deepNoteRunRef.current;
    if (!run?.runId || deepNoteControlBusy) return;
    if (deepNoteProgress?.phase === "paused") return;
    const runId = run.runId;
    deepNotePausedRunIdRef.current = runId;
    setDeepNoteControlBusy(true);
    setDeepNoteProgress((current) => ({
      runId,
      phase: current?.phase ?? "drafting",
      current: current?.current ?? null,
      total: current?.total ?? null,
      message: "正在暂停；当前网络请求会中断，已完成章节将保留…",
      updatedAt: Date.now(),
      terminal: false,
      degraded: false,
      activity: null,
    }));
    try {
      const paused = await pauseNotePipeline(runId);
      setProgress({
        runId,
        phase: "paused",
        current: paused.completedSectionIds.length + paused.failedSectionIds.length,
        total: paused.selectedSectionIds.length || null,
        message: "任务已暂停。已完成章节和运行预算已保存，可以稍后继续。",
        terminal: false,
        degraded: false,
        activity: null,
      });
      refreshDeepNoteDetail(runId, true);
      showFeedback("success", "深度笔记已暂停。");
    } catch (error) {
      deepNotePausedRunIdRef.current = null;
      setDeepNoteProgress((current) => current ? {
        ...current,
        message: `暂停失败：${noteErrorText(error)}`,
        updatedAt: Date.now(),
      } : current);
      showFeedback("error", `暂停深度笔记失败：${noteErrorText(error)}`);
    } finally {
      setDeepNoteControlBusy(false);
    }
  }, [
    deepNoteControlBusy,
    deepNoteProgress?.phase,
    refreshDeepNoteDetail,
    setProgress,
    showFeedback,
  ]);

  const resumeDeepNote = useCallback(async () => {
    const run = deepNoteRunRef.current;
    const resumablePhase = deepNoteProgress?.phase === "paused" || deepNoteProgress?.phase === "cancelled";
    if (!run?.runId || deepNoteControlBusy || !resumablePhase) return;
    const runId = run.runId;
    const previousPhase = deepNoteProgress?.phase ?? "paused";
    setDeepNoteControlBusy(true);
    deepNotePausedRunIdRef.current = null;
    setDeepNoteProgress((current) => current ? {
      ...current,
      message: "正在从已保存的断点继续…",
      updatedAt: Date.now(),
      activity: null,
    } : current);
    try {
      const resumed = await resumeNotePipeline(runId, handleNotePipelineEvent);
      setProgress({
        runId,
        phase: resumed.phase,
        current: resumed.completedSectionIds.length + resumed.failedSectionIds.length,
        total: resumed.selectedSectionIds.length || null,
        message: resumed.selectedSectionIds.length > 0
          ? "已继续执行，正在处理未完成章节…"
          : "已继续执行，正在恢复知识结构分析…",
        terminal: false,
        degraded: false,
        activity: null,
      });
      refreshDeepNoteDetail(runId, true);
      showFeedback("progress", "已继续深度笔记任务。");
    } catch (error) {
      if (previousPhase === "paused") deepNotePausedRunIdRef.current = runId;
      setDeepNoteProgress((current) => ({
        runId,
        phase: previousPhase,
        current: current?.current ?? null,
        total: current?.total ?? null,
        message: `继续失败，原检查点仍然保留：${noteErrorText(error)}`,
        updatedAt: Date.now(),
        terminal: false,
        degraded: false,
        activity: null,
      }));
      showFeedback("error", `继续深度笔记失败：${noteErrorText(error)}`);
    } finally {
      setDeepNoteControlBusy(false);
    }
  }, [
    deepNoteControlBusy,
    deepNoteProgress?.phase,
    handleNotePipelineEvent,
    refreshDeepNoteDetail,
    setProgress,
    showFeedback,
  ]);

  const retryDeepNote = useCallback(async () => {
    const runId = deepNoteRunRef.current?.runId ?? deepNoteDetail?.run.id ?? deepNoteProgress?.runId;
    const conversationId = deepNoteRunRef.current?.conversationId ?? deepNoteDetail?.run.conversationId;
    if (!runId || !conversationId || deepNoteControlBusy) return;
    deepNoteRunRef.current = { conversationId, runId, cancelRequested: false };
    latestDeepNoteRunIdRef.current = runId;
    setDeepNoteControlBusy(true);
    setDeepNoteActive(true);
    setProgress({
      runId,
      phase: deepNoteDetail?.run.outlineJson ? "drafting" : "analyzing",
      current: deepNoteDetail?.run.completedSectionIds.length ?? 0,
      total: deepNoteDetail?.run.selectedSectionIds.length || null,
      message: "正在从失败步骤恢复，已完成的检查点会保留…",
      terminal: false,
      degraded: false,
      activity: null,
    });
    showFeedback("progress", "正在重试深度笔记失败步骤…");
    try {
      const recovered = await retryNotePipeline(runId, handleNotePipelineEvent);
      refreshDeepNoteDetail(recovered.id, true);
    } catch (error) {
      setProgress({
        runId,
        phase: "error",
        current: deepNoteDetail?.run.completedSectionIds.length ?? null,
        total: deepNoteDetail?.run.selectedSectionIds.length || null,
        message: `重试失败：${noteErrorText(error)}`,
        terminal: true,
        degraded: false,
        activity: null,
      });
      showFeedback("error", `重试深度笔记失败：${noteErrorText(error)}`);
    } finally {
      setDeepNoteControlBusy(false);
    }
  }, [deepNoteControlBusy, deepNoteDetail, deepNoteProgress?.runId, handleNotePipelineEvent, refreshDeepNoteDetail, setProgress, showFeedback]);

  const restartDeepNote = useCallback(async () => {
    const oldRunId = deepNoteRunRef.current?.runId ?? deepNoteDetail?.run.id ?? deepNoteProgress?.runId;
    const conversationId = deepNoteRunRef.current?.conversationId ?? deepNoteDetail?.run.conversationId;
    if (!oldRunId || !conversationId || deepNoteControlBusy) return;
    setDeepNoteControlBusy(true);
    setDeepNoteActive(true);
    latestDeepNoteRunIdRef.current = null;
    setProgress({
      runId: oldRunId,
      phase: "preflight",
      current: null,
      total: null,
      message: "正在使用当前会话内容创建全新的深度笔记任务…",
      terminal: false,
      degraded: false,
      activity: null,
    });
    showFeedback("progress", "正在重新生成深度笔记…");
    try {
      const restarted = await restartNotePipeline(oldRunId, handleNotePipelineEvent);
      latestDeepNoteRunIdRef.current = restarted.id;
      deepNoteRunRef.current = { conversationId, runId: restarted.id, cancelRequested: false };
      setDeepNoteDetail(null);
      refreshDeepNoteDetail(restarted.id, true);
    } catch (error) {
      latestDeepNoteRunIdRef.current = oldRunId;
      deepNoteRunRef.current = { conversationId, runId: oldRunId, cancelRequested: false };
      setProgress({
        runId: oldRunId,
        phase: deepNoteDetail?.run.phase ?? "error",
        current: deepNoteDetail?.run.completedSectionIds.length ?? null,
        total: deepNoteDetail?.run.selectedSectionIds.length || null,
        message: `重新生成失败，原任务仍然保留：${noteErrorText(error)}`,
        terminal: true,
        degraded: false,
        activity: null,
      });
      showFeedback("error", `重新生成深度笔记失败：${noteErrorText(error)}`);
    } finally {
      setDeepNoteControlBusy(false);
    }
  }, [deepNoteControlBusy, deepNoteDetail, deepNoteProgress?.runId, handleNotePipelineEvent, refreshDeepNoteDetail, setProgress, showFeedback]);

  const cancelDeepNote = useCallback(async () => {
    const run = deepNoteRunRef.current;
    const alreadyStopping = deepNoteProgress?.phase === "cancelling";
    if (!run || (deepNoteControlBusy && !alreadyStopping)) return;
    setDeepNoteControlBusy(true);
    if (!run.runId) {
      run.cancelRequested = true;
      showFeedback("progress", "正在等待任务启动后取消…");
      setDeepNoteControlBusy(false);
      return;
    }
    const runId = run.runId;
    cancellingDeepNoteRunIdsRef.current.add(runId);
    deepNotePausedRunIdRef.current = null;
    setDeepNoteProgress((current) => ({
      runId,
      current: current?.current ?? null,
      total: current?.total ?? null,
      phase: "cancelling",
      message: alreadyStopping
        ? "正在再次确认后台停止状态；必要时将强制终止…"
        : "停止请求已发送；正在等待后台任务释放资源，必要时会自动强制终止…",
      updatedAt: Date.now(),
      terminal: false,
      degraded: false,
      activity: null,
    }));
    try {
      const result = await cancelNotePipeline(runId);
      const current = result.run;
      refreshDeepNoteDetail(runId, true);
      if (current.phase === "cancelled") {
        handleNotePipelineEvent({ type: "cancelled", run: current });
        if (result.forced) {
          showFeedback(
            "success",
            result.diagnosticPath
              ? `任务未及时退出，已强制停止。诊断日志：${result.diagnosticPath}`
              : "任务未及时退出，已强制停止并保留检查点。",
          );
        }
      } else if (current.phase === "done") {
        cancellingDeepNoteRunIdsRef.current.delete(runId);
        handleNotePipelineEvent({ type: "done", run: current, degraded: false });
      } else if (current.phase === "error") {
        handleNotePipelineEvent({
          type: "error",
          runId,
          message: current.errorMessage ?? "后台深度笔记任务失败。",
        });
      } else {
        setDeepNoteProgress((progress) => progress ? {
          ...progress,
          phase: "cancelling",
          message: "后台仍在结束处理中；控制按钮已恢复，可以再次停止或遗弃任务。",
          updatedAt: Date.now(),
        } : progress);
        showFeedback("progress", "后台仍在结束处理中；可以再次停止或遗弃任务。");
      }
    } catch (error) {
      cancellingDeepNoteRunIdsRef.current.delete(runId);
      showFeedback("error", `取消深度笔记失败：${noteErrorText(error)}`);
    } finally {
      setDeepNoteControlBusy(false);
    }
  }, [
    deepNoteControlBusy,
    deepNoteProgress?.phase,
    handleNotePipelineEvent,
    refreshDeepNoteDetail,
    showFeedback,
  ]);

  const abandonDeepNoteForConversation = useCallback(async (conversationId: string) => {
    const active = deepNoteRunRef.current;
    if (!active || active.conversationId !== conversationId) return 0;
    if (active.runId) ignoredDeepNoteRunIdsRef.current.add(active.runId);
    try {
      const count = await abandonNotePipelinesForConversation(conversationId);
      if (count > 0) {
        setDeepNoteProgress((current) => current ? {
          ...current,
          phase: "cancelled",
          message: "来源对话已删除，深度笔记任务已遗弃，不会继续重试。",
          terminal: true,
          degraded: false,
          activity: null,
          updatedAt: Date.now(),
        } : current);
        setDeepNoteControlBusy(false);
        setDeepNoteActive(false);
        setDeepNoteReview(null);
        setDeepNoteDetail(null);
        deepNoteRunRef.current = null;
      }
      return count;
    } catch (error) {
      if (active.runId) ignoredDeepNoteRunIdsRef.current.delete(active.runId);
      showFeedback("error", `遗弃深度笔记任务失败：${noteErrorText(error)}`);
      throw error;
    }
  }, [showFeedback]);

  const abandonDeepNote = useCallback(async () => {
    const runId = deepNoteRunRef.current?.runId ?? deepNoteDetail?.run.id ?? deepNoteProgress?.runId;
    if (!runId || (deepNoteControlBusy && deepNoteProgress?.phase !== "cancelling")) return;
    ignoredDeepNoteRunIdsRef.current.add(runId);
    setDeepNoteControlBusy(true);
    showFeedback("progress", "正在停止后台请求并永久遗弃任务…");
    try {
      await abandonNotePipeline(runId);
      latestDeepNoteRunIdRef.current = null;
      cancellingDeepNoteRunIdsRef.current.delete(runId);
      detailRequestSequenceRef.current += 1;
      if (detailRefreshTimerRef.current !== null) {
        window.clearTimeout(detailRefreshTimerRef.current);
        detailRefreshTimerRef.current = null;
      }
      finishDeepNoteRun();
      setDeepNoteDetail(null);
      setDeepNoteProgress(null);
      showFeedback("success", "深度笔记任务已遗弃，不会恢复、重试或重新生成。");
    } catch (error) {
      ignoredDeepNoteRunIdsRef.current.delete(runId);
      setDeepNoteControlBusy(false);
      showFeedback("error", `遗弃深度笔记任务失败：${noteErrorText(error)}`);
    }
  }, [deepNoteControlBusy, deepNoteDetail?.run.id, deepNoteProgress?.phase, deepNoteProgress?.runId, finishDeepNoteRun, showFeedback]);

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
        operationId: crypto.randomUUID(),
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

  const applyNoteEdit = useCallback(async (selection?: { hunkIds: number[]; titleAccepted: boolean; content: string; diff: string }) => {
    if (!noteEditResult || noteEditBusy) return;
    if (noteEditResult.requiresGlobalReview && !noteEditResult.globalReviewPassed) {
      const confirmed = window.confirm(
        "新增附件的全局复核未通过或提示可能影响核心结论。\n\n仍然应用这份增量提案吗？应用后会推进覆盖快照，建议先检查完整 Diff。",
      );
      if (!confirmed) return;
    }
    setNoteEditBusy(true);
    try {
      const selectedTitle = selection?.titleAccepted
        ? noteEditResult.proposal.newTitle
        : noteEditResult.proposal.oldTitle;
      const acceptsCompleteProposal = selection
        && selectedTitle === noteEditResult.proposal.newTitle
        && selection.content === noteEditResult.proposal.newContent;
      const updated = !selection || acceptsCompleteProposal
        ? await resolveNoteEdit(noteEditResult.proposal.id, true)
        : await resolveNoteEditContent(
          noteEditResult.proposal.id,
          selectedTitle,
          selection.content,
          selection.diff,
        );
      if (!updated) throw new Error("修改提案已失效。");
      setNoteEditResult(null);
      setNoteEditRefresh((current) => ({
        noteId: updated.id,
        version: (current?.version ?? 0) + 1,
      }));
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
    deepNoteProgress,
    deepNoteControlBusy,
    noteEditRequest,
    noteEditResult,
    noteEditBusy,
    noteEditRefresh,
    saveConversationAsNote,
    summarizeConversationAsNote,
    startDeepNote,
    startLocalFilesDeepNote,
    adjustDeepNoteOutline,
    confirmDeepNoteOutline,
    pauseDeepNote,
    resumeDeepNote,
    retryDeepNote,
    restartDeepNote,
    cancelDeepNote,
    abandonDeepNote,
    abandonDeepNoteForConversation,
    openConversationNoteEdit,
    openSelectionNoteEdit,
    prepareExistingNoteEdit,
    closeNoteEdit,
    applyNoteEdit,
    saveMessage,
  };
}
