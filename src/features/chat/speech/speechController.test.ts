import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { speechController } from "./speechController";
import { splitSpeechText } from "./speechText";

class FakeUtterance {
  readonly text: string;
  lang = "";
  rate = 0;
  onstart: (() => void) | null = null;
  onend: (() => void) | null = null;
  onerror: ((event: { error: string }) => void) | null = null;

  constructor(text: string) {
    this.text = text;
  }
}

describe("speech controller", () => {
  let utterances: FakeUtterance[];
  let synthesis: {
    speak: ReturnType<typeof vi.fn>;
    cancel: ReturnType<typeof vi.fn>;
    pause: ReturnType<typeof vi.fn>;
    resume: ReturnType<typeof vi.fn>;
  };

  beforeEach(() => {
    utterances = [];
    synthesis = {
      speak: vi.fn((utterance: FakeUtterance) => {
        utterances.push(utterance);
        utterance.onstart?.();
      }),
      cancel: vi.fn(),
      pause: vi.fn(),
      resume: vi.fn(),
    };
    vi.stubGlobal("window", { speechSynthesis: synthesis });
    vi.stubGlobal("SpeechSynthesisUtterance", FakeUtterance);
    speechController.stop();
  });

  afterEach(() => {
    speechController.stop();
    vi.unstubAllGlobals();
  });

  it("speaks chunks in order and returns to idle after the final chunk", () => {
    const text = "第一句。".repeat(180);
    const expectedChunkCount = splitSpeechText(text).length;
    expect(expectedChunkCount).toBeGreaterThan(1);

    speechController.speak({
      messageId: "message-1",
      source: "message",
      text,
    });

    expect(synthesis.speak).toHaveBeenCalledTimes(1);
    expect(speechController.getSnapshot().status).toBe("speaking");

    for (let index = 0; index < expectedChunkCount; index += 1) {
      utterances[index].onend?.();
      if (index + 1 < expectedChunkCount) {
        expect(synthesis.speak).toHaveBeenCalledTimes(index + 2);
      }
    }

    expect(speechController.getSnapshot().status).toBe("idle");
  });

  it("pauses, resumes, and stops the active target", () => {
    speechController.speak({ messageId: "message-2", source: "selection", text: "hello" });
    speechController.pause();
    expect(speechController.getSnapshot().status).toBe("paused");
    expect(synthesis.pause).toHaveBeenCalledOnce();
    speechController.resume();
    expect(speechController.getSnapshot().status).toBe("speaking");
    expect(synthesis.resume).toHaveBeenCalledOnce();
    speechController.stop();
    expect(speechController.getSnapshot().status).toBe("idle");
    expect(synthesis.cancel).toHaveBeenCalled();
  });

  it("keeps an actionable error for unsupported or empty content", () => {
    vi.stubGlobal("window", {});
    const unsupported = speechController.speak({ messageId: "message-3", source: "message", text: "hello" });
    expect(unsupported).toBe(false);
    expect(speechController.getSnapshot().status).toBe("error");
    expect(speechController.getSnapshot().target?.messageId).toBe("message-3");

    vi.stubGlobal("window", { speechSynthesis: synthesis });
    speechController.speak({ messageId: "message-4", source: "message", text: "" });
    expect(speechController.getSnapshot().error).toContain("没有可朗读");

    vi.stubGlobal("SpeechSynthesisUtterance", undefined);
    const missingConstructor = speechController.speak({ messageId: "message-5", source: "message", text: "hello" });
    expect(missingConstructor).toBe(false);
    expect(speechController.getSnapshot().error).toContain("不支持本地朗读");
  });
});
