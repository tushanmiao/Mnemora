import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../../../types/chat";
import { agentWorkflowNeedsAttention, projectAgentWorkflow } from "./workflowProjection";

function message(patch: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: "message-1",
    conversationId: "conversation-1",
    role: "assistant",
    content: "最终答案",
    status: "completed",
    createdAt: 100,
    updatedAt: 900,
    ...patch,
  };
}

describe("projectAgentWorkflow", () => {
  it("将旧 reasoning、Skill 和 ToolTrace 合并为统一流程", () => {
    const projection = projectAgentWorkflow(message({
      reasoning: "原始 reasoning",
      activatedSkills: [{
        id: "research",
        name: "研究",
        version: "1.0.0",
        contentHash: "hash",
        activation: "model",
      }],
      toolTraces: [{
        callId: "call-1",
        name: "read_pdf_pages",
        status: "completed",
        risk: "conversationRead",
        argumentSummary: "{\"pages\":[1]}",
      }],
    }));

    expect(projection.status).toBe("completed");
    expect(projection.steps.map((step) => step.kind)).toEqual([
      "reasoning",
      "skill",
      "tool",
    ]);
    expect(projection.summary).toMatchObject({
      stepCount: 3,
      toolCallCount: 1,
      skillCount: 1,
      durationMs: 800,
    });
  });

  it("同一个 callId 只生成一个最终状态节点", () => {
    const projection = projectAgentWorkflow(message({
      toolTraces: [{
        callId: "call-1",
        name: "memory_read",
        status: "failed",
        risk: "memoryRead",
        argumentSummary: "{}",
      }],
    }));
    expect(projection.steps.filter((step) => step.kind === "tool")).toHaveLength(1);
    expect(projection.steps.find((step) => step.kind === "tool")?.status).toBe("failed");
  });

  it("审批、失败与停止保持为需要处理的展开状态", () => {
    const approval = projectAgentWorkflow(message({
      status: "streaming",
      content: "",
      toolTraces: [{
        callId: "call-1",
        name: "memory_modify",
        status: "awaitingApproval",
        risk: "memoryWrite",
        argumentSummary: "{}",
      }],
    }), { streaming: true });
    expect(approval.status).toBe("waitingApproval");
    expect(approval.needsAttention).toBe(true);
    expect(projectAgentWorkflow(message({ status: "error" })).needsAttention).toBe(true);
    expect(projectAgentWorkflow(message({ status: "stopped" })).needsAttention).toBe(true);
  });

  it("仅让运行中或需要处理的助手工作流自动展开", () => {
    expect(agentWorkflowNeedsAttention(message({ status: "completed" }))).toBe(false);
    expect(agentWorkflowNeedsAttention(message({ status: "error", reasoning: "失败前的思考" }))).toBe(true);
    expect(agentWorkflowNeedsAttention(message({ status: "stopped", reasoning: "停止前的思考" }))).toBe(true);
    expect(agentWorkflowNeedsAttention(message({ status: "pending", content: "" }))).toBe(false);
    expect(agentWorkflowNeedsAttention(message({ status: "pending", content: "", reasoning: "分析中" }))).toBe(true);
    expect(agentWorkflowNeedsAttention(message({ role: "user", status: "error", reasoning: "不可见" }))).toBe(false);
  });

  it("普通聊天没有真实过程事件时不生成虚拟步骤", () => {
    const projection = projectAgentWorkflow(message());
    expect(projection.steps).toEqual([]);
    expect(projection.summary).toMatchObject({
      stepCount: 0,
      toolCallCount: 0,
      skillCount: 0,
    });
  });
  it("labels OpenAI Responses reasoning as a provider summary", () => {
    const projection = projectAgentWorkflow(message({
      reasoning: "**Explaining the transaction model**",
      modelSnapshot: {
        id: "gpt-5",
        providerId: "openai",
        displayName: "GPT-5",
        apiModel: "gpt-5",
        protocol: "openAiResponses",
        providerName: "OpenAI",
      },
    }), { language: "en" });

    expect(projection.steps[0]).toMatchObject({
      title: "Reasoning summary",
      reasoningLabel: "summary",
    });
  });
});
