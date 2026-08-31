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
  it("keeps a slash-triggered skill as the explicit override", () => {
    const skills = [skill("paper", ["/paper"]), skill("writing", ["/write"])];
    expect(resolveSkillActivation(" /paper 分析方法", skills)).toEqual({
      skillIds: ["paper"],
      slashSkillId: "paper",
    });
  });

  it("does not preselect skills for an ordinary message", () => {
    expect(resolveSkillActivation("hello", [skill("paper", ["/paper"]), skill("off", ["/off"], false)])).toEqual({
      skillIds: [],
      slashSkillId: undefined,
    });
  });

  it("creates only the explicit slash snapshot without copying the skill body", () => {
    const skills = [skill("paper", ["/paper"]), skill("writing", ["/write"])];
    expect(createActivatedSkillSnapshots({
      skillIds: ["paper", "writing"],
      slashSkillId: "paper",
    }, skills)).toEqual([
      expect.objectContaining({ id: "paper", activation: "slash", contentHash: "sha256:test" }),
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
