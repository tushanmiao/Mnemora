import type { ChatMessage, ToolTraceStatus } from "../../../../types/chat";
import type {
  AgentRunStatus,
  AgentWorkflowProjection,
  AgentWorkflowSummary,
  WorkflowStep,
  WorkflowStepStatus,
} from "../../../../types/workflow";

type ProjectionOptions = {
  reasoning?: string;
  streaming?: boolean;
  language?: "zh" | "en";
};

function runStatus(message: ChatMessage, streaming: boolean): AgentRunStatus {
  if (message.toolTraces?.some((trace) => trace.status === "awaitingApproval")) {
    return "waitingApproval";
  }
  if (message.status === "error") return "failed";
  if (message.status === "stopped") return "stopped";
  if (message.status === "completed") return "completed";
  if (streaming || message.status === "streaming") return "running";
  return message.workflowSummary?.status ?? "preparing";
}

function stepStatus(status: ToolTraceStatus): WorkflowStepStatus {
  if (status === "awaitingApproval" || status === "running") return "running";
  if (status === "completed") return "completed";
  if (status === "rejected") return "rejected";
  return "failed";
}

function terminalStepStatus(status: AgentRunStatus, hasContent: boolean): WorkflowStepStatus {
  if (status === "completed") return "completed";
  if (status === "failed") return hasContent ? "completed" : "failed";
  if (status === "stopped") return hasContent ? "completed" : "stopped";
  if (status === "budgetExhausted") return hasContent ? "completed" : "failed";
  if (status === "finalizing" || hasContent) return "running";
  return "pending";
}

function labels(language: "zh" | "en") {
  return language === "en"
    ? {
        prepare: "Preparing the workflow",
        reasoning: "Model reasoning",
        final: "Compose final answer",
      }
    : {
        prepare: "准备工作流",
        reasoning: "模型推理",
        final: "整理最终回答",
      };
}

/**
 * 将旧消息字段投影为一条统一流程。旧 reasoning、Skill 和 ToolTrace 始终只读，
 * 相同 callId 的 Tool 状态由后端数组中的最终快照决定，因此不会生成重复节点。
 */
export function projectAgentWorkflow(
  message: ChatMessage,
  options: ProjectionOptions = {},
): AgentWorkflowProjection {
  const language = options.language ?? "zh";
  const copy = labels(language);
  const reasoning = options.reasoning ?? message.reasoning ?? "";
  const streaming = options.streaming === true;
  const status = runStatus(message, streaming);
  const tools = message.toolTraces ?? [];
  const skills = message.activatedSkills ?? [];
  const hasContent = message.content.trim().length > 0;
  const steps: WorkflowStep[] = [];

  if ((status === "preparing" || status === "running")
    && reasoning.trim().length === 0
    && skills.length === 0
    && tools.length === 0
    && !hasContent) {
    steps.push({
      id: `${message.id}:prepare`,
      kind: "prepare",
      status: "running",
      title: copy.prepare,
    });
  }

  if (reasoning.trim()) {
    steps.push({
      id: `${message.id}:reasoning`,
      kind: "reasoning",
      status: status === "preparing" || status === "running" ? "running" : "completed",
      title: copy.reasoning,
      reasoning,
    });
  }

  for (const skill of skills) {
    steps.push({
      id: `${message.id}:skill:${skill.id}`,
      kind: "skill",
      status: "completed",
      title: skill.name,
      detail: `v${skill.version}`,
      skill,
    });
  }

  for (const tool of tools) {
    steps.push({
      id: `${message.id}:tool:${tool.callId}`,
      kind: "tool",
      status: stepStatus(tool.status),
      title: tool.name,
      detail: tool.preview,
      tool,
    });
  }

  steps.push({
    id: `${message.id}:final`,
    kind: "final",
    status: terminalStepStatus(status, hasContent),
    title: copy.final,
  });

  const durationMs = message.workflowSummary?.durationMs
    ?? (message.status === "completed" || message.status === "error" || message.status === "stopped"
      ? Math.max(0, message.updatedAt - message.createdAt)
      : undefined);
  const summary: AgentWorkflowSummary = {
    status,
    stepCount: steps.length,
    toolCallCount: tools.length,
    skillCount: skills.length,
    durationMs,
  };

  return {
    status,
    summary,
    steps,
    needsAttention: status === "waitingApproval"
      || status === "waitingUser"
      || status === "paused"
      || status === "failed"
      || status === "stopped"
      || status === "budgetExhausted",
  };
}

export function workflowSummaryForMessage(message: ChatMessage): AgentWorkflowSummary {
  return projectAgentWorkflow(message).summary;
}
