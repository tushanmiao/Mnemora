import type { ActivatedSkillSnapshot } from "../../../types/chat";
import type { SkillActivationSelection, SkillSummary } from "../../../types/skill";
import { RESERVED_SLASH_TRIGGERS } from "../commands/slashCommands";

const MAX_ACTIVE_SKILLS = 12;

/**
 * Slash Trigger 只负责把技能加入本轮选择；原始消息仍会持久化，Rust 在请求边界移除触发词。
 */
export function resolveSkillActivation(
  content: string,
  selectedSkillIds: readonly string[],
  skills: readonly SkillSummary[],
): SkillActivationSelection {
  const enabled = new Map(skills.filter((skill) => skill.enabled).map((skill) => [skill.id, skill]));
  const firstWord = content.trimStart().split(/\s+/, 1)[0]?.toLocaleLowerCase("en-US") ?? "";
  const slashMatches = firstWord.startsWith("/") && !RESERVED_SLASH_TRIGGERS.has(firstWord)
    ? skills.filter((skill) => skill.enabled && skill.triggers.some((trigger) => trigger.toLocaleLowerCase("en-US") === firstWord))
    : [];
  const slashSkill = slashMatches.length === 1 ? slashMatches[0] : undefined;
  const ordered = [slashSkill?.id, ...selectedSkillIds]
    .filter((id): id is string => Boolean(id) && enabled.has(id as string));
  return {
    skillIds: [...new Set(ordered)].slice(0, MAX_ACTIVE_SKILLS),
    slashSkillId: slashSkill?.id,
  };
}

/**
 * 将本轮技能 ID 转成轻量版本快照；完整正文始终由 Rust 按 ID 读取，不进入对话 JSON。
 */
export function createActivatedSkillSnapshots(
  selection: SkillActivationSelection,
  skills: readonly SkillSummary[],
): ActivatedSkillSnapshot[] {
  const enabled = new Map(skills.filter((skill) => skill.enabled).map((skill) => [skill.id, skill]));
  return [...new Set(selection.skillIds)]
    .slice(0, MAX_ACTIVE_SKILLS)
    .flatMap((skillId) => {
      const skill = enabled.get(skillId);
      if (!skill) return [];
      return [{
        id: skill.id,
        name: skill.name,
        version: skill.version,
        contentHash: skill.contentHash,
        activation: skill.id === selection.slashSkillId ? "slash" as const : "manual" as const,
      }];
    });
}

/** 重新生成时沿用原来的激活方式，但只保留当前仍安装且启用的技能。 */
export function refreshActivatedSkillSnapshots(
  snapshots: readonly ActivatedSkillSnapshot[],
  skills: readonly SkillSummary[],
): ActivatedSkillSnapshot[] {
  const enabled = new Map(skills.filter((skill) => skill.enabled).map((skill) => [skill.id, skill]));
  return [...new Map(snapshots.map((snapshot) => [snapshot.id, snapshot])).values()]
    .slice(0, MAX_ACTIVE_SKILLS)
    .flatMap((snapshot) => {
      const skill = enabled.get(snapshot.id);
      if (!skill) return [];
      return [{
        id: skill.id,
        name: skill.name,
        version: skill.version,
        contentHash: skill.contentHash,
        activation: snapshot.activation,
      }];
    });
}
