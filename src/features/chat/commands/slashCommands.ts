import type { SkillSummary } from "../../../types/skill";

export type LocalSlashCommand =
  | "help"
  | "new"
  | "clear"
  | "compact"
  | "model"
  | "settings"
  | "skills"
  | "memory"
  | "attach"
  | "installSkill"
  | "installPlugin"
  | "installPet";

export type SlashCommandExecutionResult = {
  executed: boolean;
  message?: string;
};

export type SlashSuggestion = {
  trigger: string;
  title: string;
  description: string;
  kind: "local" | "skill";
  skillId?: string;
  argumentHint?: string;
};

export type ParsedSlashInput =
  | { kind: "local"; command: LocalSlashCommand; arguments: string }
  | { kind: "skill"; skillId: string; arguments: string }
  | { kind: "unknown"; trigger: string }
  | { kind: "conflict"; trigger: string; skillIds: string[] };

const LOCAL_COMMANDS: Array<SlashSuggestion & { command: LocalSlashCommand }> = [
  { command: "help", trigger: "/help", title: "命令帮助", description: "查看可用的本地命令", kind: "local" },
  { command: "new", trigger: "/new", title: "新建对话", description: "保留当前对话并创建新对话", kind: "local" },
  { command: "clear", trigger: "/clear", title: "清除当前对话", description: "确认后永久删除当前对话及附件", kind: "local" },
  { command: "compact", trigger: "/compact", title: "压缩上下文", description: "可在后面填写需要保留的重点", kind: "local", argumentHint: "[重点]" },
  { command: "model", trigger: "/model", title: "选择模型", description: "打开当前对话的模型选择菜单", kind: "local" },
  { command: "settings", trigger: "/settings", title: "设置", description: "打开基础设置", kind: "local" },
  { command: "skills", trigger: "/skills", title: "技能", description: "打开技能设置", kind: "local" },
  { command: "memory", trigger: "/memory", title: "记忆", description: "打开记忆设置", kind: "local" },
  { command: "attach", trigger: "/attach", title: "添加附件", description: "打开本地附件选择器", kind: "local" },
  { command: "installSkill", trigger: "/install-skill", title: "安装技能", description: "本地 ZIP / dir 目录，或 github 关键词从 GitHub 搜索安装", kind: "local", argumentHint: "[dir|github 关键词]" },
  { command: "installPlugin", trigger: "/install-plugin", title: "安装插件", description: "本地或 GitHub 安装插件；装完保持停用，需手动启用", kind: "local", argumentHint: "[dir|github 关键词]" },
  { command: "installPet", trigger: "/install-pet", title: "安装宠物", description: "本地或 GitHub 安装桌面宠物资源包", kind: "local", argumentHint: "[dir|github 关键词]" },
];

export const RESERVED_SLASH_TRIGGERS = new Set(LOCAL_COMMANDS.map((item) => item.trigger));

/**
 * /help 的正文由命令表推导，而不是另写一份清单。
 * 手写清单每加一个命令就要记得同步，漏一次就长期错下去——
 * 这里让它结构上无法漂移。
 */
export function buildLocalCommandHelp() {
  const entries = LOCAL_COMMANDS.map((item) => (
    item.argumentHint ? `${item.trigger} ${item.argumentHint}` : item.trigger
  ));
  return `可用命令：${entries.join("、")}。`;
}

/** 安装类命令的可选参数：默认 zip，显式 dir/directory 才选目录。 */
export function parseInstallMode(argumentsValue: string): "zip" | "directory" {
  const token = argumentsValue.trim().toLocaleLowerCase("en-US");
  return token === "dir" || token === "directory" ? "directory" : "zip";
}

export type InstallCommandTarget =
  | { source: "local"; mode: "zip" | "directory" }
  | { source: "github"; query: string };

/**
 * 解析安装命令的参数，区分本地与 GitHub 两条路径。
 *
 *   （空）/ dir / directory  → 本地文件选择器
 *   github [关键词]          → 打开 GitHub 搜索安装对话框
 *   owner/repo               → 同上，但把仓库名直接带进去
 *
 * 注意「owner/repo 直接带入」并不等于跳过确认：对话框仍会先下载、
 * 解析清单、展示权限，等用户勾选确认才安装。
 */
export function parseInstallTarget(argumentsValue: string): InstallCommandTarget {
  const trimmed = argumentsValue.trim();
  if (!trimmed) return { source: "local", mode: "zip" };

  const lower = trimmed.toLocaleLowerCase("en-US");
  if (lower === "dir" || lower === "directory") {
    return { source: "local", mode: "directory" };
  }
  if (lower === "github" || lower === "gh") {
    return { source: "github", query: "" };
  }
  const remote = /^(?:github|gh)\s+(.+)$/i.exec(trimmed);
  if (remote) return { source: "github", query: remote[1].trim() };

  // 裸 owner/repo 也走远端：这是「我已经知道要装哪个」的常见输入。
  if (/^[\w.-]+\/[\w.-]+$/.test(trimmed)) {
    return { source: "github", query: trimmed };
  }
  // 其余当作 GitHub 搜索关键词，比静默走本地选择器更符合预期。
  return { source: "github", query: trimmed };
}

function firstToken(value: string) {
  const end = value.search(/\s/);
  return (end < 0 ? value : value.slice(0, end)).toLocaleLowerCase("en-US");
}

function normalizedInput(value: string) {
  return value.trimStart();
}

function slashInput(value: string) {
  const normalized = normalizedInput(value);
  return normalized.startsWith("/") && !normalized.includes("\n");
}

export function parseSlashInput(value: string, skills: readonly SkillSummary[]): ParsedSlashInput | null {
  if (!slashInput(value)) return null;
  const normalized = normalizedInput(value);
  const trigger = firstToken(normalized);
  const argumentsValue = normalized.slice(trigger.length).trimStart();
  const local = LOCAL_COMMANDS.find((item) => item.trigger === trigger);
  if (local) return { kind: "local", command: local.command, arguments: argumentsValue };
  const matches = skills.filter((skill) => (
    skill.enabled
    && skill.triggers.some((item) => item.toLocaleLowerCase("en-US") === trigger)
  ));
  if (matches.length === 1) return { kind: "skill", skillId: matches[0].id, arguments: argumentsValue };
  if (matches.length > 1) return { kind: "conflict", trigger, skillIds: matches.map((skill) => skill.id) };
  return { kind: "unknown", trigger };
}

export function buildSlashSuggestions(value: string, skills: readonly SkillSummary[]) {
  if (!slashInput(value)) return [];
  const query = firstToken(normalizedInput(value));
  const result: SlashSuggestion[] = LOCAL_COMMANDS.filter((item) => item.trigger.startsWith(query));
  const triggerOwners = new Map<string, SkillSummary[]>();
  for (const skill of skills) {
    if (!skill.enabled) continue;
    for (const rawTrigger of skill.triggers) {
      const trigger = rawTrigger.toLocaleLowerCase("en-US");
      if (RESERVED_SLASH_TRIGGERS.has(trigger) || !trigger.startsWith(query)) continue;
      triggerOwners.set(trigger, [...(triggerOwners.get(trigger) ?? []), skill]);
    }
  }
  for (const [trigger, owners] of triggerOwners) {
    if (owners.length !== 1) continue;
    const skill = owners[0];
    result.push({
      trigger,
      title: skill.name,
      description: skill.description,
      kind: "skill",
      skillId: skill.id,
      argumentHint: skill.argumentHint,
    });
  }
  return result.slice(0, 12);
}

export function slashCommandConflicts(skills: readonly SkillSummary[]) {
  const owners = new Map<string, string[]>();
  for (const skill of skills) {
    for (const rawTrigger of skill.triggers) {
      const trigger = rawTrigger.toLocaleLowerCase("en-US");
      owners.set(trigger, [...(owners.get(trigger) ?? []), skill.id]);
    }
  }
  const warnings: string[] = [];
  for (const [trigger, skillIds] of owners) {
    if (RESERVED_SLASH_TRIGGERS.has(trigger)) {
      warnings.push(`技能 ${skillIds.join("、")} 的触发词 ${trigger} 与内置命令冲突，已禁用该 Slash 入口。`);
    } else if (skillIds.length > 1) {
      warnings.push(`触发词 ${trigger} 被技能 ${skillIds.join("、")} 重复声明，已禁用该 Slash 入口。`);
    }
  }
  return warnings;
}
