import type { ChatMessage } from "../../../types/chat";
import type { AgentRunStatus, WorkflowStepStatus } from "../../../types/workflow";
import type { DeepNoteRunDetail, NotePipelinePhase } from "../../chat/api/notePipeline";
import { hasAgentActivity, projectAgentWorkflow } from "../../chat/agent/projections/workflowProjection";
import type { DeepNoteProgress } from "../../workspace/runtime/DeepNoteViewRuntime";
import {
  buildDeepNoteWorkflow,
  type DeepNoteWorkflowStatus,
} from "../../workspace/runtime/deepNoteDiagnostics";
import type {
  TaskRunProjection,
  TaskRunStatus,
  TaskRunStepProjection,
  TaskRunStepStatus,
} from "../types";

export const RECENT_TERMINAL_TASK_MS = 15 * 60 * 1_000;

const TERMINAL_STATUSES = new Set<TaskRunStatus>(["completed", "failed", "stopped"]);

type Language = "zh" | "en";

type DeepNoteTaskSource = {
  detail: DeepNoteRunDetail | null;
  progress: DeepNoteProgress | null;
  reviewTitle?: string | null;
};

const deepNoteStatusLabels: Record<Language, Record<TaskRunStatus, string>> = {
  zh: {
    running: "运行中",
    waiting: "等待确认",
    paused: "已暂停",
    completed: "已完成",
    failed: "失败",
    stopped: "已停止",
  },
  en: {
    running: "Running",
    waiting: "Waiting",
    paused: "Paused",
    completed: "Completed",
    failed: "Failed",
    stopped: "Stopped",
  },
};

const chatStatusLabels: Record<Language, Record<TaskRunStatus, string>> = {
  zh: {
    running: "Agent 运行中",
    waiting: "需要处理",
    paused: "已暂停",
    completed: "处理完成",
    failed: "处理失败",
    stopped: "已停止",
  },
  en: {
    running: "Agent running",
    waiting: "Action required",
    paused: "Paused",
    completed: "Completed",
    failed: "Failed",
    stopped: "Stopped",
  },
};

export function projectDeepNoteTaskRun(
  source: DeepNoteTaskSource,
  language: Language,
  now = Date.now(),
): TaskRunProjection | null {
  const run = source.detail?.run;
  const phase = source.progress?.phase ?? run?.phase ?? null;
  const sourceId = source.progress?.runId ?? run?.id ?? null;
  if (!phase || !sourceId) return null;

  const status = deepNoteRunStatus(phase);
  const updatedAt = source.progress?.updatedAt ?? run?.updatedAt ?? now;
  if (TERMINAL_STATUSES.has(status) && now - updatedAt > RECENT_TERMINAL_TASK_MS) return null;

  const workflow = buildDeepNoteWorkflow(source.detail, phase);
  const steps: TaskRunStepProjection[] = workflow.map((step) => ({
    id: `deep-note:${sourceId}:${step.id}`,
    kind: "phase",
    label: step.label,
    description: step.description,
    status: deepNoteStepStatus(step.status),
  }));
  const current = currentTaskStep(steps);
  const completedCount = steps.filter((step) => step.status === "completed").length;
  const title = source.detail?.planVersion?.plan.title
    ?? source.reviewTitle
    ?? (language === "en" ? "Deep note" : "深度笔记");

  return {
    id: `deep-note:${sourceId}`,
    sourceId,
    kind: "deepNote",
    title,
    status,
    statusLabel: deepNoteStatusLabels[language][status],
    currentStepLabel: current?.label ?? deepNoteStatusLabels[language][status],
    activity: source.progress?.message
      ?? run?.errorMessage
      ?? current?.description
      ?? deepNoteStatusLabels[language][status],
    startedAt: run?.createdAt ?? updatedAt,
    updatedAt,
    finishedAt: TERMINAL_STATUSES.has(status) ? updatedAt : undefined,
    completedCount,
    totalCount: steps.length,
    steps,
    metrics: {},
    needsAttention: status === "waiting" || status === "paused" || status === "failed",
    canPause: status === "running" && pauseableDeepNotePhase(phase),
    canResume: status === "paused",
    canStop: !TERMINAL_STATUSES.has(status),
  };
}

export function projectChatTaskRun(
  message: ChatMessage | null,
  reasoning: string,
  streaming: boolean,
  language: Language,
  now = Date.now(),
): TaskRunProjection | null {
  if (!message || !hasAgentActivity(message, reasoning)) return null;

  const workflow = projectAgentWorkflow(message, { reasoning, streaming, language });
  const status = chatRunStatus(workflow.status);
  if (TERMINAL_STATUSES.has(status) && now - message.updatedAt > RECENT_TERMINAL_TASK_MS) return null;

  const steps: TaskRunStepProjection[] = workflow.steps.map((step) => ({
    id: `chat:${message.id}:${step.id}`,
    kind: step.kind === "final" || step.kind === "prepare" ? "phase" : step.kind,
    label: step.kind === "tool" ? toolLabel(step.title, language) : step.title,
    description: step.kind === "tool"
      ? step.tool?.argumentSummary
      : step.detail,
    content: step.reasoning ?? step.tool?.preview,
    status: chatStepStatus(step.status, step.tool?.status === "awaitingApproval"),
  }));
  const current = currentTaskStep(steps);
  const completedCount = steps.filter((step) => step.status === "completed").length;
  const statusLabel = chatStatusLabels[language][status];

  return {
    id: `chat:${message.id}`,
    sourceId: message.id,
    kind: "chatAgent",
    title: language === "en" ? "Chat Agent" : "Chat Agent",
    status,
    statusLabel,
    currentStepLabel: current?.label ?? statusLabel,
    activity: chatActivityLabel(steps, status, language),
    startedAt: message.createdAt,
    updatedAt: message.updatedAt,
    finishedAt: TERMINAL_STATUSES.has(status) ? message.updatedAt : undefined,
    completedCount,
    totalCount: steps.length,
    steps,
    metrics: {
      toolCalls: workflow.summary.toolCallCount || undefined,
      skills: workflow.summary.skillCount || undefined,
      tokens: message.usage?.totalTokens,
    },
    needsAttention: workflow.needsAttention,
    canPause: false,
    canResume: false,
    canStop: status === "running",
  };
}

export function sortTaskRuns(tasks: readonly TaskRunProjection[]): TaskRunProjection[] {
  const rank: Record<TaskRunStatus, number> = {
    waiting: 0,
    failed: 1,
    paused: 2,
    running: 3,
    stopped: 4,
    completed: 5,
  };
  return [...tasks].sort((left, right) => (
    rank[left.status] - rank[right.status]
    || right.updatedAt - left.updatedAt
  ));
}

export function isTaskRunTerminal(status: TaskRunStatus) {
  return TERMINAL_STATUSES.has(status);
}

function currentTaskStep(steps: readonly TaskRunStepProjection[]) {
  return steps.find((step) => (
    step.status === "running"
    || step.status === "waiting"
    || step.status === "paused"
    || step.status === "failed"
  )) ?? [...steps].reverse().find((step) => step.status === "completed") ?? steps[0];
}

function deepNoteRunStatus(phase: NotePipelinePhase): TaskRunStatus {
  if (phase === "awaitingOutline" || phase === "blocked") return "waiting";
  if (phase === "paused") return "paused";
  if (phase === "done") return "completed";
  if (phase === "cancelled") return "stopped";
  if (phase === "error") return "failed";
  return "running";
}

function deepNoteStepStatus(status: DeepNoteWorkflowStatus): TaskRunStepStatus {
  if (status === "active") return "running";
  return status === "stopped" ? "stopped" : status;
}

function chatRunStatus(status: AgentRunStatus): TaskRunStatus {
  if (status === "waitingApproval" || status === "waitingUser") return "waiting";
  if (status === "paused") return "paused";
  if (status === "completed") return "completed";
  if (status === "stopped") return "stopped";
  if (status === "failed" || status === "budgetExhausted") return "failed";
  return "running";
}

function chatStepStatus(status: WorkflowStepStatus, awaitingApproval: boolean): TaskRunStepStatus {
  if (awaitingApproval) return "waiting";
  if (status === "rejected" || status === "stopped") return "stopped";
  return status;
}

function pauseableDeepNotePhase(phase: NotePipelinePhase) {
  return phase === "analyzing"
    || phase === "compiling"
    || phase === "queued"
    || phase === "drafting"
    || phase === "validating"
    || phase === "replanning";
}

function chatActivityLabel(
  steps: readonly TaskRunStepProjection[],
  status: TaskRunStatus,
  language: Language,
) {
  const current = currentTaskStep(steps);
  if (status === "completed") return language === "en" ? "Reasoning and calls completed" : "思考与调用已完成";
  if (status === "failed") return language === "en" ? "Agent processing failed" : "Agent 处理失败";
  if (status === "stopped") return language === "en" ? "Agent processing stopped" : "Agent 处理已停止";
  if (status === "waiting") return language === "en" ? `Action required: ${current?.label ?? "tool"}` : `需要处理：${current?.label ?? "工具调用"}`;
  if (current?.kind === "reasoning") return language === "en" ? "Model reasoning" : "模型正在思考";
  if (current?.kind === "skill") return language === "en" ? `Using skill: ${current.label}` : `正在使用技能：${current.label}`;
  if (current?.kind === "tool") return language === "en" ? `Calling: ${current.label}` : `正在调用：${current.label}`;
  return current?.label ?? chatStatusLabels[language][status];
}

function toolLabel(name: string, language: Language) {
  const labels: Record<string, [string, string]> = {
    search_tools: ["搜索工具目录", "Search tool catalog"],
    search_skills: ["搜索技能目录", "Search skill catalog"],
    skill: ["加载技能", "Load skill"],
    read_attachment_text: ["读取文本附件", "Read text attachment"],
    read_pdf_pages: ["读取 PDF 页面", "Read PDF pages"],
    read_docx_blocks: ["读取 DOCX 内容", "Read DOCX blocks"],
    read_xlsx_rows: ["读取表格行", "Read spreadsheet rows"],
    memory_read: ["读取记忆", "Read memory"],
    memory_search: ["搜索记忆", "Search memory"],
    memory_modify: ["更新记忆", "Update memory"],
  };
  const value = labels[name];
  if (!value) return name;
  return language === "en" ? value[1] : value[0];
}
