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
  if (message.role !== "assistant") return false;
  // 新消息使用空事件账本表示“尚未发生真实活动”。预选 Skill 只是请求
  // 元数据，不能单独触发工作流入口；旧消息没有账本时继续兼容旧字段。
  if (message.agentEvents !== undefined) {
    return reasoning.trim().length > 0
      || message.agentEvents.length > 0
      || message.toolTraces?.some((trace) => trace.status === "awaitingApproval") === true;
  }
  return reasoning.trim().length > 0
    || (message.activatedSkills?.length ?? 0) > 0
    || (message.toolTraces?.length ?? 0) > 0;
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
        reasoningSummary: "Reasoning summary",
      }
    : {
        reasoning: "模型思考",
        reasoningSummary: "思考摘要",
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
  const hasEventLedger = message.agentEvents !== undefined;
  const steps: WorkflowStep[] = hasEventLedger
    ? projectOrderedEvents(message, reasoning, status, copy)
    : projectLegacySteps(message, reasoning, status, copy);

  // 事件账本是引用式结构。若旧版本或中断恢复留下了孤立快照，仍把它们
  // 追加到末尾，保证调用详情不会因单个损坏事件而完全丢失。
  if (!hasEventLedger) {
    const referencedSkills = new Set(steps.flatMap((step) => step.skill ? [step.skill.id] : []));
    const referencedTools = new Set(steps.flatMap((step) => step.tool ? [step.tool.callId] : []));
    for (const skill of skills) {
      if (referencedSkills.has(skill.id)) continue;
      steps.push(skillStep(message.id, skill));
    }
    for (const tool of tools) {
      if (referencedTools.has(tool.callId)) continue;
      steps.push(toolStep(message.id, tool));
    }
  }

  const durationMs = message.workflowSummary?.durationMs
    ?? (message.status === "completed" || message.status === "error" || message.status === "stopped"
      ? Math.max(0, message.updatedAt - message.createdAt)
      : undefined);
  const summary: AgentWorkflowSummary = {
    status,
    stepCount: steps.length,
    toolCallCount: hasEventLedger
      ? new Set(message.agentEvents?.flatMap((event) => event.kind === "tool" ? [event.callId] : [])).size
      : tools.length,
    skillCount: hasEventLedger
      ? new Set(message.agentEvents?.flatMap((event) => event.kind === "skill" ? [event.skillId] : [])).size
      : skills.length,
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

function projectOrderedEvents(
  message: ChatMessage,
  reasoning: string,
  status: AgentRunStatus,
  copy: ReturnType<typeof labels>,
) {
  const skills = new Map((message.activatedSkills ?? []).map((skill) => [skill.id, skill]));
  const tools = new Map((message.toolTraces ?? []).map((tool) => [tool.callId, tool]));
  return [...(message.agentEvents ?? [])]
    .sort((left, right) => left.sequence - right.sequence || left.createdAt - right.createdAt)
    .flatMap<WorkflowStep>((event) => {
      if (event.kind === "reasoning") {
        const content = reasoning.slice(event.startOffset, event.endOffset).trim();
        if (!content) return [];
        return [{
          id: event.id,
          kind: "reasoning",
          status: status === "preparing" || status === "running" ? "running" : "completed",
          title: event.reasoningLabel === "summary" ? copy.reasoningSummary : copy.reasoning,
          reasoning: content,
          reasoningLabel: event.reasoningLabel,
          sequence: event.sequence,
          createdAt: event.createdAt,
        }];
      }
      if (event.kind === "skill") {
        const skill = skills.get(event.skillId);
        return skill ? [{ ...skillStep(message.id, skill), id: event.id, sequence: event.sequence, createdAt: event.createdAt }] : [];
      }
      const tool = tools.get(event.callId);
      return tool ? [{ ...toolStep(message.id, tool), id: event.id, sequence: event.sequence, createdAt: event.createdAt }] : [];
    });
}

function projectLegacySteps(
  message: ChatMessage,
  reasoning: string,
  status: AgentRunStatus,
  copy: ReturnType<typeof labels>,
) {
  const steps: WorkflowStep[] = [];
  if (reasoning.trim()) {
    const reasoningLabel = message.modelSnapshot?.protocol === "openAiResponses" ? "summary" : "reasoning";
    steps.push({
      id: `${message.id}:reasoning`, kind: "reasoning",
      status: status === "preparing" || status === "running" ? "running" : "completed",
      title: reasoningLabel === "summary" ? copy.reasoningSummary : copy.reasoning,
      reasoning, reasoningLabel,
    });
  }
  return steps;
}

function skillStep(messageId: string, skill: NonNullable<ChatMessage["activatedSkills"]>[number]): WorkflowStep {
  return { id: `${messageId}:skill:${skill.id}`, kind: "skill", status: "completed", title: skill.name, detail: `v${skill.version}`, skill };
}

function toolStep(messageId: string, tool: NonNullable<ChatMessage["toolTraces"]>[number]): WorkflowStep {
  return { id: `${messageId}:tool:${tool.callId}`, kind: "tool", status: stepStatus(tool.status), title: tool.name, detail: tool.preview, tool };
}

export function workflowSummaryForMessage(message: ChatMessage): AgentWorkflowSummary {
  return projectAgentWorkflow(message).summary;
}
