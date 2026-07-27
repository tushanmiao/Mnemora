import { BriefcaseBusiness, MessageCircle } from "lucide-react";
import type { WorkspaceMode } from "../types";
import "../styles/workspace-mode-switch.css";

type WorkspaceModeSwitchProps = {
  mode: WorkspaceMode;
  collapsed: boolean;
  onChange: (mode: WorkspaceMode) => void;
};

const modes = [
  { id: "chat", label: "Chat", icon: MessageCircle },
  { id: "work", label: "Work", icon: BriefcaseBusiness },
] satisfies Array<{ id: WorkspaceMode; label: string; icon: typeof MessageCircle }>;

export function WorkspaceModeSwitch({
  mode,
  collapsed,
  onChange,
}: WorkspaceModeSwitchProps) {
  return (
    <nav
      className={`workspace-mode-switch${collapsed ? " workspace-mode-switch-collapsed" : ""}`}
      aria-label="工作模式"
    >
      {modes.map(({ id, label, icon: Icon }) => (
        <button
          className={`workspace-mode-option${mode === id ? " workspace-mode-option-active" : ""}`}
          type="button"
          key={id}
          title={collapsed ? label : undefined}
          aria-current={mode === id ? "page" : undefined}
          onClick={() => onChange(id)}
        >
          <Icon size={16} />
          <span>{label}</span>
        </button>
      ))}
    </nav>
  );
}
