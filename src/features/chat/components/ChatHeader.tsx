import { useEffect, useState, useRef } from "react";
import {
  Check,
  ChevronDown,
  Moon,
  MoreHorizontal,
  PanelRight,
  PanelRightClose,
  ListChecks,
  ShieldCheck,
  Sun,
} from "lucide-react";
import { AI_PERMISSION_LABELS, type AiPermissionMode } from "../../../types/chat";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/chat-header.css";

const permissionOptions = Object.keys(AI_PERMISSION_LABELS) as AiPermissionMode[];

type ChatHeaderProps = {
  title: string;
  permission: AiPermissionMode;
  permissionDisabled: boolean;
  theme: "light" | "dark";
  compact?: boolean;
  onPermissionChange: (permission: AiPermissionMode) => void;
  onToggleTheme: () => void;
  onClosePanel?: () => void;
  showTaskProgress?: boolean;
  onToggleTaskProgress?: (enabled: boolean) => void;
};

export function ChatHeader({
  title,
  permission,
  permissionDisabled,
  theme,
  compact = false,
  onPermissionChange,
  onToggleTheme,
  onClosePanel,
  showTaskProgress = true,
  onToggleTaskProgress,
}: ChatHeaderProps) {
  const { t } = useI18n();
  const permissionLabels: Record<AiPermissionMode, string> = {
    askEveryTime: t("chat.permissionAskEveryTime"),
    askSensitive: t("chat.permissionAskSensitive"),
    fullAccess: t("chat.permissionFullAccess"),
  };
  const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
  const permissionMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function closeMenu(event: MouseEvent) {
      if (!permissionMenuRef.current?.contains(event.target as Node)) {
        setPermissionMenuOpen(false);
      }
    }

    document.addEventListener("mousedown", closeMenu);
    return () => document.removeEventListener("mousedown", closeMenu);
  }, []);

  return (
    <header className="chat-header">
      <div className="chat-heading">
        <h1>{title}</h1>
        <span>{t("common.today")}</span>
      </div>

      <div className="chat-header-actions">
        <div className="permission-control" ref={permissionMenuRef}>
          <button
            className="permission-button"
            type="button"
            title={t("chat.permission")}
            aria-expanded={permissionMenuOpen}
            disabled={permissionDisabled}
            onClick={() => setPermissionMenuOpen((open) => !open)}
          >
            <ShieldCheck size={17} />
            <span>{permissionLabels[permission]}</span>
            <ChevronDown size={14} />
          </button>

          {permissionMenuOpen ? (
            <div className="permission-menu" role="menu" aria-label={t("chat.permission")}>
              {permissionOptions.map((option) => (
                <button
                  className="permission-option"
                  type="button"
                  role="menuitemradio"
                  aria-checked={permission === option}
                  key={option}
                  onClick={() => {
                    onPermissionChange(option);
                    setPermissionMenuOpen(false);
                  }}
                >
                  <span>{permissionLabels[option]}</span>
                  {permission === option ? <Check size={16} /> : null}
                </button>
              ))}
            </div>
          ) : null}
        </div>
        {compact && onClosePanel ? (
          <button
            className="icon-button"
            type="button"
            title={t("chat.closePanel")}
            aria-label={t("chat.closePanel")}
            onClick={onClosePanel}
          >
            <PanelRightClose size={18} />
          </button>
        ) : !compact ? (
          <>
            {onToggleTaskProgress ? (
              <button
                className={`icon-button${showTaskProgress ? " is-active" : ""}`}
                type="button"
                title={showTaskProgress ? t("chat.hideTaskProgress") : t("chat.showTaskProgress")}
                aria-label={showTaskProgress ? t("chat.hideTaskProgress") : t("chat.showTaskProgress")}
                aria-pressed={showTaskProgress}
                onClick={() => onToggleTaskProgress(!showTaskProgress)}
              >
                <ListChecks size={18} />
              </button>
            ) : null}
            <button
              className="icon-button"
              type="button"
              title={theme === "light" ? t("chat.darkMode") : t("chat.lightMode")}
              onClick={onToggleTheme}
            >
              {theme === "light" ? <Moon size={18} /> : <Sun size={18} />}
            </button>
            <button className="icon-button" type="button" title={t("chat.details")} aria-label={t("chat.details")}>
              <PanelRight size={18} />
            </button>
            <button className="icon-button" type="button" title={t("chat.more")} aria-label={t("chat.more")}>
              <MoreHorizontal size={19} />
            </button>
          </>
        ) : null}
      </div>
    </header>
  );
}
