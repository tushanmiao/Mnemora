import { describe, expect, it } from "vitest";
import type { DeepNoteDagNode } from "../../chat/api/notePipeline";
import { layoutDeepNoteDag } from "./DeepNoteDagGraph";

function node(nodeId: string, dependsOn: string[] = []): DeepNoteDagNode {
  return {
    nodeId,
    nodeType: "draftSection",
    sectionId: nodeId,
    dependsOn,
    status: dependsOn.length > 0 ? "pending" : "ready",
    attemptCount: 0,
    evidenceIds: [],
    inputHash: `hash-${nodeId}`,
    outputRef: null,
    validationJson: "",
    errorMessage: null,
  };
}

describe("DeepNoteDagGraph layout", () => {
  it("places every dependency in an earlier layer", () => {
    const layout = layoutDeepNoteDag([
      node("input"),
      node("evidence-a", ["input"]),
      node("evidence-b", ["input"]),
      node("ledger", ["evidence-a", "evidence-b"]),
      node("persist", ["ledger"]),
    ]);
    const layers = new Map(layout.nodes.map((item) => [item.node.nodeId, item.layer]));
    expect(layers.get("input")).toBe(0);
    expect(layers.get("evidence-a")).toBe(1);
    expect(layers.get("evidence-b")).toBe(1);
    expect(layers.get("ledger")).toBe(2);
    expect(layers.get("persist")).toBe(3);
    expect(layout.edges).toHaveLength(5);
  });

  it("keeps missing dependencies and cycles visible in a fallback layer", () => {
    const layout = layoutDeepNoteDag([
      node("root", ["missing"]),
      node("cycle-a", ["cycle-b"]),
      node("cycle-b", ["cycle-a"]),
    ]);
    expect(layout.nodes).toHaveLength(3);
    expect(layout.width).toBeGreaterThan(0);
    expect(layout.height).toBeGreaterThan(0);
    expect(layout.nodes.every((item) => Number.isFinite(item.x) && Number.isFinite(item.y))).toBe(true);
  });
});
