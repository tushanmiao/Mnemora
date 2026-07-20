import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  appendStreamingDelta,
  appendStreamingReasoningDelta,
  consumeStreamingMessage,
  discardStreamingMessage,
  resetAllStreamingMessages,
  startStreamingMessage,
} from "./streamingStore";

describe("streamingStore lifecycle", () => {
  let nextFrameId = 0;
  let callbacks: Map<number, FrameRequestCallback>;
  let cancelFrame: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    callbacks = new Map();
    nextFrameId = 0;
    cancelFrame = vi.fn((frameId: number) => callbacks.delete(frameId));
    vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => {
      const frameId = ++nextFrameId;
      callbacks.set(frameId, callback);
      return frameId;
    }));
    vi.stubGlobal("cancelAnimationFrame", cancelFrame);
  });

  afterEach(() => {
    resetAllStreamingMessages();
    vi.unstubAllGlobals();
  });

  it("consumes pending text and removes the entry", () => {
    startStreamingMessage("message-1");
    appendStreamingReasoningDelta("message-1", "plan");
    appendStreamingDelta("message-1", "answer");

    expect(consumeStreamingMessage("message-1")).toEqual({
      content: "answer",
      reasoning: "plan",
    });
    expect(consumeStreamingMessage("message-1")).toBeNull();
    expect(cancelFrame).toHaveBeenCalled();
  });

  it("discards one entry without affecting another", () => {
    startStreamingMessage("discarded");
    startStreamingMessage("kept");
    appendStreamingDelta("discarded", "old");
    appendStreamingDelta("kept", "new");

    discardStreamingMessage("discarded");

    expect(consumeStreamingMessage("discarded")).toBeNull();
    expect(consumeStreamingMessage("kept")?.content).toBe("new");
  });

  it("cancels the previous frame when the same message restarts", () => {
    startStreamingMessage("message-1");
    appendStreamingDelta("message-1", "stale");
    startStreamingMessage("message-1");

    expect(cancelFrame).toHaveBeenCalledTimes(1);
    expect(consumeStreamingMessage("message-1")).toEqual({ content: "", reasoning: "" });
  });

  it("resets every active entry", () => {
    startStreamingMessage("message-1");
    startStreamingMessage("message-2");
    appendStreamingDelta("message-1", "one");
    appendStreamingDelta("message-2", "two");

    resetAllStreamingMessages();

    expect(consumeStreamingMessage("message-1")).toBeNull();
    expect(consumeStreamingMessage("message-2")).toBeNull();
    expect(cancelFrame).toHaveBeenCalledTimes(2);
  });
});
