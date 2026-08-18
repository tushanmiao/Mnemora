import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../../types/chat";
import {
  compressionTranscript,
  compressionTranscriptBatches,
  contextInputBudget,
  toModelMessages,
} from "./contextCompression";
import { estimateTextTokens } from "./contextUsage";

describe("toModelMessages", () => {
  it("keeps attachment-only user messages", () => {
    const message: ChatMessage = {
      id: "message-1",
      conversationId: "conversation-1",
      role: "user",
      content: "",
      attachments: [{
        id: "attachment-1",
        kind: "image",
        name: "capture.png",
        mimeType: "image/png",
        sizeBytes: 128,
        path: "attachment-1_capture.png",
      }],
      status: "completed",
      createdAt: 1,
      updatedAt: 1,
    };

    expect(toModelMessages([message])).toEqual([{
      role: "user",
      content: "",
      attachments: message.attachments,
    }]);
  });

  it("adds structured literature references to model context and compression", () => {
    const message: ChatMessage = {
      id: "message-2",
      conversationId: "conversation-1",
      role: "user",
      content: "这个结论可靠吗？",
      literatureReferences: [{
        id: "reference-1",
        libraryItemId: "item-1",
        title: "Reliable Paper",
        pageIndex: 2,
        kind: "selection",
        text: "The reported improvement is statistically significant.",
      }],
      status: "completed",
      createdAt: 1,
      updatedAt: 1,
    };

    const modelMessage = toModelMessages([message])[0];
    expect(modelMessage.content).toContain("Reliable Paper");
    expect(modelMessage.content).toContain("第 3 页");
    expect(modelMessage.content).toContain("用户问题：\n这个结论可靠吗？");
    expect(compressionTranscript("", [message])).toContain("Reliable Paper，第 3 页");
  });

  it("adds structured note references without flattening them into system instructions", () => {
    const message: ChatMessage = {
      id: "message-note",
      conversationId: "conversation-1",
      role: "user",
      content: "解释这一段",
      noteReferences: [{
        id: "note-reference-1",
        noteId: "note-1",
        noteTitle: "学习笔记",
        revisionHash: "revision-1",
        startLine: 4,
        endLine: 6,
        selectedText: "这是用户选择的笔记正文。",
      }],
      status: "completed",
      createdAt: 1,
      updatedAt: 1,
    };
    const modelMessage = toModelMessages([message])[0];
    expect(modelMessage.content).toContain("[笔记引用 1]");
    expect(modelMessage.content).toContain("用户问题：\n解释这一段");
    expect(compressionTranscript("", [message])).toContain("学习笔记");
  });
});

describe("context compression budget", () => {
  it("reserves configured output and the larger safety margin", () => {
    expect(contextInputBudget(128_000, 8_192)).toBe(109_568);
    expect(contextInputBudget(8_000, 4_096)).toBe(0);
  });

  it("splits one oversized message without dropping its content", () => {
    const content = "x".repeat(700);
    const message: ChatMessage = {
      id: "long-message",
      conversationId: "conversation-1",
      role: "user",
      content,
      status: "completed",
      createdAt: 1,
      updatedAt: 1,
    };
    const batches = compressionTranscriptBatches([message], 64);
    expect(batches.length).toBeGreaterThan(1);
    expect(batches.every((batch) => estimateTextTokens(batch) <= 64)).toBe(true);
    expect((batches.join("").match(/x/g) ?? []).length).toBe(content.length);
  });
});
