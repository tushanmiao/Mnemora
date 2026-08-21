import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { RotateCcw, Save } from "lucide-react";
import type {
  AppSettings,
  ChineseFontFamily,
  FontPreset,
  LatinFontFamily,
} from "../../../types/appSettings";
import { useI18n } from "../../../i18n/I18nProvider";
import { NOTE_FONT_PRESET_VALUES } from "../utils/fontSettings";
import "../styles/general-settings.css";
import "../styles/note-settings.css";

type NoteSettingsPanelProps = {
  settings: AppSettings;
  initialError: string | null;
  onPreview: (settings: AppSettings) => void;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
};

type Feedback = { kind: "success" | "error"; message: string } | null;

export function NoteSettingsPanel({
  settings,
  initialError,
  onPreview,
  onSave,
}: NoteSettingsPanelProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(
    initialError ? { kind: "error", message: initialError } : null,
  );

  useEffect(() => setDraft(settings), [settings]);
  useEffect(() => {
    if (initialError) setFeedback({ kind: "error", message: initialError });
  }, [initialError]);

  const preview = (next: AppSettings) => {
    setDraft(next);
    onPreview(next);
    setFeedback(null);
  };

  const update = <Key extends keyof AppSettings>(key: Key, value: AppSettings[Key]) => {
    preview({ ...draft, [key]: value });
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaving(true);
    setFeedback(null);
    try {
      const saved = await onSave(draft);
      setDraft(saved);
      setFeedback({ kind: "success", message: t("notesSettings.saved") });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setSaving(false);
    }
  };

  const setPreset = (value: FontPreset) => {
    if (value === "custom") {
      update("noteFontPreset", value);
      return;
    }
    preview({ ...draft, noteFontPreset: value, ...NOTE_FONT_PRESET_VALUES[value] });
  };

  return (
    <form className="settings-content note-settings-content" onSubmit={handleSubmit}>
      <div className="settings-content-heading">
        <div>
          <h2>{t("settings.notes")}</h2>
          <span>{t("notesSettings.subtitle")}</span>
        </div>
        <button className="settings-button settings-button-primary" type="submit" disabled={saving}>
          <Save size={16} />
          <span>{saving ? t("common.saving") : t("common.save")}</span>
        </button>
      </div>

      <div className="note-settings-scroll">
        {feedback ? (
          <div className={`settings-feedback settings-feedback-${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
            {feedback.message}
          </div>
        ) : null}

        <section className="general-settings-section">
          <h3>{t("notesSettings.typography")}</h3>
          <SettingRow label={t("notesSettings.fontSize")} description={t("notesSettings.fontSizeDescription")}>
            <div className="font-size-control">
              <input
                type="range"
                min={12}
                max={32}
                step={1}
                value={draft.noteFontSize}
                aria-label={t("notesSettings.fontSize")}
                onChange={(event) => update("noteFontSize", Number(event.target.value))}
              />
              <output>{draft.noteFontSize} px</output>
            </div>
          </SettingRow>
          <SettingRow label={t("notesSettings.lineHeight")} description={t("notesSettings.lineHeightDescription")}>
            <div className="font-size-control">
              <input
                type="range"
                min={1.3}
                max={2.4}
                step={0.05}
                value={draft.noteLineHeight}
                aria-label={t("notesSettings.lineHeight")}
                onChange={(event) => update("noteLineHeight", Number(event.target.value))}
              />
              <output>{draft.noteLineHeight.toFixed(2)}</output>
            </div>
          </SettingRow>
          <SettingRow label={t("notesSettings.fontPreset")} description={t("notesSettings.fontPresetDescription")}>
            <div className="settings-segmented" role="group" aria-label={t("notesSettings.fontPreset")}>
              {(["system", "academic", "custom"] as const).map((value) => (
                <button
                  className={draft.noteFontPreset === value ? "settings-segmented-active" : ""}
                  type="button"
                  key={value}
                  aria-pressed={draft.noteFontPreset === value}
                  onClick={() => setPreset(value)}
                >
                  {value === "system" ? t("general.systemUi") : value === "academic" ? t("general.academic") : t("general.custom")}
                </button>
              ))}
            </div>
          </SettingRow>
          {draft.noteFontPreset === "custom" ? (
            <>
              <SettingRow label={t("general.chineseFont")}>
                <select
                  className="settings-input settings-select general-control"
                  value={draft.noteChineseFontFamily}
                  onChange={(event) => update("noteChineseFontFamily", event.target.value as ChineseFontFamily)}
                >
                  <option value="system">{t("general.systemChinese")}</option>
                  <option value="microsoftYaHei">微软雅黑</option>
                  <option value="simsun">宋体</option>
                  <option value="notoSansCjk">Noto Sans CJK</option>
                  <option value="notoSerifCjk">Noto Serif CJK</option>
                </select>
              </SettingRow>
              <SettingRow label={t("general.latinFont")}>
                <select
                  className="settings-input settings-select general-control"
                  value={draft.noteLatinFontFamily}
                  onChange={(event) => update("noteLatinFontFamily", event.target.value as LatinFontFamily)}
                >
                  <option value="system">{t("general.systemLatin")}</option>
                  <option value="segoeUi">Segoe UI</option>
                  <option value="inter">Inter</option>
                  <option value="timesNewRoman">Times New Roman</option>
                  <option value="georgia">Georgia</option>
                </select>
              </SettingRow>
            </>
          ) : null}
        </section>

        <section className="note-settings-preview" aria-label={t("notesSettings.preview")}>
          <span>{t("notesSettings.preview")}</span>
          <h3>{t("notesSettings.previewTitle")}</h3>
          <p>{t("notesSettings.previewBody")}</p>
          <blockquote>{t("notesSettings.previewQuote")}</blockquote>
        </section>

        <div className="appearance-reset-row">
          <button
            className="settings-button settings-button-secondary"
            type="button"
            onClick={() => preview({
              ...draft,
              noteFontSize: 16,
              noteLineHeight: 1.85,
              noteFontPreset: "system",
              noteChineseFontFamily: "system",
              noteLatinFontFamily: "system",
            })}
          >
            <RotateCcw size={15} />{t("notesSettings.reset")}
          </button>
        </div>
      </div>
    </form>
  );
}

function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="general-setting-row">
      <div className="general-setting-copy"><strong>{label}</strong>{description ? <span>{description}</span> : null}</div>
      <div className="general-setting-control">{children}</div>
    </div>
  );
}

export default NoteSettingsPanel;
