import type { useSkills } from "../../skills/hooks/useSkills";
import { SkillManager } from "../../skills/components/SkillPage";

type Props = {
  state: ReturnType<typeof useSkills>;
  onRemoteInstall: () => void;
};

export function SkillSettingsPanel({ state, onRemoteInstall }: Props) {
  return <SkillManager state={state} onRemoteInstall={onRemoteInstall} />;
}
