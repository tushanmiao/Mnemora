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

export function agentWorkflowNeedsAttention(
  message: ChatMessage,
  streaming = false,
  reasoning = message.reasoning ?? "",
) {
  if (!hasAgentActivity(message, reasoning)) return false;
  return streaming
    || message.status === "pending"
    || message.status === "streaming"
    || message.status === "error"
    || message.status === "stopped"
    || message.toolTraces?.some((trace) => trace.status === "awaitingApproval") === true;
}

export function hasAgentActivity(message: ChatMessage, reasoning = message.reasoning ?? "") {
  return message.role === "assistant" && (
    reasoning.trim().length > 0
    || (message.activatedSkills?.length ?? 0) > 0
    || (message.toolTraces?.length ?? 0) > 0
  );
}

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

function labels(language: "zh" | "en") {
  return language === "en"
    ? {
        reasoning: "Model reasoning",
      }
    : {
        reasoning: "模型思考",
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
  const steps: WorkflowStep[] = [];

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
