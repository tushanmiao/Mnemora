import { BriefcaseBusiness, MessageCircle } from "lucide-react";
import type { WorkspaceMode } from "../types";
import { useI18n } from "../../../i18n/I18nProvider";
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
  const { t } = useI18n();
  return (
    <nav
      className={`workspace-mode-switch${collapsed ? " workspace-mode-switch-collapsed" : ""}`}
      aria-label={t("mode.label")}
    >
      {modes.map(({ id, label, icon: Icon }) => (
        <button
          className={`workspace-mode-option${mode === id || (mode === "notes" && id === "chat") ? " workspace-mode-option-active" : ""}`}
          type="button"
          key={id}
          title={collapsed ? label : undefined}
          aria-current={mode === id || (mode === "notes" && id === "chat") ? "page" : undefined}
          onClick={() => onChange(id)}
        >
          <Icon size={16} />
          <span>{label}</span>
        </button>
      ))}
    </nav>
  );
}
