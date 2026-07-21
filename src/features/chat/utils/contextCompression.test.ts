import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../../types/chat";
import { toModelMessages } from "./contextCompression";

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
});
