import type { useSkills } from "../../skills/hooks/useSkills";
import { SkillManager } from "../../skills/components/SkillPage";

type Props = {
  state: ReturnType<typeof useSkills>;
};

export function SkillSettingsPanel({ state }: Props) {
  return <SkillManager state={state} />;
}
