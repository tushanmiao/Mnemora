import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../../types/chat";
import {
  activeMessageNavigatorNodeId,
  buildMessageNavigatorNodes,
} from "./messageNavigator";

function message(id: string, role: "user" | "assistant", content: string): ChatMessage {
  return {
    id,
    conversationId: "conversation",
    role,
    content,
    status: "completed",
    createdAt: 1,
    updatedAt: 1,
  };
}

describe("message navigator", () => {
  it("maps each user turn to its virtual render index", () => {
    const nodes = buildMessageNavigatorNodes([
      message("u1", "user", "first question"),
      message("a1", "assistant", "first answer"),
      message("u2", "user", "second question"),
      message("a2", "assistant", "second answer"),
    ]);

    expect(nodes.map((node) => node.targetRenderIndex)).toEqual([0, 2]);
    expect(nodes[0].answerPreview).toBe("first answer");
    expect(activeMessageNavigatorNodeId(nodes, 0)).toBe("turn-u1");
    expect(activeMessageNavigatorNodeId(nodes, 1)).toBe("turn-u1");
    expect(activeMessageNavigatorNodeId(nodes, 2)).toBe("turn-u2");
  });
});
