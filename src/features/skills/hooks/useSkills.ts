import { useCallback, useEffect, useState } from "react";
import type { SkillImportKind, SkillImportResult, SkillSummary } from "../../../types/skill";
import {
  importSkill,
  listSkills,
  restoreBuiltinSkill,
  setSkillEnabled,
  uninstallSkill,
} from "../api/skills";
import { slashCommandConflicts } from "../../chat/commands/slashCommands";

export function useSkills() {
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [busySkillId, setBusySkillId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const result = await listSkills();
      setSkills(result.skills);
      setWarnings([...result.warnings, ...slashCommandConflicts(result.skills)]);
    } catch (reason) {
      setError(errorMessage(reason, "读取技能列表失败。"));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const toggle = useCallback(async (skillId: string, enabled: boolean) => {
    setBusySkillId(skillId);
    setError("");
    try {
      const updated = await setSkillEnabled(skillId, enabled);
      setSkills((current) => current.map((skill) => skill.id === skillId ? updated : skill));
    } catch (reason) {
      setError(errorMessage(reason, "保存技能状态失败。"));
    } finally {
      setBusySkillId(null);
    }
  }, []);

  const install = useCallback(async (
    path: string,
    kind: SkillImportKind,
    replaceExisting = false,
  ): Promise<SkillImportResult | null> => {
    setBusySkillId("__install__");
    setError("");
    try {
      const result = await importSkill(path, kind, replaceExisting);
      if (result.status === "installed") await refresh();
      return result;
    } catch (reason) {
      setError(errorMessage(reason, "安装技能失败。"));
      return null;
    } finally {
      setBusySkillId(null);
    }
  }, [refresh]);

  const uninstall = useCallback(async (skillId: string) => {
    setBusySkillId(skillId);
    setError("");
    try {
      await uninstallSkill(skillId);
      setSkills((current) => current.filter((skill) => skill.id !== skillId));
    } catch (reason) {
      setError(errorMessage(reason, "删除技能失败。"));
    } finally {
      setBusySkillId(null);
    }
  }, []);

  const restore = useCallback(async (skillId: string) => {
    setBusySkillId(skillId);
    setError("");
    try {
      const updated = await restoreBuiltinSkill(skillId);
      setSkills((current) => current.map((skill) => skill.id === skillId ? updated : skill));
    } catch (reason) {
      setError(errorMessage(reason, "恢复内置技能失败。"));
    } finally {
      setBusySkillId(null);
    }
  }, []);

  return {
    skills,
    warnings,
    loading,
    error,
    busySkillId,
    refresh,
    toggle,
    install,
    uninstall,
    restore,
  };
}

function errorMessage(reason: unknown, fallback: string) {
  if (reason instanceof Error) return reason.message;
  return typeof reason === "string" ? reason : fallback;
}
