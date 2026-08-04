import { describe, expect, it } from "vitest";
import { INITIAL_DEEP_NOTE_STATE, reduceDeepNotePipeline } from "./pipelineState";

describe("reduceDeepNotePipeline", () => {
  it("covers the successful phase chain", () => {
    let state = reduceDeepNotePipeline(INITIAL_DEEP_NOTE_STATE, { type: "start" });
    state = reduceDeepNotePipeline(state, { type: "analyze" });
    state = reduceDeepNotePipeline(state, { type: "outlineReady", outline: { title: "T", summary: "", weakPoints: [], sections: [{ id: "s", heading: "H", kind: "concept", brief: "B", needsSupplement: false, sourceMessageIds: [] }] } });
    state = reduceDeepNotePipeline(state, { type: "draft", total: 1 });
    state = reduceDeepNotePipeline(state, { type: "sectionCompleted", current: 1 });
    state = reduceDeepNotePipeline(state, { type: "assemble" });
    state = reduceDeepNotePipeline(state, { type: "persist" });
    state = reduceDeepNotePipeline(state, { type: "complete" });
    expect(state.phase).toBe("done");
  });

  it("allows cancellation from an active phase", () => {
    const state = reduceDeepNotePipeline(
      reduceDeepNotePipeline(INITIAL_DEEP_NOTE_STATE, { type: "start" }),
      { type: "cancel" },
    );
    expect(state.phase).toBe("cancelled");
  });
});
