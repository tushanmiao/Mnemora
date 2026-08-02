export type SkillSource = "builtin" | "user";
export type SkillFileKind = "skillMd" | "reference" | "script" | "asset" | "other";
export type SkillMode = "chat" | "work" | "notes";
export type SkillRisk = "low" | "medium" | "high";
export type SkillResourceCost = "low" | "medium" | "high";

/** 上游来源只用于审计和界面展示，不会进入模型提示词。 */
export interface SkillProvenance {
  repository?: string;
  path?: string;
  revision?: string;
  attribution?: string;
  adapted: boolean;
  adaptationNotes?: string;
}

/** 技能列表和对话选择器共用的轻量元数据，不包含 SKILL.md 正文。 */
export interface SkillSummary {
  id: string;
  name: string;
  description: string;
  version: string;
  source: SkillSource;
  enabled: boolean;
  /** 内置技能首次安装时的默认状态；用户可以覆盖。 */
  defaultEnabled: boolean;
  /** 只在对应工作模式中把轻量技能目录暴露给模型。 */
  supportedModes?: SkillMode[];
  /** 技能本身的风险提示；真正的权限仍由 Tool 层控制。 */
  risk?: SkillRisk;
  /** 加载正文和附带资源前的预估成本。 */
  resourceCost?: SkillResourceCost;
  triggers: string[];
  argumentHint?: string;
  recommendedTools: string[];
  requiredTools: string[];
  disableModelInvocation: boolean;
  license?: string;
  compatibility?: string;
  provenance: SkillProvenance;
  contentHash: string;
}

export interface SkillFileEntry {
  path: string;
  kind: SkillFileKind;
  sizeBytes: number;
}

export interface SkillDetail extends SkillSummary {
  markdown: string;
  files: SkillFileEntry[];
}

export interface SkillListResult {
  skills: SkillSummary[];
  warnings: string[];
}

export type SkillImportKind = "directory" | "zip";
export type SkillImportStatus = "installed" | "alreadyExists";

export interface SkillImportResult {
  status: SkillImportStatus;
  skill: SkillSummary;
}

/** 输入区解析后的本轮技能选择；Slash 技能会在消息快照中单独标记。 */
export interface SkillActivationSelection {
  skillIds: string[];
  slashSkillId?: string;
}
