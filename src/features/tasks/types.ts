export type TaskRunKind = "chatAgent" | "deepNote";

export type TaskRunStatus =
  | "running"
  | "waiting"
  | "paused"
  | "completed"
  | "failed"
  | "stopped";

export type TaskRunStepKind = "phase" | "reasoning" | "skill" | "tool";

export type TaskRunStepStatus =
  | "pending"
  | "running"
  | "waiting"
  | "paused"
  | "completed"
  | "failed"
  | "stopped";

export interface TaskRunStepProjection {
  id: string;
  kind: TaskRunStepKind;
  label: string;
  description?: string;
  content?: string;
  status: TaskRunStepStatus;
}

export interface TaskRunMetrics {
  toolCalls?: number;
  skills?: number;
  tokens?: number;
}

/**
 * 任务中心只消费运行事实的轻量投影。执行引擎、聊天消息和深度笔记详情仍是各自状态的唯一来源。
 */
export interface TaskRunProjection {
  id: string;
  sourceId: string;
  kind: TaskRunKind;
  title: string;
  status: TaskRunStatus;
  statusLabel: string;
  currentStepLabel: string;
  activity: string;
  startedAt: number;
  updatedAt: number;
  finishedAt?: number;
  completedCount: number;
  totalCount: number;
  steps: TaskRunStepProjection[];
  metrics: TaskRunMetrics;
  needsAttention: boolean;
  canPause: boolean;
  canResume: boolean;
  canRetry: boolean;
  canRestart: boolean;
  canStop: boolean;
}
