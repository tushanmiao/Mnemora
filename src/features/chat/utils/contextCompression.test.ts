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
      failed: false,
    }]);
  });

  it("keeps a placeholder for failed turns so consecutive user messages never pile up", () => {
    // 回归：连续两次上游 429 之后，若把失败的助手消息整条丢掉，请求尾部会出现三条
    // 连续的用户消息、三个都没被回答的问题，模型可能挑其中信息量最大的那一条作答，
    // 而不是用户真正在等的最后一条。
    const base = { conversationId: "conversation-1", createdAt: 1, updatedAt: 1 } as const;
    const messages: ChatMessage[] = [
      { ...base, id: "u1", role: "user", content: "分析这个 md 文件", status: "completed" },
      {
        ...base,
        id: "a1",
        role: "assistant",
        content: "",
        status: "error",
        errorMessage: "HTTP 429：账户并发额度已用尽",
      },
      { ...base, id: "u2", role: "user", content: "怎么理解这部分", status: "completed" },
      { ...base, id: "a2", role: "assistant", content: "", status: "error" },
      { ...base, id: "u3", role: "user", content: "识别网址", status: "completed" },
    ];

    const sent = toModelMessages(messages);
    expect(sent.map((message) => message.role)).toEqual([
      "user",
      "assistant",
      "user",
      "assistant",
      "user",
    ]);
    expect(sent[1].failed).toBe(true);
    expect(sent[1].content).toContain("HTTP 429");
    expect(sent[1].content).toContain("随后发送了新的消息");
    expect(sent[3].failed).toBe(true);
    expect(sent[3].content).toBe("（这一轮回复没有产生回答。用户没有等待重试，随后发送了新的消息。）");
    // 最后一条用户消息在结构上是唯一的当前请求。
    expect(sent[4]).toEqual({ role: "user", content: "识别网址", attachments: [], failed: false });
  });

  it("does not claim a new request when the failed turn is the last one", () => {
    const base = { conversationId: "conversation-1", createdAt: 1, updatedAt: 1 } as const;
    const messages: ChatMessage[] = [
      { ...base, id: "u1", role: "user", content: "识别网址", status: "completed" },
      { ...base, id: "a1", role: "assistant", content: "", status: "error", errorMessage: "HTTP 500。" },
    ];
    const sent = toModelMessages(messages);
    // 上游报错自带的句末标点要去掉，否则拼出「HTTP 500。。」。
    expect(sent[1].content).toBe("（这一轮回复失败，没有产生回答：HTTP 500。）");
    expect(sent[1].content).not.toContain("随后发送");
  });

  it("keeps partial content of an interrupted turn and still marks it undelivered", () => {
    const message: ChatMessage = {
      id: "stopped-1",
      conversationId: "conversation-1",
      role: "assistant",
      content: "我先看一下这张图",
      status: "stopped",
      createdAt: 1,
      updatedAt: 1,
    };
    const [sent] = toModelMessages([message]);
    expect(sent.failed).toBe(true);
    expect(sent.content).toContain("我先看一下这张图");
    expect(sent.content).toContain("被中断");
  });

  it("excludes the in-flight assistant placeholder", () => {
    const base = { conversationId: "conversation-1", createdAt: 1, updatedAt: 1 } as const;
    const messages: ChatMessage[] = [
      { ...base, id: "u1", role: "user", content: "识别网址", status: "completed" },
      { ...base, id: "a1", role: "assistant", content: "", status: "pending" },
      { ...base, id: "a2", role: "assistant", content: "写到一半", status: "streaming" },
    ];
    expect(toModelMessages(messages).map((message) => message.role)).toEqual(["user"]);
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
