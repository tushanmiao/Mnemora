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
  | "install";

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
  { command: "install", trigger: "/install", title: "安装资源包", description: "安装插件、技能或宠物：/install plugin 天气；不写名称则打开本地文件选择器", kind: "local", argumentHint: "<plugin|skill|pet> [名称]" },
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

/** 可安装的资源包类型。与后端 RemotePackageKind 的取值保持一致。 */
export type InstallKind = "plugin" | "skill" | "pet";

export const INSTALL_KINDS: readonly InstallKind[] = ["plugin", "skill", "pet"];

const INSTALL_KIND_LABEL: Record<InstallKind, string> = {
  plugin: "插件",
  skill: "技能",
  pet: "宠物",
};

export function installKindLabel(kind: InstallKind) {
  return INSTALL_KIND_LABEL[kind];
}

export type InstallCommandTarget =
  /** 类型缺失或拼错：回一条用法说明，不猜测用户想装什么。 */
  | { kind: null; reason: "missing" | "unknown"; token: string }
  /** 显式要求本地文件：local / dir。 */
  | { kind: InstallKind; source: "local"; mode: "zip" | "directory" }
  /**
   * 其余一律走远端。query 可能是名称，也可能是一句功能描述——
   * 两者都直接交给搜索，不在这里区分。
   */
  | { kind: InstallKind; source: "github"; query: string };

/**
 * 解析 `/install <类型> [描述或名称]`。
 *
 * 类型必填。缺失或无法识别时返回 kind: null，由调用方回一条用法说明——
 * 不猜类型：装错类型意味着往系统里塞进了一个你没打算装的东西。
 *
 *   /install                      → missing
 *   /install foo                  → unknown
 *   /install plugin               → 只表达意图，打开对话框等你输入
 *   /install plugin 天气           → 按描述搜索
 *   /install plugin 查天气并出摘要   → 同上，一整句描述也可以
 *   /install plugin owner/repo    → 已知仓库，直接取回
 *   /install plugin local         → 本地 ZIP 选择器
 *   /install plugin dir           → 本地目录选择器
 *
 * 描述和名称不做区分：GitHub 搜索本身就是全文匹配，把「查天气并出摘要」
 * 丢进去和把「weather」丢进去走的是同一条路，只是命中质量不同。
 * 硬要在客户端猜「这是名称还是描述」只会猜错。
 */
export function parseInstallTarget(argumentsValue: string): InstallCommandTarget {
  const trimmed = argumentsValue.trim();
  if (!trimmed) return { kind: null, reason: "missing", token: "" };

  const firstSpace = trimmed.search(/\s/);
  const kindToken = (firstSpace < 0 ? trimmed : trimmed.slice(0, firstSpace))
    .toLocaleLowerCase("en-US");
  const rest = firstSpace < 0 ? "" : trimmed.slice(firstSpace).trim();

  const kind = INSTALL_KINDS.find((item) => item === kindToken);
  if (!kind) return { kind: null, reason: "unknown", token: kindToken };

  // 本地安装现在需要显式要求：默认路径是「说一句需求就自动找」。
  const restLower = rest.toLocaleLowerCase("en-US");
  if (restLower === "local" || restLower === "zip") {
    return { kind, source: "local", mode: "zip" };
  }
  if (restLower === "dir" || restLower === "directory") {
    return { kind, source: "local", mode: "directory" };
  }

  return { kind, source: "github", query: rest };
}

/** `/install` 缺类型或类型无效时回的用法说明。 */
export function buildInstallUsage(target: Extract<InstallCommandTarget, { kind: null }>) {
  const forms = INSTALL_KINDS
    .map((kind) => `/install ${kind}（${INSTALL_KIND_LABEL[kind]}）`)
    .join("、");
  const prefix = target.reason === "unknown"
    ? `无法识别的安装类型「${target.token}」。`
    : "请指定要安装的类型。";
  return `${prefix}可用形式：${forms}。后面可以直接说想要什么功能，例如 /install plugin 查天气并生成摘要；已知仓库可写 /install plugin owner/repo；想从本地文件安装则用 /install plugin local 或 /install plugin dir。`;
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
