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
    contextBudget: { contextWindowTokens: 128_000, estimatedInputTokens: 58_000, plannerOutputReserveTokens: 8_192, promptOverheadTokens: 4_096, safetyMarginTokens: 8_000, usableInputTokens: 100_000, directInputLimitTokens: 24_000, chunkTargetTokens: 16_000, chunkCount: 5, processedChunkCount: 5, totalMessageCount: 24, processedMessageCount: 24, coverageComplete: true, omittedMessageIds: [] },
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

  it("turns persisted model completion data into a readable log entry", () => {
    const event = describeNotePipelineEvent({
      sequence: 3,
      eventType: "modelCallCompleted",
      nodeId: null,
      payloadJson: JSON.stringify({ durationMs: 12_400, responseChars: 3_200 }),
      createdAt: 20_000,
    });
    expect(event.label).toBe("模型请求完成");
    expect(event.detail).toContain("12 秒");
    expect(event.detail).toContain("3200 字符");
  });
});
