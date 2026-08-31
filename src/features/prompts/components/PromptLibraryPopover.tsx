import { useEffect, useMemo, useRef, useState } from "react";
import { MessageSquareText, Plus, Search } from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import type { PromptTemplate } from "../../../types/prompt";

type Props = {
  templates: PromptTemplate[];
  disabled?: boolean;
  onSelect: (template: PromptTemplate) => void;
  onCreate: () => void;
  onManage: () => void;
};

export function PromptLibraryPopover({ templates, disabled = false, onSelect, onCreate, onManage }: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return templates;
    return templates.filter((template) => (
      `${template.title}\n${template.content}`.toLocaleLowerCase().includes(normalized)
    ));
  }, [query, templates]);

  useEffect(() => {
    if (!open) return undefined;
    const frame = requestAnimationFrame(() => searchRef.current?.focus());
    const closeOnPointer = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", closeOnPointer);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("mousedown", closeOnPointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div className="prompt-library" ref={rootRef}>
      <button
        className={`icon-button${open ? " prompt-library-trigger-active" : ""}`}
        type="button"
        title={t("chat.promptLibrary")}
        aria-label={t("chat.promptLibrary")}
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          setOpen((value) => !value);
          setQuery("");
        }}
      >
        <MessageSquareText size={18} />
      </button>

      {open ? (
        <div className="prompt-library-menu" role="dialog" aria-label={t("chat.promptLibrary")}>
          <header>
            <strong>{t("chat.promptLibrary")}</strong>
            <button
              type="button"
              title={t("promptSettings.add")}
              aria-label={t("promptSettings.add")}
              onClick={() => {
                setOpen(false);
                onCreate();
              }}
            >
              <Plus size={16} />
            </button>
          </header>
          <label className="prompt-library-search">
            <Search size={14} aria-hidden="true" />
            <input
              ref={searchRef}
              value={query}
              aria-label={t("chat.promptSearchPlaceholder")}
              placeholder={t("chat.promptSearchPlaceholder")}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <div className="prompt-library-list">
            {filtered.length === 0 ? (
              <button
                className="prompt-library-empty"
                type="button"
                onClick={() => {
                  setOpen(false);
                  onManage();
                }}
              >
                <strong>{query.trim() ? t("promptSettings.noResultsTitle") : t("chat.promptEmpty")}</strong>
                <span>{query.trim() ? t("promptSettings.noResultsDescription") : t("chat.promptManage")}</span>
              </button>
            ) : filtered.map((template) => (
              <button
                className="prompt-library-item"
                type="button"
                key={template.id}
                onClick={() => {
                  onSelect(template);
                  setOpen(false);
                  setQuery("");
                }}
              >
                <strong>{template.title}</strong>
                <span>{template.content}</span>
              </button>
            ))}
          </div>
          <footer>
            <button type="button" onClick={() => { setOpen(false); onManage(); }}>{t("chat.promptManage")}</button>
          </footer>
        </div>
      ) : null}
    </div>
  );
}
