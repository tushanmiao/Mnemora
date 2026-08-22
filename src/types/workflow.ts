import type { ActivatedSkillSnapshot, ToolTrace } from "./chat";

export type AgentRunStatus =
  | "preparing"
  | "running"
  | "waitingApproval"
  | "waitingUser"
  | "paused"
  | "checkpointing"
  | "finalizing"
  | "completed"
  | "failed"
  | "stopped"
  | "budgetExhausted";

export type WorkflowStepStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "rejected"
  | "stopped";

export type WorkflowStepKind = "prepare" | "reasoning" | "skill" | "tool" | "final";

export interface AgentWorkflowSummary {
  status: AgentRunStatus;
  stepCount: number;
  toolCallCount: number;
  skillCount: number;
  durationMs?: number;
}

/** UI 投影后的稳定步骤；同一 Tool call 只会对应一个步骤。 */
export interface WorkflowStep {
  id: string;
  kind: WorkflowStepKind;
  status: WorkflowStepStatus;
  title: string;
  detail?: string;
  reasoning?: string;
  reasoningLabel?: "reasoning" | "summary";
  skill?: ActivatedSkillSnapshot;
  tool?: ToolTrace;
  sequence?: number;
  createdAt?: number;
}

export interface AgentWorkflowProjection {
  status: AgentRunStatus;
  summary: AgentWorkflowSummary;
  steps: WorkflowStep[];
  toolOutcomes: {
    total: number;
    succeeded: number;
    failed: number;
    active: number;
  };
  needsAttention: boolean;
}
