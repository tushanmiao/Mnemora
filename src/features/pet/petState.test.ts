import { describe, expect, it } from "vitest";
import { nextPetStateExpiry, projectPetState } from "./petState";

describe("projectPetState", () => {
  it("prioritizes a deep-note confirmation over chat state", () => {
    const result = projectPetState(
      { status: "streaming", updatedAt: 1 } as never,
      null,
      { phase: "awaitingOutline", updatedAt: 2 } as never,
      3,
    );
    expect(result.state).toBe("waiting");
    expect(result.label).toContain("确认");
  });

  it("maps a running chat tool without exposing its arguments", () => {
    const result = projectPetState({
      status: "streaming",
      updatedAt: 10,
      toolTraces: [{ status: "running" }],
    } as never, null, null, 11);
    expect(result.state).toBe("tooling");
    expect(result.detail).not.toContain("argument");
  });

  it("schedules only the remaining recent-success expiry", () => {
    const message = { status: "completed", updatedAt: 1_000 } as never;
    expect(nextPetStateExpiry(message, null, null, 4_000)).toBe(5_000);
    expect(nextPetStateExpiry(message, null, null, 9_001)).toBeNull();
    expect(nextPetStateExpiry({ status: "streaming", updatedAt: 1_000 } as never, null, null, 4_000)).toBeNull();
  });
});
