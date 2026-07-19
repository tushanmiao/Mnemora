import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  AlertCircle,
  CheckCircle2,
  Download,
  FolderOpen,
  RefreshCw,
  Save,
  Upload,
} from "lucide-react";
import {
  chooseWorkingDirectory,
  exportSettingsBundle,
  importSettingsBundle,
} from "../api/appSettings";
import type { AppSettings, SettingsBundle, ThemeColor, ThemeMode } from "../types/appSettings";
import type { ModelSettings } from "../types/settings";
import "../styles/general-settings.css";

type Feedback = { kind: "success" | "error"; message: string } | null;

type GeneralSettingsPanelProps = {
  settings: AppSettings;
  modelSettings: ModelSettings;
  initialError: string | null;
  onPreview: (settings: AppSettings) => void;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
  onImported: (bundle: SettingsBundle) => void;
  onDefaultModelChange: (providerId: string, modelId: string) => Promise<void>;
};

const TOKEN_OPTIONS = [4_096, 8_192, 16_384, 32_768, 65_536, 131_072];

export function GeneralSettingsPanel({
  settings,
  modelSettings,
  initialError,
  onPreview,
  onSave,
  onImported,
  onDefaultModelChange,
}: GeneralSettingsPanelProps) {
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState(false);
  const [backupBusy, setBackupBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(
    initialError ? { kind: "error", message: initialError } : null,
  );

  useEffect(() => setDraft(settings), [settings]);
  useEffect(() => {
    if (initialError) setFeedback({ kind: "error", message: initialError });
  }, [initialError]);

  const modelOptions = useMemo(() => modelSettings.providers
    .filter((provider) => provider.enabled)
    .flatMap((provider) => provider.models
      .filter((model) => model.enabled)
      .map((model) => ({
        providerId: provider.id,
        providerName: provider.name,
        modelId: model.id,
        value: JSON.stringify([provider.id, model.id]),
        label: `${provider.name} · ${model.displayName}`,
      }))), [modelSettings.providers]);

  const defaultModelValue = modelSettings.defaultProviderId && modelSettings.defaultModelId
    ? JSON.stringify([modelSettings.defaultProviderId, modelSettings.defaultModelId])
    : "";

  const updateDraft = <Key extends keyof AppSettings>(key: Key, value: AppSettings[Key]) => {
    setDraft((current) => {
      const next = { ...current, [key]: value };
      if (key === "theme" || key === "themeColor") onPreview(next);
      return next;
    });
    setFeedback(null);
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaving(true);
    setFeedback(null);
    try {
      const saved = await onSave(draft);
      setDraft(saved);
      setFeedback({ kind: "success", message: "基础设置已保存。" });
    } catch (error) {
      setFeedback({
        kind: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  const handleExport = async () => {
    setBackupBusy(true);
    setFeedback(null);
    try {
      const exported = await exportSettingsBundle();
      if (exported) setFeedback({ kind: "success", message: "非敏感设置已导出。" });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setBackupBusy(false);
    }
  };

  const handleImport = async () => {
    setBackupBusy(true);
    setFeedback(null);
    try {
      const bundle = await importSettingsBundle();
      if (bundle) {
        onImported(bundle);
        setDraft(bundle.appSettings);
        setFeedback({ kind: "success", message: "非敏感设置已导入。" });
      }
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setBackupBusy(false);
    }
  };

  return (
    <form className="settings-content general-settings-content" onSubmit={handleSubmit} noValidate>
      <div className="settings-content-heading">
        <div>
          <h2>基础</h2>
          <span>外观、行为和 AI 客户端</span>
        </div>
        <button className="settings-button settings-button-primary" type="submit" disabled={saving}>
          <Save size={16} />
          <span>{saving ? "保存中" : "保存"}</span>
        </button>
      </div>

      <div className="general-settings-scroll">
        <section className="general-settings-section">
          <h3>外观</h3>
          <SettingRow label="界面语言">
            <select
              className="settings-input settings-select general-control"
              value={draft.interfaceLanguage}
              onChange={(event) => updateDraft("interfaceLanguage", event.target.value as AppSettings["interfaceLanguage"])}
            >
              <option value="zh">中文</option>
              <option value="en" disabled>English（后续）</option>
            </select>
          </SettingRow>
          <SettingRow label="主题">
            <SegmentedControl
              value={draft.theme}
              options={[
                { value: "system", label: "跟随系统" },
                { value: "light", label: "浅色" },
                { value: "dark", label: "深色" },
              ]}
              onChange={(value) => updateDraft("theme", value as ThemeMode)}
            />
          </SettingRow>
          <SettingRow label="主题颜色">
            <div className="theme-color-options" role="radiogroup" aria-label="主题颜色">
              {([
                ["neutral", "中性"],
                ["warm", "暖白"],
                ["cool", "冷白"],
              ] as const).map(([value, label]) => (
                <button
                  className={`theme-color-option theme-color-${value}${draft.themeColor === value ? " theme-color-option-active" : ""}`}
                  type="button"
                  role="radio"
                  aria-checked={draft.themeColor === value}
                  key={value}
                  onClick={() => updateDraft("themeColor", value as ThemeColor)}
                >
                  <span className="theme-color-swatch" aria-hidden="true" />
                  <span>{label}</span>
                </button>
              ))}
            </div>
          </SettingRow>
        </section>

        <section className="general-settings-section">
          <h3>行为</h3>
          <SettingRow label="开机启动">
            <Toggle checked={draft.launchAtStartup} onChange={(value) => updateDraft("launchAtStartup", value)} />
          </SettingRow>
          <SettingRow label="Chat 自动重试" description="只允许在尚未产生回复文本前重试；手动连接测试永不重试。">
            <Toggle checked={draft.retryEnabled} onChange={(value) => updateDraft("retryEnabled", value)} />
          </SettingRow>
          {draft.retryEnabled ? (
            <SettingRow label="最大重试次数">
              <input
                className="settings-input general-number-input"
                type="number"
                min={1}
                max={5}
                value={draft.retryAttempts}
                onChange={(event) => updateDraft("retryAttempts", Number(event.target.value))}
              />
            </SettingRow>
          ) : null}
        </section>

        <section className="general-settings-section">
          <h3>个人资料</h3>
          <SettingRow label="用户名">
            <input
              className="settings-input general-control"
              value={draft.userDisplayName}
              placeholder="选填"
              onChange={(event) => updateDraft("userDisplayName", event.target.value)}
            />
          </SettingRow>
          <SettingRow label="头像地址" description="只用于本地界面展示，不会发送给模型。" stack>
            <div className="profile-avatar-row">
              <div className="profile-avatar-preview" aria-hidden="true">
                {draft.userAvatar ? <img src={draft.userAvatar} alt="" /> : (draft.userDisplayName.trim()[0] ?? "M").toUpperCase()}
              </div>
              <input
                className="settings-input"
                type="url"
                placeholder="https://..."
                value={draft.userAvatar}
                onChange={(event) => updateDraft("userAvatar", event.target.value)}
              />
            </div>
          </SettingRow>
        </section>

        <section className="general-settings-section">
          <h3>对话默认值</h3>
          <SettingRow label="默认聊天模型">
            <select
              className="settings-input settings-select general-control"
              value={defaultModelValue}
              onChange={(event) => {
                const option = modelOptions.find((item) => item.value === event.target.value);
                if (!option) return;
                void onDefaultModelChange(option.providerId, option.modelId).catch((error) => {
                  setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
                });
              }}
            >
              <option value="">未设置</option>
              {modelOptions.map((option) => (
                <option value={option.value} key={`${option.providerId}:${option.modelId}`}>
                  {option.label}
                </option>
              ))}
            </select>
          </SettingRow>
          <SettingRow label="普通对话工作目录" description="基础 Chat 不会主动访问此目录；未来启用文件工具后使用。" stack>
            <div className="working-directory-row">
              <input
                className="settings-input"
                value={draft.workingDirectory}
                placeholder="默认：用户目录/Mnemora/workspace"
                onChange={(event) => updateDraft("workingDirectory", event.target.value)}
              />
              <button
                className="settings-button settings-button-secondary"
                type="button"
                onClick={() => void chooseWorkingDirectory().then((path) => {
                  if (path) updateDraft("workingDirectory", path);
                })}
              >
                <FolderOpen size={15} />
                <span>选择</span>
              </button>
              <button
                className="settings-button settings-button-secondary"
                type="button"
                onClick={() => updateDraft("workingDirectory", "")}
              >
                <RefreshCw size={15} />
                <span>恢复默认</span>
              </button>
            </div>
          </SettingRow>
          <SettingRow label="全局 System Prompt" description="会放在对话级 System Prompt 之前。" stack>
            <textarea
              className="settings-textarea"
              rows={5}
              value={draft.systemPrompt}
              onChange={(event) => updateDraft("systemPrompt", event.target.value)}
            />
          </SettingRow>
        </section>

        <section className="general-settings-section">
          <h3>响应</h3>
          <SettingRow label="流式输出">
            <Toggle checked={draft.streamEnabled} onChange={(value) => updateDraft("streamEnabled", value)} />
          </SettingRow>
          <SettingRow label="思考模式" description="仅在当前协议和模型支持时发送对应参数。">
            <Toggle checked={draft.thinkingEnabled} onChange={(value) => updateDraft("thinkingEnabled", value)} />
          </SettingRow>
          <SettingRow label="最大输出 Token">
            <select
              className="settings-input settings-select general-control"
              value={draft.maxOutputTokens}
              onChange={(event) => updateDraft("maxOutputTokens", Number(event.target.value))}
            >
              {TOKEN_OPTIONS.map((tokens) => <option value={tokens} key={tokens}>{tokens.toLocaleString()} tokens</option>)}
            </select>
          </SettingRow>
          <SettingRow label="回复语言">
            <select
              className="settings-input settings-select general-control"
              value={draft.responseLanguage}
              onChange={(event) => updateDraft("responseLanguage", event.target.value as AppSettings["responseLanguage"])}
            >
              <option value="followInput">跟随输入</option>
              <option value="zh">中文</option>
              <option value="zhHant">繁体中文</option>
              <option value="en">English</option>
            </select>
          </SettingRow>
        </section>

        <section className="general-settings-section">
          <h3>备份与恢复</h3>
          <div className="backup-settings-row">
            <div>
              <strong>非敏感设置备份</strong>
              <span>包含基础设置和模型结构，不包含 API Key。</span>
            </div>
            <div>
              <button className="settings-button settings-button-secondary" type="button" disabled={backupBusy} onClick={() => void handleExport()}>
                <Download size={15} /><span>导出设置</span>
              </button>
              <button className="settings-button settings-button-secondary" type="button" disabled={backupBusy} onClick={() => void handleImport()}>
                <Upload size={15} /><span>导入设置</span>
              </button>
            </div>
          </div>
        </section>
      </div>

      {feedback ? (
        <div className={`settings-feedback settings-feedback-${feedback.kind}`} role="status">
          {feedback.kind === "success" ? <CheckCircle2 size={17} /> : <AlertCircle size={17} />}
          <span>{feedback.message}</span>
        </div>
      ) : null}
    </form>
  );
}

function SettingRow({
  label,
  description,
  stack = false,
  children,
}: {
  label: string;
  description?: string;
  stack?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className={`general-setting-row${stack ? " general-setting-row-stack" : ""}`}>
      <div className="general-setting-copy">
        <strong>{label}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      <div className="general-setting-control">{children}</div>
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <button
      className={`settings-toggle${checked ? " settings-toggle-active" : ""}`}
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}

function SegmentedControl({
  value,
  options,
  onChange,
}: {
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <div className="settings-segmented">
      {options.map((option) => (
        <button
          className={value === option.value ? "settings-segmented-active" : ""}
          type="button"
          key={option.value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
