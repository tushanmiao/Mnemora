import { describe, expect, it } from "vitest";
import type { SkillSummary } from "../../../types/skill";
import {
  createActivatedSkillSnapshots,
  refreshActivatedSkillSnapshots,
  resolveSkillActivation,
} from "./skillActivation";

const skill = (id: string, triggers: string[], enabled = true): SkillSummary => ({
  id,
  name: id,
  description: id,
  version: "1.0.0",
  source: "builtin",
  enabled,
  defaultEnabled: enabled,
  triggers,
  recommendedTools: [],
  requiredTools: [],
  disableModelInvocation: false,
  provenance: { adapted: false },
  contentHash: "sha256:test",
});

describe("resolveSkillActivation", () => {
  it("prioritizes a slash-triggered skill and deduplicates manual selection", () => {
    const skills = [skill("paper", ["/paper"]), skill("writing", ["/write"])];
    expect(resolveSkillActivation(" /paper 分析方法", ["writing", "paper"], skills)).toEqual({
      skillIds: ["paper", "writing"],
      slashSkillId: "paper",
    });
  });

  it("drops disabled or missing manual skills", () => {
    expect(resolveSkillActivation("hello", ["off", "missing"], [skill("off", ["/off"], false)])).toEqual({
      skillIds: [],
      slashSkillId: undefined,
    });
  });

  it("creates message snapshots without copying the skill body", () => {
    const skills = [skill("paper", ["/paper"]), skill("writing", ["/write"])];
    expect(createActivatedSkillSnapshots({
      skillIds: ["paper", "writing"],
      slashSkillId: "paper",
    }, skills)).toEqual([
      expect.objectContaining({ id: "paper", activation: "slash", contentHash: "sha256:test" }),
      expect.objectContaining({ id: "writing", activation: "manual", contentHash: "sha256:test" }),
    ]);
  });

  it("refreshes regeneration snapshots and drops unavailable skills", () => {
    expect(refreshActivatedSkillSnapshots([
      {
        id: "paper",
        name: "old paper",
        version: "0.1.0",
        contentHash: "sha256:old",
        activation: "slash",
      },
      {
        id: "removed",
        name: "removed",
        version: "1.0.0",
        contentHash: "sha256:removed",
        activation: "manual",
      },
    ], [skill("paper", ["/paper"])] )).toEqual([
      expect.objectContaining({
        id: "paper",
        name: "paper",
        version: "1.0.0",
        contentHash: "sha256:test",
        activation: "slash",
      }),
    ]);
  });
});
