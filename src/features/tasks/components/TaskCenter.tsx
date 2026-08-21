import {
  AlertTriangle,
  Bot,
  BrainCircuit,
  Check,
  ChevronDown,
  ChevronRight,
  Circle,
  CircleStop,
  ExternalLink,
  ListChecks,
  LoaderCircle,
  Pause,
  Play,
  RotateCcw,
  Sparkles,
  Square,
  Trash2,
  Wrench,
  X,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import type { ChatMessage } from "../../../types/chat";
import { useI18n } from "../../../i18n/I18nProvider";
import type { DeepNoteRunDetail } from "../../chat/api/notePipeline";
import { useStreamingMessage } from "../../chat/stores/streamingStore";
import type { DeepNoteProgress } from "../../workspace/runtime/DeepNoteViewRuntime";
import {
  isTaskRunTerminal,
  projectChatTaskRun,
  projectDeepNoteTaskRun,
  RECENT_TERMINAL_TASK_MS,
  sortTaskRuns,
} from "../projections/taskRunProjection";
import type {
  TaskRunKind,
  TaskRunProjection,
  TaskRunStatus,
  TaskRunStepProjection,
} from "../types";
import "../styles/task-center.css";

type TaskCenterProps = {
  chatMessage: ChatMessage | null;
  showChatTask?: boolean;
  deepNoteDetail: DeepNoteRunDetail | null;
  deepNoteProgress: DeepNoteProgress | null;
  deepNoteReviewTitle?: string | null;
  deepNoteControlBusy: boolean;
  onOpenChatTask: (messageId: string) => void;
  onStopChatTask: () => void;
  onOpenDeepNoteTask: () => void;
  onPauseDeepNoteTask: () => void;
  onResumeDeepNoteTask: () => void;
  onRetryDeepNoteTask: () => void;
  onRestartDeepNoteTask: () => void;
  onStopDeepNoteTask: () => void;
  onAbandonDeepNoteTask: () => void;
};

type TaskCenterPosition = { x: number; y: number };
const TASK_CENTER_POSITION_KEY = "mnemora.task-center.position.v1";
const VIEWPORT_MARGIN = 8;

const copy = {
  zh: {
    title: "任务进度",
    open: "打开任务进度",
    close: "关闭任务进度",
    runningTasks: (count: number) => `${count} 个任务进行中`,
    recentTasks: (count: number) => `${count} 个最近任务`,
    current: "当前活动",
    elapsed: "已用时",
    steps: "计划进度",
    tools: "工具",
    skills: "技能",
    tokens: "Token",
    pause: "暂停",
    resume: "继续",
    retry: "重试失败步骤",
    restart: "重新生成",
    stop: "停止",
    abandon: "遗弃任务",
    abandonConfirm: "永久遗弃这个深度笔记任务吗？\n\n后台请求会停止，任务将从进度面板中关闭，并且不能继续、重试或重新生成。已保存的笔记不会删除。",
    details: "完整详情",
    showContent: "查看过程内容",
    taskList: "任务列表",
  },
  en: {
    title: "Task progress",
    open: "Open task progress",
    close: "Close task progress",
    runningTasks: (count: number) => `${count} tasks running`,
    recentTasks: (count: number) => `${count} recent tasks`,
    current: "Current activity",
    elapsed: "Elapsed",
    steps: "Plan progress",
    tools: "Tools",
    skills: "Skills",
    tokens: "Tokens",
    pause: "Pause",
    resume: "Resume",
    retry: "Retry failed step",
    restart: "Regenerate",
    stop: "Stop",
    abandon: "Discard task",
    abandonConfirm: "Permanently discard this deep-note task?\n\nBackground work will stop, the task will close, and it cannot be resumed, retried, or regenerated. Saved notes will not be deleted.",
    details: "Full details",
    showContent: "Show process content",
    taskList: "Task list",
  },
} as const;

export function TaskCenter({
  chatMessage,
  showChatTask = true,
  deepNoteDetail,
  deepNoteProgress,
  deepNoteReviewTitle,
  deepNoteControlBusy,
  onOpenChatTask,
  onStopChatTask,
  onOpenDeepNoteTask,
  onPauseDeepNoteTask,
  onResumeDeepNoteTask,
  onRetryDeepNoteTask,
  onRestartDeepNoteTask,
  onStopDeepNoteTask,
  onAbandonDeepNoteTask,
}: TaskCenterProps) {
  const { language } = useI18n();
  const text = copy[language];
  const [clock, setClock] = useState(Date.now());
  const [open, setOpen] = useState(false);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [position, setPosition] = useState<TaskCenterPosition | null>(loadTaskCenterPosition);
  const [dragging, setDragging] = useState(false);
  const rootRef = useRef<HTMLElement>(null);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
    suppressClick: boolean;
  } | null>(null);
  const dragMovedRef = useRef(false);
  const suppressTriggerClickRef = useRef(false);
  const chatCanStream = Boolean(chatMessage && (
    chatMessage.status === "pending" || chatMessage.status === "streaming"
  ));
  const streaming = useStreamingMessage(chatMessage?.id ?? "", chatCanStream);
  const reasoning = streaming?.reasoning ?? chatMessage?.reasoning ?? "";

  const tasks = useMemo(() => sortTaskRuns([
    projectDeepNoteTaskRun({
      detail: deepNoteDetail,
      progress: deepNoteProgress,
      reviewTitle: deepNoteReviewTitle,
    }, language, clock),
    showChatTask ? projectChatTaskRun(chatMessage, reasoning, chatCanStream || streaming !== null, language, clock) : null,
  ].filter((task): task is TaskRunProjection => task !== null)), [
    chatCanStream,
    chatMessage,
    clock,
    deepNoteDetail,
    deepNoteProgress,
    deepNoteReviewTitle,
    language,
    reasoning,
    showChatTask,
  ]);

  const liveTasks = tasks.filter((task) => !isTaskRunTerminal(task.status));
  const selectedTask = tasks.find((task) => task.id === selectedTaskId) ?? tasks[0] ?? null;
  const primaryTask = tasks[0] ?? null;

  useEffect(() => {
    if (!tasks.length) {
      setSelectedTaskId(null);
      setOpen(false);
      return;
    }
    if (!selectedTaskId || !tasks.some((task) => task.id === selectedTaskId)) {
      setSelectedTaskId(tasks[0].id);
    }
  }, [selectedTaskId, tasks]);

  useEffect(() => {
    if (liveTasks.length === 0) return undefined;
    const timer = window.setInterval(() => setClock(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [liveTasks.length]);

  useEffect(() => {
    const expiry = tasks
      .filter((task) => isTaskRunTerminal(task.status))
      .map((task) => task.updatedAt + RECENT_TERMINAL_TASK_MS)
      .filter((value) => value > clock)
      .sort((left, right) => left - right)[0];
    if (!expiry || liveTasks.length > 0) return undefined;
    const timer = window.setTimeout(() => setClock(Date.now()), expiry - clock + 20);
    return () => window.clearTimeout(timer);
  }, [clock, liveTasks.length, tasks]);

  useEffect(() => {
    if (!open) return undefined;
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const clampPosition = useCallback((next: TaskCenterPosition) => {
    const root = rootRef.current;
    const panel = root?.querySelector<HTMLElement>(".task-center-panel");
    const trigger = root?.querySelector<HTMLElement>(".task-center-trigger");
    const width = Math.max(root?.offsetWidth ?? 0, panel?.offsetWidth ?? 0, trigger?.offsetWidth ?? 0);
    const height = open
      ? (trigger?.offsetHeight ?? 40) + 7 + (panel?.offsetHeight ?? 0)
      : (trigger?.offsetHeight ?? root?.offsetHeight ?? 40);
    return {
      x: Math.min(Math.max(VIEWPORT_MARGIN, next.x), Math.max(VIEWPORT_MARGIN, window.innerWidth - width - VIEWPORT_MARGIN)),
      y: Math.min(Math.max(VIEWPORT_MARGIN, next.y), Math.max(VIEWPORT_MARGIN, window.innerHeight - height - VIEWPORT_MARGIN)),
    };
  }, [open]);

  useEffect(() => {
    if (!position) return undefined;
    const clamp = () => setPosition((current) => current ? clampPosition(current) : current);
    const frame = window.requestAnimationFrame(clamp);
    window.addEventListener("resize", clamp);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", clamp);
    };
  }, [clampPosition, open, position !== null]);

  const beginDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button:not(.task-center-trigger)")) return;
    const rect = rootRef.current?.getBoundingClientRect();
    if (!rect) return;
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: position?.x ?? rect.left,
      originY: position?.y ?? rect.top,
      suppressClick: event.currentTarget.classList.contains("task-center-trigger"),
    };
    dragMovedRef.current = false;
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveDrag = (event: ReactPointerEvent<HTMLElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const deltaX = event.clientX - drag.startX;
    const deltaY = event.clientY - drag.startY;
    if (!dragMovedRef.current && Math.hypot(deltaX, deltaY) < 4) return;
    dragMovedRef.current = true;
    setDragging(true);
    setPosition(clampPosition({ x: drag.originX + deltaX, y: drag.originY + deltaY }));
  };

  const endDrag = (event: ReactPointerEvent<HTMLElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragRef.current = null;
    setDragging(false);
    if (dragMovedRef.current) {
      const finalPosition = clampPosition({
        x: drag.originX + event.clientX - drag.startX,
        y: drag.originY + event.clientY - drag.startY,
      });
      suppressTriggerClickRef.current = drag.suppressClick;
      setPosition(finalPosition);
      persistTaskCenterPosition(finalPosition);
    }
  };

  const resetPosition = () => {
    setPosition(null);
    try { window.localStorage.removeItem(TASK_CENTER_POSITION_KEY); } catch { /* 存储不可用时仍可复位当前会话 */ }
  };

  if (!primaryTask || !selectedTask) return null;

  const triggerTitle = tasks.length > 1
    ? (liveTasks.length > 0 ? text.runningTasks(liveTasks.length) : text.recentTasks(tasks.length))
    : primaryTask.currentStepLabel;
  const triggerMeta = tasks.length > 1
    ? `${primaryTask.currentStepLabel} · ${formatDuration(taskDuration(primaryTask, clock), language)}`
    : `${primaryTask.completedCount}/${primaryTask.totalCount} · ${formatDuration(taskDuration(primaryTask, clock), language)}`;

  const handleOpenDetails = () => {
    setOpen(false);
    if (selectedTask.kind === "deepNote") onOpenDeepNoteTask();
    else onOpenChatTask(selectedTask.sourceId);
  };

  return (
    <aside
      className={`task-center${dragging ? " is-dragging" : ""}`}
      ref={rootRef}
      data-open={open ? "true" : "false"}
      style={position ? { left: position.x, top: position.y, right: "auto" } as CSSProperties : undefined}
    >
      <button
        className="task-center-trigger"
        data-status={primaryTask.status}
        type="button"
        title={`${text.open}：${triggerTitle}`}
        aria-label={`${text.open}：${triggerTitle}`}
        aria-expanded={open}
        onPointerDown={beginDrag}
        onPointerMove={moveDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={resetPosition}
        onClick={() => {
          if (suppressTriggerClickRef.current) {
            suppressTriggerClickRef.current = false;
            return;
          }
          setOpen((value) => !value);
        }}
      >
        <span className="task-center-trigger-icon" aria-hidden="true">
          <TaskStatusIcon status={primaryTask.status} />
        </span>
        <span className="task-center-trigger-copy">
          <strong>{triggerTitle}</strong>
          <small>{triggerMeta}</small>
        </span>
        <ChevronDown className="task-center-trigger-chevron" size={15} aria-hidden="true" />
      </button>

      {open ? (
        <section className="task-center-panel" aria-label={text.title}>
          <header
            className="task-center-panel-header"
            title={language === "en" ? "Drag to move; double-click to reset" : "拖动可移动；双击恢复默认位置"}
            onPointerDown={beginDrag}
            onPointerMove={moveDrag}
            onPointerUp={endDrag}
            onPointerCancel={endDrag}
            onDoubleClick={resetPosition}
          >
            <span><ListChecks size={16} />{text.title}</span>
            <small>{liveTasks.length > 0 ? text.runningTasks(liveTasks.length) : text.recentTasks(tasks.length)}</small>
            <button className="icon-button" type="button" title={text.close} aria-label={text.close} onClick={() => setOpen(false)}>
              <X size={16} />
            </button>
          </header>

          {tasks.length > 1 ? (
            <nav className="task-center-task-list" aria-label={text.taskList}>
              {tasks.map((task) => (
                <button
                  key={task.id}
                  data-active={task.id === selectedTask.id ? "true" : "false"}
                  data-status={task.status}
                  type="button"
                  onClick={() => setSelectedTaskId(task.id)}
                >
                  <span aria-hidden="true"><TaskKindIcon kind={task.kind} /></span>
                  <span><strong>{task.title}</strong><small>{task.currentStepLabel}</small></span>
                  <TaskStatusIcon status={task.status} />
                </button>
              ))}
            </nav>
          ) : null}

          <div className="task-center-detail" data-kind={selectedTask.kind} data-status={selectedTask.status}>
            <div className="task-center-run-heading">
              <span className="task-center-kind-icon" aria-hidden="true"><TaskKindIcon kind={selectedTask.kind} /></span>
              <span>
                <strong>{selectedTask.title}</strong>
                <small>{selectedTask.statusLabel} · {text.elapsed} {formatDuration(taskDuration(selectedTask, clock), language)}</small>
              </span>
              <b>{selectedTask.completedCount}/{selectedTask.totalCount}</b>
            </div>

            <div className="task-center-progress" aria-label={`${selectedTask.completedCount}/${selectedTask.totalCount}`}>
              <span style={{ transform: `scaleX(${completionPercent(selectedTask) / 100})` }} />
            </div>

            <section className="task-center-activity" data-attention={selectedTask.needsAttention ? "true" : "false"}>
              <span aria-hidden="true">{selectedTask.needsAttention ? <AlertTriangle size={14} /> : <ChevronRight size={14} />}</span>
              <span><small>{text.current}</small><strong>{selectedTask.activity}</strong></span>
            </section>

            <TaskMetrics task={selectedTask} language={language} />

            <section className="task-center-plan">
              <h2>{text.steps}</h2>
              <ol>
                {selectedTask.steps.map((step) => <TaskStep key={step.id} step={step} language={language} />)}
              </ol>
            </section>
          </div>

          <footer className="task-center-actions">
            {selectedTask.canRetry ? (
              <button type="button" disabled={deepNoteControlBusy} onClick={onRetryDeepNoteTask}>
                <RotateCcw size={14} />{text.retry}
              </button>
            ) : selectedTask.canResume ? (
              <button type="button" disabled={deepNoteControlBusy} onClick={onResumeDeepNoteTask}>
                <Play size={14} />{text.resume}
              </button>
            ) : selectedTask.canPause ? (
              <button type="button" disabled={deepNoteControlBusy} onClick={onPauseDeepNoteTask}>
                <Pause size={14} />{text.pause}
              </button>
            ) : null}
            {selectedTask.canRestart ? (
              <button type="button" disabled={deepNoteControlBusy} onClick={onRestartDeepNoteTask}>
                <RotateCcw size={14} />{text.restart}
              </button>
            ) : null}
            {selectedTask.canStop ? (
              <button
                className="task-center-stop"
                type="button"
                disabled={selectedTask.kind === "deepNote" ? deepNoteControlBusy : false}
                onClick={selectedTask.kind === "deepNote" ? onStopDeepNoteTask : onStopChatTask}
              >
                <Square size={13} />{text.stop}
              </button>
            ) : null}
            {selectedTask.kind === "deepNote" && selectedTask.canAbandon ? (
              <button
                className="task-center-abandon"
                type="button"
                disabled={deepNoteControlBusy}
                onClick={() => {
                  if (!window.confirm(text.abandonConfirm)) return;
                  setOpen(false);
                  onAbandonDeepNoteTask();
                }}
              >
                <Trash2 size={14} />{text.abandon}
              </button>
            ) : null}
            <button className="task-center-open-details" type="button" onClick={handleOpenDetails}>
              <ExternalLink size={14} />{text.details}
            </button>
          </footer>
        </section>
      ) : null}
    </aside>
  );
}

function loadTaskCenterPosition(): TaskCenterPosition | null {
  if (typeof window === "undefined") return null;
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(TASK_CENTER_POSITION_KEY) ?? "null");
    if (!value || typeof value !== "object") return null;
    const { x, y } = value as Partial<TaskCenterPosition>;
    return typeof x === "number" && Number.isFinite(x) && typeof y === "number" && Number.isFinite(y)
      ? { x, y }
      : null;
  } catch {
    return null;
  }
}

function persistTaskCenterPosition(position: TaskCenterPosition) {
  try { window.localStorage.setItem(TASK_CENTER_POSITION_KEY, JSON.stringify(position)); } catch { /* 忽略受限存储环境 */ }
}

function TaskMetrics({ task, language }: { task: TaskRunProjection; language: "zh" | "en" }) {
  const text = copy[language];
  const metrics: Array<[string, string | number]> = [];
  if (task.metrics.toolCalls) metrics.push([text.tools, task.metrics.toolCalls]);
  if (task.metrics.skills) metrics.push([text.skills, task.metrics.skills]);
  if (task.metrics.tokens) metrics.push([text.tokens, formatNumber(task.metrics.tokens, language)]);
  if (metrics.length === 0) return null;
  return (
    <dl className="task-center-metrics">
      {metrics.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
    </dl>
  );
}

function TaskStep({ step, language }: { step: TaskRunStepProjection; language: "zh" | "en" }) {
  const text = copy[language];
  return (
    <li data-status={step.status} data-kind={step.kind}>
      <span className="task-center-step-node" aria-hidden="true"><TaskStepIcon step={step} /></span>
      <div>
        <strong>{step.label}</strong>
        {step.description ? <small>{step.description}</small> : null}
        {step.content ? (
          <details>
            <summary>{text.showContent}</summary>
            <pre>{step.content}</pre>
          </details>
        ) : null}
        {step.kind === "skill" && step.skill ? (
          <small className="task-center-step-meta">{step.skill.id} · v{step.skill.version} · {step.skill.activation}</small>
        ) : null}
        {step.kind === "tool" && step.tool ? (
          <details className="task-center-step-detail">
            <summary>调用详情</summary>
            <code>{step.tool.argumentSummary}</code>
            {step.tool.preview ? <p>{step.tool.preview}</p> : null}
          </details>
        ) : null}
      </div>
    </li>
  );
}

function TaskKindIcon({ kind }: { kind: TaskRunKind }) {
  return kind === "deepNote" ? <BrainCircuit size={15} /> : <Bot size={15} />;
}

function TaskStatusIcon({ status }: { status: TaskRunStatus }) {
  if (status === "completed") return <Check size={15} />;
  if (status === "failed") return <XCircle size={15} />;
  if (status === "stopped") return <CircleStop size={15} />;
  if (status === "abandoned") return <CircleStop size={15} />;
  if (status === "paused") return <Pause size={15} />;
  if (status === "waiting") return <AlertTriangle size={15} />;
  return <LoaderCircle className="task-center-spin" size={15} />;
}

function TaskStepIcon({ step }: { step: TaskRunStepProjection }): ReactNode {
  if (step.status === "completed") return <Check size={12} />;
  if (step.status === "failed") return <XCircle size={12} />;
  if (step.status === "stopped") return <CircleStop size={12} />;
  if (step.status === "paused") return <Pause size={12} />;
  if (step.status === "waiting") return <AlertTriangle size={12} />;
  if (step.status === "running") return <LoaderCircle className="task-center-spin" size={12} />;
  if (step.kind === "reasoning") return <BrainCircuit size={12} />;
  if (step.kind === "skill") return <Sparkles size={12} />;
  if (step.kind === "tool") return <Wrench size={12} />;
  return <Circle size={10} />;
}

function taskDuration(task: TaskRunProjection, now: number) {
  return Math.max(0, (task.finishedAt ?? now) - task.startedAt);
}

function completionPercent(task: TaskRunProjection) {
  if (task.totalCount <= 0) return 0;
  return Math.min(100, Math.round((task.completedCount / task.totalCount) * 100));
}

function formatDuration(milliseconds: number, language: "zh" | "en") {
  const totalSeconds = Math.max(0, Math.round(milliseconds / 1_000));
  if (totalSeconds < 60) return language === "en" ? `${totalSeconds}s` : `${totalSeconds} 秒`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) return language === "en" ? `${minutes}m ${seconds}s` : `${minutes} 分 ${seconds} 秒`;
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return language === "en" ? `${hours}h ${remainingMinutes}m` : `${hours} 小时 ${remainingMinutes} 分`;
}

function formatNumber(value: number, language: "zh" | "en") {
  return new Intl.NumberFormat(language === "en" ? "en-US" : "zh-CN", {
    notation: value >= 10_000 ? "compact" : "standard",
    maximumFractionDigits: 1,
  }).format(value);
}
