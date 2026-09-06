import { describe, expect, it } from "vitest";
import type { Conversation } from "../../../types/conversation";
import type { ToolTrace, ToolTraceStatus } from "../../../types/chat";
import { pendingApprovalConversationIds } from "./pendingApprovals";

function trace(status: ToolTraceStatus, approvalId?: string): ToolTrace {
  return {
    callId: `call-${status}`,
    name: "note_update",
    status,
    risk: "noteWrite",
    argumentSummary: "{}",
    approvalId,
  };
}

function conversation(id: string, traces: ToolTrace[]): Conversation {
  return {
    id,
    title: id,
    messages: [{
      id: `message-${id}`,
      conversationId: id,
      role: "assistant",
      content: "",
      status: "completed",
      toolTraces: traces,
      createdAt: 1,
      updatedAt: 1,
    }],
    assistantId: null,
    providerId: null,
    modelId: null,
    systemPrompt: "",
    contextSummary: "",
    compressedUntilMessageId: null,
    contextCompressionCount: 0,
    enabledSkillIds: [],
    linkedLibraryItemIds: [],
    permissionMode: "askSensitive",
    projectId: null,
    collectionId: null,
    pinned: false,
    createdAt: 1,
    updatedAt: 1,
  };
}

describe("pendingApprovalConversationIds", () => {
  it("finds conversations that are still waiting on the user", () => {
    const result = pendingApprovalConversationIds([
      conversation("waiting", [trace("awaitingApproval", "approval-1")]),
      conversation("running", [trace("running")]),
    ]);
    expect(result).toEqual(["waiting"]);
  });

  it("ignores interrupts the backend already closed", () => {
    // 等待结束时后端用同一个 callId 再发一个 toolTrace，前端归约器会清掉 approvalId。
    // 判据必须同时看 status 和 approvalId，否则超时/取消后的轨迹会被当成仍在等待。
    const result = pendingApprovalConversationIds([
      conversation("timed-out", [trace("timedOut", "approval-1")]),
      conversation("stale", [trace("awaitingApproval")]),
      conversation("approved", [trace("approved", "approval-2")]),
    ]);
    expect(result).toEqual([]);
  });

  it("returns an empty list when nothing is cached", () => {
    expect(pendingApprovalConversationIds([])).toEqual([]);
  });
});
