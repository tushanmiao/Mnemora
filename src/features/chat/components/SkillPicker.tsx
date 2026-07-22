import { useEffect, useRef, useState } from "react";
import { Check, Sparkles, X } from "lucide-react";
import type { SkillSummary } from "../../../types/skill";

const MAX_SELECTED_SKILLS = 3;

type Props = {
  skills: SkillSummary[];
  selectedSkillIds: string[];
  disabled?: boolean;
  onChange: (skillIds: string[]) => void;
};

export function SkillPicker({ skills, selectedSkillIds, disabled = false, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const enabledSkills = skills.filter((skill) => skill.enabled);
  const availableIds = new Set(enabledSkills.map((skill) => skill.id));
  const availableSelectedIds = selectedSkillIds.filter((id) => availableIds.has(id));
  const selectedCount = availableSelectedIds.length;

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const toggle = (skillId: string) => {
    if (availableSelectedIds.includes(skillId)) {
      onChange(availableSelectedIds.filter((id) => id !== skillId));
      return;
    }
    if (availableSelectedIds.length >= MAX_SELECTED_SKILLS) return;
    onChange([...availableSelectedIds, skillId]);
  };

  return (
    <div className="skill-picker" ref={rootRef}>
      <button
        className={`icon-button${selectedCount > 0 ? " skill-picker-active" : ""}`}
        type="button"
        title="选择技能"
        aria-label="选择技能"
        aria-expanded={open}
        disabled={disabled || enabledSkills.length === 0}
        onClick={() => setOpen((value) => !value)}
      >
        <Sparkles size={18} />
      </button>
      {open ? (
        <div className="skill-picker-menu" role="menu">
          <header><strong>本轮技能</strong><span>最多 {MAX_SELECTED_SKILLS} 个</span></header>
          {enabledSkills.map((skill) => {
            const checked = availableSelectedIds.includes(skill.id);
            const blocked = !checked && availableSelectedIds.length >= MAX_SELECTED_SKILLS;
            return (
              <button type="button" role="menuitemcheckbox" aria-checked={checked} disabled={blocked} key={skill.id} onClick={() => toggle(skill.id)}>
                <span className="skill-picker-check">{checked ? <Check size={13} /> : null}</span>
                <span><strong>{skill.name}</strong><small>{skill.description}</small></span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

export function ActiveSkillTags({ skills, selectedSkillIds, disabled = false, onChange }: Props) {
  const selected = selectedSkillIds
    .map((id) => skills.find((skill) => skill.enabled && skill.id === id))
    .filter((skill): skill is SkillSummary => Boolean(skill));
  if (selected.length === 0) return null;
  return (
    <div className="composer-skill-tags" aria-label="当前对话技能">
      {selected.map((skill) => (
        <span key={skill.id} title={skill.description}>
          <Sparkles size={12} />{skill.name}
          <button
            type="button"
            title={`移除 ${skill.name}`}
            aria-label={`移除技能 ${skill.name}`}
            disabled={disabled}
            onClick={() => onChange(selectedSkillIds.filter((id) => id !== skill.id))}
          >
            <X size={12} />
          </button>
        </span>
      ))}
    </div>
  );
}
