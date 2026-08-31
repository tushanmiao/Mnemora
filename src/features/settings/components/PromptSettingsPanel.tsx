import { useEffect, useMemo, useState, type FormEvent } from "react";
import { FileText, Pencil, Plus, Search, Trash2, X } from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import type { PromptTemplateInput } from "../../../types/prompt";
import type { usePromptTemplates } from "../../prompts/hooks/usePromptTemplates";
import "../styles/prompt-settings.css";

type Props = {
  state: ReturnType<typeof usePromptTemplates>;
};

type EditorDraft = PromptTemplateInput & { id?: string };

export function PromptSettingsPanel({ state }: Props) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<EditorDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error"; message: string } | null>(null);

  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return state.templates;
    return state.templates.filter((template) => (
      `${template.title}\n${template.content}`.toLocaleLowerCase().includes(normalized)
    ));
  }, [query, state.templates]);

  const openNew = () => {
    setDraft({ title: "", content: "" });
    setFeedback(null);
  };

  useEffect(() => {
    if (!state.createRequested) return;
    setDraft({ title: "", content: "" });
    setFeedback(null);
    state.consumeCreateRequest();
  }, [state.consumeCreateRequest, state.createRequested]);

  const handleSave = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!draft) return;
    if (!draft.title.trim() || !draft.content.trim()) {
      setFeedback({ kind: "error", message: t("promptSettings.validationRequired") });
      return;
    }
    setSaving(true);
    setFeedback(null);
    try {
      await state.save(draft);
      setDraft(null);
      setFeedback({ kind: "success", message: t("promptSettings.saved") });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (id: string, title: string) => {
    if (!window.confirm(t("promptSettings.deleteConfirm", { title }))) return;
    try {
      await state.remove(id);
      if (draft?.id === id) setDraft(null);
      setFeedback({ kind: "success", message: t("promptSettings.deleted") });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    }
  };

  return (
    <div className="settings-content prompt-settings-content">
      <div className="settings-content-heading">
        <div>
          <h2>{t("settings.prompts")}</h2>
          <span>{t("promptSettings.subtitle")}</span>
        </div>
        <button className="settings-button settings-button-primary" type="button" onClick={openNew}>
          <Plus size={16} />
          <span>{t("promptSettings.add")}</span>
        </button>
      </div>

      <div className="settings-scroll settings-scroll-measure prompt-settings-scroll">
        <div className="prompt-settings-toolbar">
          <Search size={15} aria-hidden="true" />
          <input
            className="settings-input"
            value={query}
            aria-label={t("promptSettings.searchPlaceholder")}
            placeholder={t("promptSettings.searchPlaceholder")}
            onChange={(event) => setQuery(event.target.value)}
          />
          <span>{t("promptSettings.count", { count: state.templates.length })}</span>
        </div>

        {feedback ? (
          <p className={`settings-feedback settings-feedback-${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
            {feedback.message}
          </p>
        ) : state.error ? <p className="settings-feedback settings-feedback-error" role="alert">{state.error}</p> : null}

        {draft ? (
          <form className="prompt-settings-editor" onSubmit={handleSave}>
            <header>
              <strong>{draft.id ? t("promptSettings.editTitle") : t("promptSettings.newTitle")}</strong>
              <button className="icon-button" type="button" title={t("common.close")} aria-label={t("common.close")} onClick={() => setDraft(null)}>
                <X size={16} />
              </button>
            </header>
            <label>
              <span>{t("promptSettings.title")}</span>
              <input
                className="settings-input"
                maxLength={80}
                value={draft.title}
                placeholder={t("promptSettings.titlePlaceholder")}
                onChange={(event) => setDraft((current) => current ? { ...current, title: event.target.value } : current)}
              />
            </label>
            <label>
              <span>{t("promptSettings.content")}</span>
              <textarea
                className="prompt-settings-textarea"
                rows={7}
                maxLength={16_000}
                value={draft.content}
                placeholder={t("promptSettings.contentPlaceholder")}
                onChange={(event) => setDraft((current) => current ? { ...current, content: event.target.value } : current)}
              />
            </label>
            <footer>
              <button className="settings-button settings-button-secondary" type="button" onClick={() => setDraft(null)}>{t("common.cancel")}</button>
              <button className="settings-button settings-button-primary" type="submit" disabled={saving}>
                {saving ? t("promptSettings.saving") : t("promptSettings.save")}
              </button>
            </footer>
          </form>
        ) : null}

        {state.loading ? (
          <div className="settings-panel-loading">{t("common.loading")}</div>
        ) : filtered.length === 0 ? (
          <div className="settings-empty">
            <FileText size={28} />
            <strong>{query.trim() ? t("promptSettings.noResultsTitle") : t("promptSettings.emptyTitle")}</strong>
            <span>{query.trim() ? t("promptSettings.noResultsDescription") : t("promptSettings.emptyDescription")}</span>
          </div>
        ) : (
          <div className="prompt-settings-list">
            {filtered.map((template) => (
              <article className="prompt-settings-item" key={template.id}>
                <button
                  className="prompt-settings-item-main"
                  type="button"
                  onClick={() => {
                    setDraft({ id: template.id, title: template.title, content: template.content });
                    setFeedback(null);
                  }}
                >
                  <span><strong>{template.title}</strong><small>{template.content}</small></span>
                  <Pencil size={15} aria-hidden="true" />
                </button>
                <button
                  className="prompt-settings-delete"
                  type="button"
                  title={t("promptSettings.delete")}
                  aria-label={`${t("promptSettings.delete")}：${template.title}`}
                  onClick={() => void handleDelete(template.id, template.title)}
                >
                  <Trash2 size={15} />
                </button>
              </article>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
