import { describe, expect, it } from "vitest";
import type { SkillSummary } from "../../../types/skill";
import {
  RESERVED_SLASH_TRIGGERS,
  buildLocalCommandHelp,
  buildSlashSuggestions,
  parseInstallMode,
  parseInstallTarget,
  parseSlashInput,
} from "./slashCommands";

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

  it("parses the three install commands", () => {
    expect(parseSlashInput("/install-skill", [])).toMatchObject({ kind: "local", command: "installSkill" });
    expect(parseSlashInput("/install-plugin", [])).toMatchObject({ kind: "local", command: "installPlugin" });
    expect(parseSlashInput("/install-pet", [])).toMatchObject({ kind: "local", command: "installPet" });
  });

  it("passes the install mode through as an argument", () => {
    expect(parseSlashInput("/install-skill dir", [])).toMatchObject({ arguments: "dir" });
    expect(parseInstallMode("")).toBe("zip");
    expect(parseInstallMode("dir")).toBe("directory");
    expect(parseInstallMode("directory")).toBe("directory");
    expect(parseInstallMode("  DIR  ")).toBe("directory");
    // 无法识别的参数退回默认值，而不是报错——安装本身还会弹文件选择器。
    expect(parseInstallMode("zip")).toBe("zip");
    expect(parseInstallMode("nonsense")).toBe("zip");
  });

  it("routes install arguments to local or github", () => {
    expect(parseInstallTarget("")).toEqual({ source: "local", mode: "zip" });
    expect(parseInstallTarget("dir")).toEqual({ source: "local", mode: "directory" });
    expect(parseInstallTarget("directory")).toEqual({ source: "local", mode: "directory" });

    expect(parseInstallTarget("github")).toEqual({ source: "github", query: "" });
    expect(parseInstallTarget("gh")).toEqual({ source: "github", query: "" });
    expect(parseInstallTarget("github weather")).toEqual({ source: "github", query: "weather" });
    expect(parseInstallTarget("GitHub  多个 关键词")).toEqual({ source: "github", query: "多个 关键词" });

    // 裸 owner/repo 是「我已经知道装哪个」，直接带进对话框
    expect(parseInstallTarget("someone/weather-skill")).toEqual({
      source: "github",
      query: "someone/weather-skill",
    });

    // 其余关键词也走远端搜索，比静默弹本地选择器更符合预期
    expect(parseInstallTarget("天气")).toEqual({ source: "github", query: "天气" });
  });

  /**
   * 这条守的是「/help 与命令表同步」这个约束本身：
   * 只要有人加了命令却没让它进 /help，这里就会失败。
   */
  it("derives the help text from every reserved trigger", () => {
    const help = buildLocalCommandHelp();
    for (const trigger of RESERVED_SLASH_TRIGGERS) {
      expect(help).toContain(trigger);
    }
    expect(help).toContain("/compact [重点]");
    expect(help).toContain("/install-plugin [dir|github 关键词]");
  });

  it("keeps install triggers reserved so a skill cannot shadow them", () => {
    const hijacker = [skill("evil", "/install-plugin")];
    expect(parseSlashInput("/install-plugin", hijacker)).toMatchObject({
      kind: "local",
      command: "installPlugin",
    });
    expect(buildSlashSuggestions("/install", hijacker).filter((item) => item.kind === "skill")).toHaveLength(0);
  });
});
