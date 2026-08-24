import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../../types/chat";
import type { DeepNoteRunDetail } from "../../chat/api/notePipeline";
import type { AgentRunSnapshot } from "../../chat/api/chat";
import {
  projectChatTaskRun,
  projectDeepNoteTaskRun,
  sortTaskRuns,
} from "./taskRunProjection";

function assistant(patch: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: "assistant-1",
    conversationId: "conversation-1",
    role: "assistant",
    content: "最终回答",
    status: "completed",
    createdAt: 1_000,
    updatedAt: 2_000,
    ...patch,
  };
}

function deepNoteDetail(): DeepNoteRunDetail {
  return {
    run: {
      id: "run-1",
      conversationId: "conversation-1",
      noteId: null,
      phase: "analyzing",
      outlineJson: "",
      selectedSectionIds: [],
      providerId: "provider-1",
      modelId: "model-1",
      maxOutputTokens: 8_000,
      thinkingEnabled: true,
      retryAttempts: 5,
      inputSnapshotHash: "hash",
      currentPlanVersion: 1,
      executionVersion: 1,
      budgetJson: "{}",
      preflightJson: "{}",
      sidecarJson: "{}",
      idempotencyKey: "key",
      completedSectionIds: [],
      failedSectionIds: [],
      warnings: [],
      errorMessage: null,
      createdAt: 1_000,
      updatedAt: 2_000,
    },
    preflight: {
      ready: true,
      model: {
        providerId: "provider-1",
        modelId: "model-1",
        apiModel: "model",
        contextWindowTokens: 128_000,
        capabilities: { tools: true, vision: true, reasoning: true, structuredOutputs: true },
      },
      requiresTools: true,
      requiresVision: false,
      missingCapabilities: [],
      warnings: [],
      attachmentIds: [],
    },
    inputSnapshot: { messageIds: ["m1", "m2"], attachmentIds: [], createdAt: 1_100 },
    planVersion: null,
    budget: {
      semanticCallLimit: 12,
      semanticCallsUsed: 2,
      nodeAttemptLimit: 5,
      sectionRevisionLimit: 5,
      replanLimit: 5,
      replansUsed: 0,
      maxParallelNodes: 2,
      maxParallelChunks: 2,
    },
    contextBudget: {
      contextWindowTokens: 128_000,
      estimatedInputTokens: 2_000,
      plannerOutputReserveTokens: 8_000,
      promptOverheadTokens: 1_000,
      safetyMarginTokens: 4_000,
      usableInputTokens: 115_000,
      directInputLimitTokens: 96_000,
      chunkTargetTokens: 16_000,
      chunkCount: 1,
      processedChunkCount: 1,
      totalMessageCount: 2,
      processedMessageCount: 2,
      coverageComplete: true,
      omittedMessageIds: [],
    },
    sourceChunkCount: 1,
    nodes: [],
    sections: [],
    sourceChunks: [],
    evidence: [],
    ledger: {},
    events: [],
    markdownPreview: "",
    sidecarJson: "{}",
  };
}

describe("taskRunProjection", () => {
  it("只在 Chat 出现真实 reasoning、Skill 或 Tool 活动时建立任务", () => {
    expect(projectChatTaskRun(assistant(), "", false, "zh", 2_000)).toBeNull();
    const task = projectChatTaskRun(assistant({ reasoning: "分析内容" }), "分析内容", false, "zh", 2_000);
    expect(task).toMatchObject({
      kind: "chatAgent",
      status: "completed",
      completedCount: 1,
      totalCount: 1,
    });
    expect(task?.steps[0]).toMatchObject({ kind: "reasoning", content: "分析内容" });
  });

  it("完成后的 Chat 任务在重新进入会话后仍可从最后一条消息恢复", () => {
    const task = projectChatTaskRun(
      assistant({
        reasoning: "已完成的思考摘要",
        updatedAt: 2_000,
        workflowSummary: {
          status: "completed",
          stepCount: 1,
          toolCallCount: 0,
          skillCount: 0,
          durationMs: 1_000,
        },
      }),
      "已完成的思考摘要",
      false,
      "zh",
      24 * 60 * 60 * 1_000,
    );

    expect(task).toMatchObject({ status: "completed", sourceId: "assistant-1" });
  });

  it("把等待审批的 Tool 映射为需要处理的任务", () => {
    const task = projectChatTaskRun(assistant({
      status: "streaming",
      toolTraces: [{
        callId: "call-1",
        name: "memory_modify",
        status: "awaitingApproval",
        risk: "memoryWrite",
        argumentSummary: "更新长期记忆",
      }],
    }), "", true, "zh", 2_000);
    expect(task).toMatchObject({ status: "waiting", needsAttention: true });
    expect(task?.steps[0]).toMatchObject({ kind: "tool", status: "waiting" });
  });

  it("以后端 Agent Run 快照覆盖消息中的过期状态", () => {
    const snapshot: AgentRunSnapshot = {
      id: "run-1",
      conversationId: "conversation-1",
      messageId: "assistant-1",
      state: "stopping",
      activity: "idle",
      stateVersion: 4,
      executionVersion: 1,
      runtimeInstanceId: "runtime-1",
      modelId: "model-1",
      errorCode: null,
      errorMessage: null,
      heartbeatAt: 2_100,
      createdAt: 1_000,
      updatedAt: 2_100,
      finishedAt: null,
      toolCalls: [],
    };
    const task = projectChatTaskRun(
      assistant({ status: "streaming", agentRunId: "run-1" }),
      "",
      true,
      "zh",
      2_200,
      snapshot,
    );
    expect(task).toMatchObject({ status: "stopping", updatedAt: 2_100, canStop: true });
  });

  it("回答完成但工具失败时保留完成事实并明确提示异常", () => {
    const task = projectChatTaskRun(assistant({
      toolTraces: [
        {
          callId: "call-success",
          name: "search_skills",
          status: "completed",
          risk: "builtinRead",
          argumentSummary: "{}",
        },
        {
          callId: "call-failed",
          name: "inspect_skill",
          status: "failed",
          risk: "builtinRead",
          argumentSummary: "{}",
          preview: "工具不在白名单中",
        },
      ],
    }), "", false, "zh", 2_000);

    expect(task).toMatchObject({
      status: "completed",
      statusLabel: "回答完成，但工具有失败",
      currentStepLabel: "回答完成，但工具有失败",
      activity: "回答已完成，但 1 个工具调用失败",
      needsAttention: true,
      completedCount: 1,
      totalCount: 2,
    });
  });

  it("深度笔记上下文覆盖完成后准确定位在知识结构阶段", () => {
    const detail = deepNoteDetail();
    const task = projectDeepNoteTaskRun({
      detail,
      progress: {
        runId: "run-1",
        phase: "analyzing",
        current: null,
        total: null,
        message: "正在等待模型返回知识结构",
        updatedAt: 2_000,
        terminal: false,
        degraded: false,
      },
    }, "zh", 2_000);
    expect(task).toMatchObject({
      status: "running",
      currentStepLabel: "生成知识结构与提纲",
      completedCount: 2,
      totalCount: 6,
    });
  });

  it("多任务排序优先展示需要处理的任务，其次展示运行任务", () => {
    const detail = deepNoteDetail();
    detail.run.phase = "paused";
    const paused = projectDeepNoteTaskRun({ detail, progress: null }, "zh", 2_000)!;
    const running = projectChatTaskRun(assistant({
      status: "streaming",
      reasoning: "分析",
    }), "分析", true, "zh", 2_000)!;
    expect(sortTaskRuns([running, paused]).map((task) => task.status)).toEqual(["paused", "running"]);
  });

  it("为失败的深度笔记提供步骤重试和完整重生成", () => {
    const detail = deepNoteDetail();
    detail.run.phase = "error";
    detail.run.errorMessage = "模型请求超时";
    const task = projectDeepNoteTaskRun({ detail, progress: null }, "zh", 2_000);

    expect(task).toMatchObject({
      status: "failed",
      canResume: false,
      canRetry: true,
      canRestart: true,
      canAbandon: true,
    });
  });

  it("为已停止的深度笔记提供检查点继续和完整重生成", () => {
    const detail = deepNoteDetail();
    detail.run.phase = "cancelled";
    const task = projectDeepNoteTaskRun({ detail, progress: null }, "zh", 2_000);

    expect(task).toMatchObject({
      status: "stopped",
      canResume: true,
      canRetry: false,
      canRestart: true,
      canAbandon: true,
    });
  });

  it("停止中的任务保持逃生入口但不能继续或重新生成", () => {
    const detail = deepNoteDetail();
    detail.run.phase = "cancelling";
    const task = projectDeepNoteTaskRun({ detail, progress: null }, "zh", 2_000);

    expect(task).toMatchObject({
      status: "stopping",
      statusLabel: "正在停止",
      canStop: true,
      canAbandon: true,
      canResume: false,
      canRestart: false,
      needsAttention: true,
    });
  });

  it("已遗弃的任务不允许任何恢复或再次遗弃操作", () => {
    const detail = deepNoteDetail();
    detail.run.abandoned = true;
    detail.run.phase = "cancelled";
    const task = projectDeepNoteTaskRun({ detail, progress: null }, "zh", 2_000);

    expect(task).toMatchObject({
      status: "abandoned",
      canResume: false,
      canRetry: false,
      canRestart: false,
      canStop: false,
      canAbandon: false,
    });
  });
});
