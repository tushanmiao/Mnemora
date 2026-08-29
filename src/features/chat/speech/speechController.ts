import {
  detectSpeechLanguage,
  splitSpeechText,
} from "./speechText";

export type SpeechSource = "message" | "selection";
export type SpeechStatus = "idle" | "preparing" | "speaking" | "paused" | "error";

export type SpeechTarget = {
  messageId: string;
  source: SpeechSource;
  text: string;
};

export type SpeechSnapshot = {
  status: SpeechStatus;
  target: SpeechTarget | null;
  chunkIndex: number;
  chunkCount: number;
  error: string | null;
};

const INITIAL_SNAPSHOT: SpeechSnapshot = {
  status: "idle",
  target: null,
  chunkIndex: 0,
  chunkCount: 0,
  error: null,
};

class SpeechController {
  private snapshot: SpeechSnapshot = INITIAL_SNAPSHOT;
  private readonly listeners = new Set<() => void>();
  private chunks: string[] = [];
  private generation = 0;

  getSnapshot = () => this.snapshot;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  speak(target: SpeechTarget) {
    const text = target.text.trim();
    if (!text) {
      this.publish({
        ...INITIAL_SNAPSHOT,
        status: "error",
        target: { ...target, text },
        error: "这条内容没有可朗读的正文。",
      });
      return false;
    }
    const synthesis = this.getSynthesis();
    if (!synthesis) {
      this.publish({
        ...INITIAL_SNAPSHOT,
        status: "error",
        target: { ...target, text },
        error: "当前环境不支持本地朗读。",
      });
      return false;
    }
    if (typeof SpeechSynthesisUtterance !== "function") {
      this.publish({
        ...INITIAL_SNAPSHOT,
        status: "error",
        target: { ...target, text },
        error: "当前环境不支持本地朗读。",
      });
      return false;
    }

    this.cancelCurrent(false);
    this.chunks = splitSpeechText(text);
    if (this.chunks.length === 0) return false;
    const generation = ++this.generation;
    this.publish({
      status: "preparing",
      target: { ...target, text },
      chunkIndex: 0,
      chunkCount: this.chunks.length,
      error: null,
    });
    this.speakChunk(generation, 0);
    return true;
  }

  toggle(target: SpeechTarget) {
    if (this.snapshot.target?.messageId !== target.messageId
      || this.snapshot.target.source !== target.source
      || this.snapshot.target.text !== target.text) {
      return this.speak(target);
    }
    if (this.snapshot.status === "speaking") {
      this.pause();
      return true;
    }
    if (this.snapshot.status === "paused") {
      this.resume();
      return true;
    }
    return this.speak(target);
  }

  pause() {
    const synthesis = this.getSynthesis();
    if (!synthesis || this.snapshot.status !== "speaking") return;
    try {
      synthesis.pause();
    } catch {
      this.publish({ ...this.snapshot, status: "error", error: "朗读失败，请重试。" });
      return;
    }
    this.publish({ ...this.snapshot, status: "paused" });
  }

  resume() {
    const synthesis = this.getSynthesis();
    if (!synthesis || this.snapshot.status !== "paused") return;
    try {
      synthesis.resume();
    } catch {
      this.publish({ ...this.snapshot, status: "error", error: "朗读失败，请重试。" });
      return;
    }
    this.publish({ ...this.snapshot, status: "speaking" });
  }

  stop() {
    this.cancelCurrent(true);
  }

  private speakChunk(generation: number, index: number) {
    if (generation !== this.generation || this.snapshot.status === "idle") return;
    const synthesis = this.getSynthesis();
    const chunk = this.chunks[index];
    if (!synthesis || !chunk) {
      this.finish(generation);
      return;
    }

    const Utterance = typeof SpeechSynthesisUtterance === "function"
      ? SpeechSynthesisUtterance
      : null;
    if (!Utterance) {
      this.publish({
        ...this.snapshot,
        status: "error",
        error: "当前环境不支持本地朗读。",
      });
      return;
    }

    let utterance: SpeechSynthesisUtterance;
    try {
      utterance = new Utterance(chunk);
    } catch {
      this.publish({
        ...this.snapshot,
        status: "error",
        error: "当前环境不支持本地朗读。",
      });
      return;
    }
    utterance.lang = detectSpeechLanguage(chunk);
    utterance.rate = 1;
    utterance.onstart = () => {
      if (generation !== this.generation) return;
      this.publish({ ...this.snapshot, status: "speaking", chunkIndex: index });
    };
    utterance.onend = () => {
      if (generation !== this.generation) return;
      if (index + 1 >= this.chunks.length) {
        this.finish(generation);
      } else {
        this.speakChunk(generation, index + 1);
      }
    };
    utterance.onerror = (event) => {
      if (generation !== this.generation) return;
      // cancel() may emit "canceled" or "interrupted" in WebView2; those are
      // intentional lifecycle events and must not surface as user errors.
      if (event.error === "canceled" || event.error === "interrupted") return;
      this.publish({ ...this.snapshot, status: "error", error: "朗读失败，请重试。" });
    };
    try {
      synthesis.speak(utterance);
    } catch {
      if (generation !== this.generation) return;
      this.publish({ ...this.snapshot, status: "error", error: "朗读失败，请重试。" });
    }
  }

  private finish(generation: number) {
    if (generation !== this.generation) return;
    this.publish({ ...INITIAL_SNAPSHOT });
  }

  private cancelCurrent(reset: boolean) {
    this.generation += 1;
    try {
      this.getSynthesis()?.cancel();
    } catch {
      // Some WebView speech implementations throw while already idle. The
      // controller still needs to clear its own state in that case.
    }
    this.chunks = [];
    if (reset) this.publish({ ...INITIAL_SNAPSHOT });
  }

  private getSynthesis(): SpeechSynthesis | null {
    return typeof window !== "undefined" && "speechSynthesis" in window
      ? window.speechSynthesis
      : null;
  }

  private publish(snapshot: SpeechSnapshot) {
    this.snapshot = snapshot;
    for (const listener of this.listeners) listener();
  }
}

export const speechController = new SpeechController();
