import type { ActivatedSkillSnapshot } from "../../../types/chat";
import type { SkillActivationSelection, SkillSummary } from "../../../types/skill";
import { RESERVED_SLASH_TRIGGERS } from "../commands/slashCommands";

const MAX_ACTIVE_SKILLS = 12;

/**
 * Slash Trigger 是保留的显式覆盖入口；普通消息不再携带人工选择，交给模型按目录隐式命中。
 */
export function resolveSkillActivation(
  content: string,
  skills: readonly SkillSummary[],
): SkillActivationSelection {
  const firstWord = content.trimStart().split(/\s+/, 1)[0]?.toLocaleLowerCase("en-US") ?? "";
  const slashMatches = firstWord.startsWith("/") && !RESERVED_SLASH_TRIGGERS.has(firstWord)
    ? skills.filter((skill) => skill.enabled && skill.triggers.some((trigger) => trigger.toLocaleLowerCase("en-US") === firstWord))
    : [];
  const slashSkill = slashMatches.length === 1 ? slashMatches[0] : undefined;
  return {
    skillIds: slashSkill ? [slashSkill.id] : [],
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
  if (!selection.slashSkillId || !selection.skillIds.includes(selection.slashSkillId)) return [];
  const enabled = new Map(skills.filter((skill) => skill.enabled).map((skill) => [skill.id, skill]));
  const skill = enabled.get(selection.slashSkillId);
  if (!skill) return [];
  return [{
    id: skill.id,
    name: skill.name,
    version: skill.version,
    contentHash: skill.contentHash,
    activation: "slash",
  }];
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
