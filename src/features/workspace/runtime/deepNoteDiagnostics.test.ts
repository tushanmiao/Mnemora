import { describe, expect, it } from "vitest";
import type { DeepNoteRunDetail, NotePipelineActivity } from "../../chat/api/notePipeline";
import {
  buildDeepNoteWorkflow,
  describeNotePipelineEvent,
  diagnoseDeepNoteRuntime,
} from "./deepNoteDiagnostics";

function detail(overrides: Partial<DeepNoteRunDetail> = {}): DeepNoteRunDetail {
  return {
    run: {
      id: "run-1", conversationId: "conversation-1", noteId: null, phase: "analyzing",
      outlineJson: "", selectedSectionIds: [], providerId: "provider-1", modelId: "model-1",
      maxOutputTokens: 8_192, thinkingEnabled: false, retryAttempts: 5, inputSnapshotHash: "hash",
      currentPlanVersion: 0, executionVersion: 0, budgetJson: "{}", preflightJson: "{}",
      sidecarJson: "{}", idempotencyKey: "key", completedSectionIds: [], failedSectionIds: [],
      warnings: [], errorMessage: null, createdAt: 1, updatedAt: 1,
    },
    preflight: null,
    inputSnapshot: null,
    planVersion: null,
    budget: { semanticCallLimit: 12, semanticCallsUsed: 0, nodeAttemptLimit: 5, sectionRevisionLimit: 5, replanLimit: 4, replansUsed: 0, maxParallelNodes: 2 },
    contextBudget: { contextWindowTokens: 128_000, estimatedInputTokens: 58_000, plannerOutputReserveTokens: 4_096, promptOverheadTokens: 4_096, safetyMarginTokens: 8_000, usableInputTokens: 100_000, directInputLimitTokens: 24_000, chunkTargetTokens: 16_000, chunkCount: 5, processedChunkCount: 5, totalMessageCount: 24, processedMessageCount: 24, coverageComplete: true, omittedMessageIds: [] },
    sourceChunkCount: 5, nodes: [], sections: [], sourceChunks: [], evidence: [], ledger: {}, events: [], markdownPreview: "", sidecarJson: "{}",
    ...overrides,
  };
}

describe("deep note diagnostics", () => {
  it("separates completed input coverage from active outline planning", () => {
    const value = detail({
      preflight: {
        ready: true,
        model: { providerId: "provider-1", modelId: "model-1", apiModel: "model-1", contextWindowTokens: 128_000, capabilities: { tools: true, vision: null, reasoning: null, structuredOutputs: true } },
        requiresTools: true, requiresVision: false, missingCapabilities: [], warnings: [], attachmentIds: [],
      },
      inputSnapshot: { messageIds: Array.from({ length: 24 }, (_, index) => `m-${index}`), attachmentIds: [], createdAt: 1 },
    });
    const workflow = buildDeepNoteWorkflow(value, "analyzing");
    expect(workflow.find((step) => step.id === "context")?.status).toBe("completed");
    expect(workflow.find((step) => step.id === "planning")?.status).toBe("active");
    expect(workflow.find((step) => step.id === "execution")?.status).toBe("pending");
  });

  it("reports the active model timeout window instead of calling it stuck", () => {
    const activity: NotePipelineActivity = {
      kind: "modelCall", callId: "call-1", operation: "deepNote", attempt: 1,
      maxRetries: 5, startedAt: 10_000, timeoutMs: 240_000, delayMs: null, lastError: null,
    };
    const diagnosis = diagnoseDeepNoteRuntime("analyzing", activity, 10_000, 18_000);
    expect(diagnosis.title).toContain("第 1/6 次");
    expect(diagnosis.elapsedSeconds).toBe(8);
    expect(diagnosis.timeoutSeconds).toBe(232);
  });

  it("explains the bounded stopping state and forced-stop fallback", () => {
    const diagnosis = diagnoseDeepNoteRuntime("cancelling", null, 10_000, 16_000);
    expect(diagnosis.title).toBe("停止请求已发送");
    expect(diagnosis.detail).toContain("强制终止");

    const event = describeNotePipelineEvent({
      sequence: 8,
      eventType: "runCancelled",
      nodeId: null,
      payloadJson: JSON.stringify({
        forced: true,
        reason: "forced-after-cancellation-timeout",
        diagnosticPath: "C:/logs/task-diagnostics.jsonl",
      }),
      createdAt: 26_000,
    });
    expect(event.label).toBe("任务已强制停止");
    expect(event.detail).toContain("task-diagnostics.jsonl");
  });

  it("turns persisted model completion data into a readable log entry", () => {
    const event = describeNotePipelineEvent({
      sequence: 3,
      eventType: "modelCallCompleted",
      nodeId: null,
      payloadJson: JSON.stringify({
        operation: "deepNoteOutline", durationMs: 12_400, inputChars: 8_000,
        responseChars: 3_200, maxOutputTokens: 4_096,
      }),
      createdAt: 20_000,
    });
    expect(event.label).toBe("知识账本汇总提纲完成");
    expect(event.detail).toContain("12 秒");
    expect(event.detail).toContain("输入 8000 字符");
    expect(event.detail).toContain("3200 字符");
    expect(event.detail).toContain("4096 Token");
  });

  it("keeps legacy model events readable without invented zero metrics", () => {
    const event = describeNotePipelineEvent({
      sequence: 4,
      eventType: "modelCallCompleted",
      nodeId: null,
      payloadJson: JSON.stringify({ durationMs: 2_000, responseChars: 180 }),
      createdAt: 22_000,
    });
    expect(event.detail).toBe("耗时 2 秒 · 返回 180 字符");
    expect(event.detail).not.toContain("输入 0");
    expect(event.detail).not.toContain("输出上限 0");
  });

  it("explains continue, retry, and restart recovery events", () => {
    const continued = describeNotePipelineEvent({
      sequence: 5, eventType: "runContinued", nodeId: null,
      payloadJson: JSON.stringify({ executionVersion: 2, resetFailedSections: false }),
      createdAt: 23_000,
    });
    const retried = describeNotePipelineEvent({
      sequence: 6, eventType: "runRetryRequested", nodeId: null,
      payloadJson: JSON.stringify({ executionVersion: 3, resetFailedSections: true }),
      createdAt: 24_000,
    });
    const restarted = describeNotePipelineEvent({
      sequence: 7, eventType: "runRestarted", nodeId: null,
      payloadJson: JSON.stringify({ newRunId: "run-2" }),
      createdAt: 25_000,
    });

    expect(continued).toEqual({ label: "已从停止点继续", detail: "恢复执行版本 v2，保留已有检查点" });
    expect(retried).toEqual({ label: "已重试失败步骤", detail: "恢复执行版本 v3，失败章节和节点已重置" });
    expect(restarted).toEqual({ label: "已重新生成", detail: "新任务 run-2" });
  });
});
