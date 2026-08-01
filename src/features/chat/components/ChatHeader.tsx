import { useEffect, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Moon,
  MoreHorizontal,
  PanelRight,
  PanelRightClose,
  ShieldCheck,
  Star,
  Sun,
} from "lucide-react";
import { AI_PERMISSION_LABELS, type AiPermissionMode } from "../../../types/chat";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/chat-header.css";

const permissionOptions = Object.keys(AI_PERMISSION_LABELS) as AiPermissionMode[];

type ChatHeaderProps = {
  title: string;
  modelLabel: string;
  modelTitle: string;
  modelConfigured: boolean;
  modelGroups: ModelSelectorGroup[];
  selectedProviderId: string | null;
  selectedModelId: string | null;
  modelSelectionDisabled: boolean;
  modelMenuRequest?: number;
  permission: AiPermissionMode;
  permissionDisabled: boolean;
  theme: "light" | "dark";
  compact?: boolean;
  onModelChange: (providerId: string, modelId: string) => void;
  onPermissionChange: (permission: AiPermissionMode) => void;
  onToggleTheme: () => void;
  onClosePanel?: () => void;
};

export type ModelSelectorOption = {
  id: string;
  displayName: string;
  apiModel: string;
  isDefault: boolean;
};

export type ModelSelectorGroup = {
  providerId: string;
  providerName: string;
  models: ModelSelectorOption[];
};

export function ChatHeader({
  title,
  modelLabel,
  modelTitle,
  modelConfigured,
  modelGroups,
  selectedProviderId,
  selectedModelId,
  modelSelectionDisabled,
  modelMenuRequest,
  permission,
  permissionDisabled,
  theme,
  compact = false,
  onModelChange,
  onPermissionChange,
  onToggleTheme,
  onClosePanel,
}: ChatHeaderProps) {
  const { t } = useI18n();
  const permissionLabels: Record<AiPermissionMode, string> = {
    askEveryTime: t("chat.permissionAskEveryTime"),
    askSensitive: t("chat.permissionAskSensitive"),
    fullAccess: t("chat.permissionFullAccess"),
  };
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
  const modelMenuRef = useRef<HTMLDivElement>(null);
  const permissionMenuRef = useRef<HTMLDivElement>(null);
  const previousModelMenuRequest = useRef(modelMenuRequest);

  useEffect(() => {
    function closeMenu(event: MouseEvent) {
      if (!modelMenuRef.current?.contains(event.target as Node)) {
        setModelMenuOpen(false);
      }
      if (!permissionMenuRef.current?.contains(event.target as Node)) {
        setPermissionMenuOpen(false);
      }
    }

    document.addEventListener("mousedown", closeMenu);
    return () => document.removeEventListener("mousedown", closeMenu);
  }, []);

  useEffect(() => {
    if (modelMenuRequest === previousModelMenuRequest.current) return;
    previousModelMenuRequest.current = modelMenuRequest;
    if (modelMenuRequest === undefined) return;
    if (!modelSelectionDisabled) {
      setModelMenuOpen(true);
      setPermissionMenuOpen(false);
    }
  }, [modelMenuRequest, modelSelectionDisabled]);

  return (
    <header className="chat-header">
      <div className="chat-heading">
        <h1>{title}</h1>
        <span>{t("common.today")}</span>
      </div>

      <div className="chat-header-actions">
        <div className="model-selector" ref={modelMenuRef}>
          <button
            className="model-button"
            type="button"
            title={modelTitle}
            aria-haspopup="listbox"
            aria-expanded={modelMenuOpen}
            disabled={modelSelectionDisabled}
            onClick={() => {
              setModelMenuOpen((open) => !open);
              setPermissionMenuOpen(false);
            }}
          >
            <span
              className={`model-status${modelConfigured ? " model-status-configured" : ""}`}
              aria-hidden="true"
            />
            <span>{modelLabel}</span>
            <ChevronDown size={15} className={modelMenuOpen ? "model-chevron-open" : ""} />
          </button>

          {modelMenuOpen ? (
            <div className="model-menu" role="listbox" aria-label={t("chat.selectModel")}>
              {modelGroups.length === 0 ? (
                <div className="model-menu-empty">{t("chat.noModels")}</div>
              ) : modelGroups.map((group) => (
                <section className="model-menu-group" key={group.providerId}>
                  <div className="model-menu-group-heading">
                    <span className="model-menu-group-dot" aria-hidden="true" />
                    <span>{group.providerName}</span>
                  </div>
                  {group.models.map((model) => {
                    const selected = selectedProviderId === group.providerId
                      && selectedModelId === model.id;
                    return (
                      <button
                        className={`model-option${selected ? " model-option-selected" : ""}`}
                        type="button"
                        role="option"
                        aria-selected={selected}
                        key={`${group.providerId}:${model.id}`}
                        onClick={() => {
                          onModelChange(group.providerId, model.id);
                          setModelMenuOpen(false);
                        }}
                      >
                        <span className="model-option-copy">
                          <span className="model-option-name">{model.displayName}</span>
                          {model.apiModel !== model.displayName ? (
                            <span className="model-option-api">{model.apiModel}</span>
                          ) : null}
                        </span>
                        <span className="model-option-mark" aria-hidden="true">
                          {selected ? <Check size={15} /> : model.isDefault ? <Star size={14} fill="currentColor" /> : null}
                        </span>
                      </button>
                    );
                  })}
                </section>
              ))}
            </div>
          ) : null}
        </div>
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
