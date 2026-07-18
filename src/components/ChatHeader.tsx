import { useEffect, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Moon,
  MoreHorizontal,
  PanelRight,
  ShieldCheck,
  Star,
  Sun,
} from "lucide-react";
import { AI_PERMISSION_LABELS, type AiPermissionMode } from "../types/chat";
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
  permission: AiPermissionMode;
  permissionDisabled: boolean;
  theme: "light" | "dark";
  onModelChange: (providerId: string, modelId: string) => void;
  onPermissionChange: (permission: AiPermissionMode) => void;
  onToggleTheme: () => void;
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
  permission,
  permissionDisabled,
  theme,
  onModelChange,
  onPermissionChange,
  onToggleTheme,
}: ChatHeaderProps) {
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
  const modelMenuRef = useRef<HTMLDivElement>(null);
  const permissionMenuRef = useRef<HTMLDivElement>(null);

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

  return (
    <header className="chat-header">
      <div className="chat-heading">
        <h1>{title}</h1>
        <span>今天</span>
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
            <div className="model-menu" role="listbox" aria-label="选择模型">
              {modelGroups.length === 0 ? (
                <div className="model-menu-empty">暂无可用模型</div>
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
            title="AI 权限"
            aria-expanded={permissionMenuOpen}
            disabled={permissionDisabled}
            onClick={() => setPermissionMenuOpen((open) => !open)}
          >
            <ShieldCheck size={17} />
            <span>{AI_PERMISSION_LABELS[permission]}</span>
            <ChevronDown size={14} />
          </button>

          {permissionMenuOpen ? (
            <div className="permission-menu" role="menu" aria-label="AI 权限">
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
                  <span>{AI_PERMISSION_LABELS[option]}</span>
                  {permission === option ? <Check size={16} /> : null}
                </button>
              ))}
            </div>
          ) : null}
        </div>
        <button
          className="icon-button"
          type="button"
          title={theme === "light" ? "切换到深色模式" : "切换到浅色模式"}
          onClick={onToggleTheme}
        >
          {theme === "light" ? <Moon size={18} /> : <Sun size={18} />}
        </button>
        <button className="icon-button" type="button" title="打开详情面板">
          <PanelRight size={18} />
        </button>
        <button className="icon-button" type="button" title="更多操作">
          <MoreHorizontal size={19} />
        </button>
      </div>
    </header>
  );
}
