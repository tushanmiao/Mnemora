import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, Star } from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";

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

type ModelSelectorProps = {
  groups: ModelSelectorGroup[];
  selectedProviderId: string | null;
  selectedModelId: string | null;
  disabled?: boolean;
  menuRequest?: number;
  label: string;
  title: string;
  configured: boolean;
  onChange: (providerId: string, modelId: string) => void;
};

export function ModelSelector({
  groups,
  selectedProviderId,
  selectedModelId,
  disabled = false,
  menuRequest,
  label,
  title,
  configured,
  onChange,
}: ModelSelectorProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const previousRequest = useRef(menuRequest);

  useEffect(() => {
    const close = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  useEffect(() => {
    if (menuRequest === previousRequest.current) return;
    previousRequest.current = menuRequest;
    if (menuRequest !== undefined && !disabled) setOpen(true);
  }, [disabled, menuRequest]);

  return (
    <div className="composer-model-control" ref={rootRef}>
      <button
        className="composer-model-button"
        type="button"
        title={title}
        aria-label={title}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
      >
        <span className={`model-status${configured ? " model-status-configured" : ""}`} aria-hidden="true" />
        <span className="composer-model-label">{label}</span>
        <ChevronDown size={14} className={open ? "model-chevron-open" : ""} />
      </button>
      {open ? (
        <div className="model-menu composer-model-menu" role="listbox" aria-label={t("chat.selectModel")}>
          {groups.length === 0 ? <div className="model-menu-empty">{t("chat.noModels")}</div> : groups.map((group) => (
            <section className="model-menu-group" key={group.providerId}>
              <div className="model-menu-group-heading"><span className="model-menu-group-dot" aria-hidden="true" /><span>{group.providerName}</span></div>
              {group.models.map((model) => {
                const selected = selectedProviderId === group.providerId && selectedModelId === model.id;
                return (
                  <button
                    className={`model-option${selected ? " model-option-selected" : ""}`}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    key={`${group.providerId}:${model.id}`}
                    onClick={() => { onChange(group.providerId, model.id); setOpen(false); }}
                  >
                    <span className="model-option-copy">
                      <span className="model-option-name">{model.displayName}</span>
                      {model.apiModel !== model.displayName ? <span className="model-option-api">{model.apiModel}</span> : null}
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
  );
}
