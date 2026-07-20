import { describe, expect, it } from "vitest";
import type { Conversation } from "../../../types/conversation";
import {
  estimateConversationTextBytes,
  trimConversationCache,
} from "./conversationCache";

function conversation(id: string, content = id): Conversation {
  return {
    id,
    title: id,
    messages: [{
      id: `message-${id}`,
      conversationId: id,
      role: "user",
      content,
      status: "completed",
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
    permissionMode: "askSensitive",
    projectId: null,
    collectionId: null,
    pinned: false,
    createdAt: 1,
    updatedAt: 1,
  };
}

describe("trimConversationCache", () => {
  it("keeps the current conversation and applies the count limit", () => {
    const candidates = Array.from({ length: 5 }, (_, index) => conversation(`c${index}`));
    const result = trimConversationCache(candidates, {
      currentConversationId: "c0",
      protectedConversationIds: new Set(),
      maxCount: 3,
      maxTextBytes: Number.MAX_SAFE_INTEGER,
    });

    expect(result.map((item) => item.id)).toEqual(["c0", "c1", "c2"]);
  });

  it("allows required conversations to exceed the normal count budget", () => {
    const candidates = [conversation("current"), conversation("recent"), conversation("running")];
    const result = trimConversationCache(candidates, {
      currentConversationId: "current",
      protectedConversationIds: new Set(["running"]),
      maxCount: 1,
      maxTextBytes: 1,
    });

    expect(result.map((item) => item.id)).toEqual(["current", "running"]);
  });

  it("does not add an optional conversation beyond the text budget", () => {
    const current = conversation("current", "a".repeat(200));
    const optional = conversation("optional", "b".repeat(200));
    const result = trimConversationCache([current, optional], {
      currentConversationId: "current",
      protectedConversationIds: new Set(),
      maxCount: 4,
      maxTextBytes: estimateConversationTextBytes(current),
    });

    expect(result.map((item) => item.id)).toEqual(["current"]);
  });
});
