import { describe, expect, it } from "vitest";
import type { SkillSummary } from "../../../types/skill";
import { buildSlashSuggestions, parseSlashInput } from "./slashCommands";

const skill = (id: string, trigger: string): SkillSummary => ({
  id,
  name: id,
  description: `${id} description`,
  version: "1.0.0",
  source: "user",
  enabled: true,
  defaultEnabled: true,
  triggers: [trigger],
  recommendedTools: [],
  requiredTools: [],
  disableModelInvocation: false,
  provenance: { adapted: false },
  contentHash: "sha256:test",
});

describe("slash commands", () => {
  it("parses local commands only at the beginning of a single line", () => {
    expect(parseSlashInput("/new", [])).toMatchObject({ kind: "local", command: "new" });
    expect(parseSlashInput(" /new", [])).toMatchObject({ kind: "local", command: "new" });
    expect(parseSlashInput("text /new", [])).toBeNull();
  });

  it("does not expose reserved or conflicting skill triggers", () => {
    const skills = [skill("one", "/review"), skill("two", "/review"), skill("three", "/new")];
    expect(buildSlashSuggestions("/", skills).map((item) => item.trigger)).not.toContain("/review");
    expect(buildSlashSuggestions("/", skills).filter((item) => item.trigger === "/new")).toHaveLength(1);
    expect(parseSlashInput("/review paper", skills)?.kind).toBe("conflict");
  });
});
