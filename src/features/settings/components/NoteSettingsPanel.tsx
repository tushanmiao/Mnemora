import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { Eye, RotateCcw, Save, Type } from "lucide-react";
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
import { EditableRangeControl } from "./EditableRangeControl";

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

      <div className="settings-scroll settings-scroll-measure">
        {feedback ? (
          <div className={`settings-feedback settings-feedback-${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
            {feedback.message}
          </div>
        ) : null}

        {/* 左控件 / 右预览：这页只有三四个设置项，纵向堆叠会空掉大半屏；
            并排还解决了一个实际问题——预览原先在很下面，拖滑块时看不见效果。 */}
        <div className="note-settings-layout">
        <section className="settings-section">
          <div className="settings-section-head">
            <Type size={16} />
            <h3>{t("notesSettings.typography")}</h3>
          </div>
          <SettingRow label={t("notesSettings.fontSize")} description={t("notesSettings.fontSizeDescription")}>
            <EditableRangeControl
              value={draft.noteFontSize}
              min={12}
              max={32}
              step={1}
              suffix="px"
              ariaLabel={t("notesSettings.fontSize")}
              onChange={(value) => update("noteFontSize", value)}
            />
          </SettingRow>
          <SettingRow label={t("notesSettings.lineHeight")} description={t("notesSettings.lineHeightDescription")}>
            <EditableRangeControl
              value={draft.noteLineHeight}
              min={1.3}
              max={2.4}
              step={0.05}
              fractionDigits={2}
              ariaLabel={t("notesSettings.lineHeight")}
              onChange={(value) => update("noteLineHeight", value)}
            />
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
          <div className="settings-row-actions">
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
        </section>

        <aside className="note-preview-pane" aria-label={t("notesSettings.preview")}>
          <div className="settings-section-head">
            <Eye size={16} />
            <h3>{t("notesSettings.preview")}</h3>
          </div>
          {/* 预览要覆盖真正会受影响的元素：标题层级、正文、列表、引用、行内代码。
              只放一段正文的话，字号和行高对层级关系的影响根本看不出来。 */}
          <div className="note-settings-preview">
            <h4>{t("notesSettings.previewTitle")}</h4>
            <p>{t("notesSettings.previewBody")}</p>
            <ul>
              <li>{t("notesSettings.previewListOne")}</li>
              <li>{t("notesSettings.previewListTwo")}</li>
            </ul>
            <blockquote>{t("notesSettings.previewQuote")}</blockquote>
            <p className="note-preview-meta">
              {draft.noteFontSize} px · {t("notesSettings.lineHeight")} {draft.noteLineHeight.toFixed(2)}
            </p>
          </div>
        </aside>
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
    <div className="settings-row">
      <div className="settings-row-copy"><strong>{label}</strong>{description ? <span>{description}</span> : null}</div>
      <div className="settings-row-control">{children}</div>
    </div>
  );
}

export default NoteSettingsPanel;
