import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  SkillDetail,
  SkillImportKind,
  SkillImportResult,
  SkillListResult,
  SkillSummary,
} from "../../../types/skill";

export function listSkills(): Promise<SkillListResult> {
  if (!isTauri()) return Promise.resolve({ skills: [], warnings: [] });
  return invoke<SkillListResult>("skills_list");
}

export function getSkillDetail(skillId: string): Promise<SkillDetail> {
  if (!isTauri()) return Promise.reject(new Error("技能详情需要在 Tauri 应用中读取。"));
  return invoke<SkillDetail>("skills_get_detail", { skillId });
}

export function importSkill(
  path: string,
  kind: SkillImportKind,
  replaceExisting = false,
): Promise<SkillImportResult> {
  if (!isTauri()) return Promise.reject(new Error("技能安装需要在 Tauri 应用中执行。"));
  return invoke<SkillImportResult>("skills_import", {
    request: { path, kind, replaceExisting },
  });
}

export function setSkillEnabled(skillId: string, enabled: boolean): Promise<SkillSummary> {
  if (!isTauri()) return Promise.reject(new Error("技能状态需要在 Tauri 应用中保存。"));
  return invoke<SkillSummary>("skills_set_enabled", { skillId, enabled });
}

export function uninstallSkill(skillId: string): Promise<void> {
  if (!isTauri()) return Promise.reject(new Error("技能删除需要在 Tauri 应用中执行。"));
  return invoke<void>("skills_uninstall", { skillId });
}

export function restoreBuiltinSkill(skillId: string): Promise<SkillSummary> {
  if (!isTauri()) return Promise.reject(new Error("技能恢复需要在 Tauri 应用中执行。"));
  return invoke<SkillSummary>("skills_restore_builtin", { skillId });
}

