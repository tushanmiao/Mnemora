export type SkillSource = "builtin" | "user";
export type SkillFileKind = "skillMd" | "reference" | "script" | "asset" | "other";

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
