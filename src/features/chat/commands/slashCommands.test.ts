import { describe, expect, it } from "vitest";
import type { SkillSummary } from "../../../types/skill";
import {
  RESERVED_SLASH_TRIGGERS,
  buildInstallUsage,
  buildLocalCommandHelp,
  buildSlashSuggestions,
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

  it("exposes a single install command", () => {
    expect(parseSlashInput("/install", [])).toMatchObject({ kind: "local", command: "install" });
    expect(parseSlashInput("/install plugin 天气", [])).toMatchObject({
      kind: "local",
      command: "install",
      arguments: "plugin 天气",
    });
    // 旧的分开命令不再存在
    expect(parseSlashInput("/install-plugin", [])).toMatchObject({ kind: "unknown" });
  });

  it("requires an install kind and never guesses one", () => {
    expect(parseInstallTarget("")).toEqual({ kind: null, reason: "missing", token: "" });
    expect(parseInstallTarget("   ")).toEqual({ kind: null, reason: "missing", token: "" });
    expect(parseInstallTarget("天气")).toEqual({ kind: null, reason: "unknown", token: "天气" });
    expect(parseInstallTarget("plugins")).toEqual({ kind: null, reason: "unknown", token: "plugins" });
  });

  it("treats a bare kind as intent and everything after it as a query", () => {
    // 只表达意图：进远端流程但查询为空，对话框会停下来等用户描述需求。
    // 这里不能回落到本地文件选择器——用户说「我要装插件」时，
    // 更可能是还不知道装哪个，而不是手上已经有个 ZIP。
    expect(parseInstallTarget("plugin")).toEqual({ kind: "plugin", source: "github", query: "" });
    expect(parseInstallTarget("PLUGIN")).toEqual({ kind: "plugin", source: "github", query: "" });

    // 一个词、一整句描述、仓库名，都原样交给搜索——不在客户端猜是哪种
    expect(parseInstallTarget("plugin 天气")).toEqual({
      kind: "plugin", source: "github", query: "天气",
    });
    expect(parseInstallTarget("skill 查天气并生成结构化摘要")).toEqual({
      kind: "skill", source: "github", query: "查天气并生成结构化摘要",
    });
    expect(parseInstallTarget("pet  someone/cat-pet ")).toEqual({
      kind: "pet", source: "github", query: "someone/cat-pet",
    });
  });

  it("keeps an explicit escape hatch for local files", () => {
    expect(parseInstallTarget("plugin local")).toEqual({ kind: "plugin", source: "local", mode: "zip" });
    expect(parseInstallTarget("plugin zip")).toEqual({ kind: "plugin", source: "local", mode: "zip" });
    expect(parseInstallTarget("skill dir")).toEqual({ kind: "skill", source: "local", mode: "directory" });
    expect(parseInstallTarget("pet DIRECTORY")).toEqual({ kind: "pet", source: "local", mode: "directory" });
  });

  it("explains every valid form when the kind is missing or wrong", () => {
    const missing = buildInstallUsage({ kind: null, reason: "missing", token: "" });
    for (const kind of ["plugin", "skill", "pet"]) {
      expect(missing).toContain(`/install ${kind}`);
    }
    const unknown = buildInstallUsage({ kind: null, reason: "unknown", token: "plugins" });
    expect(unknown).toContain("plugins");
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
    expect(help).toContain("/install <plugin|skill|pet> [名称]");
  });

  it("keeps the install trigger reserved so a skill cannot shadow it", () => {
    const hijacker = [skill("evil", "/install")];
    expect(parseSlashInput("/install plugin 天气", hijacker)).toMatchObject({
      kind: "local",
      command: "install",
    });
    expect(buildSlashSuggestions("/install", hijacker).filter((item) => item.kind === "skill")).toHaveLength(0);
  });
});
