import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../../types/chat";
import { estimateConversationContext } from "./contextUsage";

function message(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: "message-1",
    conversationId: "conversation-1",
    role: "user",
    content: "",
    status: "completed",
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("estimateConversationContext", () => {
  it("includes image attachment budget in local estimates", () => {
    const estimate = estimateConversationContext([
      message({
        attachments: [{
          id: "attachment-1",
          kind: "image",
          name: "capture.png",
          mimeType: "image/png",
          sizeBytes: 128,
          path: "attachment-1_capture.png",
        }],
      }),
    ], "");

    expect(estimate.source).toBe("estimated");
    expect(estimate.tokens).toBe(1_200);
  });

  it("uses dimensions for the active image and does not resend historical image bodies", () => {
    const attachment = {
      id: "attachment-1",
      kind: "image" as const,
      name: "capture.png",
      mimeType: "image/png",
      sizeBytes: 128,
      path: "attachment-1_capture.png",
      width: 1_024,
      height: 768,
    };
    const estimate = estimateConversationContext([
      message({ id: "old", attachments: [attachment] }),
      message({ id: "new", attachments: [{ ...attachment, id: "attachment-2" }] }),
    ], "");

    expect(estimate.tokens).toBe(40 + 765);
  });

  it("counts explicitly included PDF reference text", () => {
    const withoutReference = estimateConversationContext([
      message({ content: "解释这一段" }),
    ], "");
    const withReference = estimateConversationContext([
      message({
        content: "解释这一段",
        literatureReferences: [{
          id: "reference-1",
          libraryItemId: "item-1",
          title: "Paper",
          pageIndex: 0,
          kind: "selection",
          text: "A".repeat(400),
        }],
      }),
    ], "");

    expect(withReference.tokens).toBeGreaterThan(withoutReference.tokens + 90);
  });
});
