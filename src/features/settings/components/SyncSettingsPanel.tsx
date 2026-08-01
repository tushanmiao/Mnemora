import { useEffect, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  Cloud,
  ExternalLink,
  FolderOpen,
  KeyRound,
  LoaderCircle,
  RefreshCw,
  Save,
  Trash2,
} from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import {
  DEFAULT_SYNC_SETTINGS,
  type SyncResult,
  type SyncSettings,
  type SyncTarget,
} from "../../../types/syncSettings";
import {
  chooseObsidianVault,
  deleteFeishuAppSecret,
  deleteNotionToken,
  loadSyncSettings,
  runNoteSync,
  saveSyncSettings,
  setFeishuAppSecret,
  setNotionToken,
} from "../api/syncSettings";
import "../styles/sync-settings.css";

type Feedback = { type: "success" | "error"; message: string } | null;

export function SyncSettingsPanel() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<SyncSettings>(DEFAULT_SYNC_SETTINGS);
  const [token, setToken] = useState("");
  const [feishuSecret, setFeishuSecret] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [result, setResult] = useState<SyncResult | null>(null);

  useEffect(() => {
    let active = true;
    void loadSyncSettings()
      .then((loaded) => {
        if (active) setSettings(loaded);
      })
      .catch((reason) => {
        if (active) setFeedback({ type: "error", message: errorMessage(reason) });
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  const patchSettings = (patch: Partial<SyncSettings>) => {
    setSettings((current) => ({ ...current, ...patch }));
    setFeedback(null);
  };

  const selectTarget = (target: SyncTarget) => {
    patchSettings({ target });
    setResult(null);
  };

  const chooseVault = async () => {
    const path = await chooseObsidianVault();
    if (!path) return;
    setSettings((current) => ({
      ...current,
      obsidian: { ...current.obsidian, vaultPath: path },
    }));
  };

  const save = async () => {
    setSaving(true);
    setFeedback(null);
    try {
      const saved = await saveSyncSettings(settings);
      setSettings(saved);
      setFeedback({ type: "success", message: t("sync.saved") });
    } catch (reason) {
      setFeedback({ type: "error", message: errorMessage(reason) });
    } finally {
      setSaving(false);
    }
  };

  const saveToken = async () => {
    if (!token.trim()) {
      setFeedback({ type: "error", message: t("sync.tokenRequired") });
      return;
    }
    setSaving(true);
    setFeedback(null);
    try {
      await setNotionToken(token);
      setToken("");
      setSettings((current) => ({
        ...current,
        notion: { ...current.notion, hasToken: true },
      }));
      setFeedback({ type: "success", message: t("sync.tokenSaved") });
    } catch (reason) {
      setFeedback({ type: "error", message: errorMessage(reason) });
    } finally {
      setSaving(false);
    }
  };

  const removeToken = async () => {
    if (!window.confirm(t("sync.deleteTokenConfirm"))) return;
    setSaving(true);
    setFeedback(null);
    try {
      await deleteNotionToken();
      setSettings((current) => ({
        ...current,
        notion: { ...current.notion, hasToken: false },
      }));
      setFeedback({ type: "success", message: t("sync.tokenDeleted") });
    } catch (reason) {
      setFeedback({ type: "error", message: errorMessage(reason) });
    } finally {
      setSaving(false);
    }
  };

  const saveFeishuSecret = async () => {
    if (!feishuSecret.trim()) {
      setFeedback({ type: "error", message: t("sync.feishuSecretRequired") });
      return;
    }
    setSaving(true);
    setFeedback(null);
    try {
      await setFeishuAppSecret(feishuSecret);
      setFeishuSecret("");
      setSettings((current) => ({
        ...current,
        feishu: { ...current.feishu, hasAppSecret: true },
      }));
      setFeedback({ type: "success", message: t("sync.feishuSecretSaved") });
    } catch (reason) {
      setFeedback({ type: "error", message: errorMessage(reason) });
    } finally {
      setSaving(false);
    }
  };

  const removeFeishuSecret = async () => {
    if (!window.confirm(t("sync.deleteFeishuSecretConfirm"))) return;
    setSaving(true);
    setFeedback(null);
    try {
      await deleteFeishuAppSecret();
      setSettings((current) => ({
        ...current,
        feishu: { ...current.feishu, hasAppSecret: false },
      }));
      setFeedback({ type: "success", message: t("sync.feishuSecretDeleted") });
    } catch (reason) {
      setFeedback({ type: "error", message: errorMessage(reason) });
    } finally {
      setSaving(false);
    }
  };

  const runSync = async () => {
    if (settings.target === "feishu" && !settings.feishu.appId.trim()) {
      setFeedback({ type: "error", message: t("sync.feishuAppIdRequired") });
      return;
    }
    if (settings.target === "feishu" && !settings.feishu.hasAppSecret) {
      setFeedback({ type: "error", message: t("sync.feishuSecretRequired") });
      return;
    }
    setSyncing(true);
    setFeedback(null);
    setResult(null);
    try {
      const saved = await saveSyncSettings(settings);
      setSettings(saved);
      const nextResult = await runNoteSync();
      setResult(nextResult);
      setFeedback({
        type: nextResult.failed > 0 ? "error" : "success",
        message: t("sync.completed", {
          succeeded: nextResult.succeeded,
          skipped: nextResult.skipped,
          failed: nextResult.failed,
        }),
      });
    } catch (reason) {
      setFeedback({ type: "error", message: errorMessage(reason) });
    } finally {
      setSyncing(false);
    }
  };

  return (
    <section className="settings-content sync-settings-content" aria-busy={loading || saving || syncing}>
      <div className="settings-content-heading">
        <div><h2>{t("sync.title")}</h2><span>{t("sync.subtitle")}</span></div>
        <button className="settings-button settings-button-primary" type="button" disabled={loading || saving || syncing} onClick={() => void save()}>
          {saving ? <LoaderCircle className="settings-spin" size={15} /> : <Save size={15} />}
          <span>{saving ? t("common.saving") : t("common.save")}</span>
        </button>
      </div>

      <div className="sync-settings-scroll">
        {feedback ? (
          <div className={`settings-feedback settings-feedback-${feedback.type}`}>
            {feedback.type === "success" ? <CheckCircle2 size={17} /> : <AlertCircle size={17} />}
            <span>{feedback.message}</span>
          </div>
        ) : null}

        <section className="sync-section">
          <div className="sync-section-heading"><div><Cloud size={17} /><h3>{t("sync.configuration")}</h3></div><span>{t("sync.manualOnly")}</span></div>
          <label className="settings-switch-label sync-enable-row">
            <input type="checkbox" checked={settings.enabled} onChange={(event) => patchSettings({ enabled: event.target.checked })} />
            <span>{t("sync.enabled")}</span>
          </label>
          <div className="settings-segmented sync-targets" aria-label={t("sync.target")}>
            <button className={settings.target === "feishu" ? "settings-segmented-active" : ""} type="button" onClick={() => selectTarget("feishu")}>{t("sync.feishu")}</button>
            <button className={settings.target === "obsidian" ? "settings-segmented-active" : ""} type="button" onClick={() => selectTarget("obsidian")}>Obsidian</button>
            <button className={settings.target === "notion" ? "settings-segmented-active" : ""} type="button" onClick={() => selectTarget("notion")}>Notion</button>
          </div>
          <div className="sync-options">
            <label className="settings-switch-label"><input type="checkbox" checked={settings.includeMetadata} onChange={(event) => patchSettings({ includeMetadata: event.target.checked })} /><span>{t("sync.includeMetadata")}</span></label>
            <label className="settings-switch-label"><input type="checkbox" checked={settings.includeAnnotations} onChange={(event) => patchSettings({ includeAnnotations: event.target.checked })} /><span>{t("sync.includeAnnotations")}</span></label>
          </div>
        </section>

        {settings.target === "feishu" ? (
          <section className="sync-section">
            <div className="sync-section-heading"><div><Cloud size={17} /><h3>{t("sync.feishu")}</h3></div><span>{t("sync.preferred")}</span></div>
            <div className="settings-field">
              <label htmlFor="sync-feishu-app-id">{t("sync.feishuAppId")}</label>
              <input
                id="sync-feishu-app-id"
                className="settings-input"
                value={settings.feishu.appId}
                placeholder="cli_..."
                onChange={(event) => setSettings((current) => ({
                  ...current,
                  feishu: { ...current.feishu, appId: event.target.value },
                }))}
              />
              <span className="sync-field-help">{t("sync.feishuAppIdDescription")}</span>
            </div>
            <div className="settings-field">
              <label htmlFor="sync-feishu-folder-token">{t("sync.feishuFolderToken")}</label>
              <input
                id="sync-feishu-folder-token"
                className="settings-input"
                value={settings.feishu.folderToken}
                placeholder="fldcn..."
                onChange={(event) => setSettings((current) => ({
                  ...current,
                  feishu: { ...current.feishu, folderToken: event.target.value },
                }))}
              />
              <span className="sync-field-help">{t("sync.feishuFolderTokenDescription")}</span>
            </div>
            <div className="settings-field">
              <div className="settings-label-row"><label htmlFor="sync-feishu-secret">App Secret</label><span className={settings.feishu.hasAppSecret ? "sync-token-ready" : ""}>{settings.feishu.hasAppSecret ? t("sync.configured") : t("sync.notConfigured")}</span></div>
              <div className="sync-secret-row">
                <input id="sync-feishu-secret" className="settings-input" type="password" autoComplete="off" value={feishuSecret} placeholder={settings.feishu.hasAppSecret ? t("sync.replaceFeishuSecret") : "App Secret"} onChange={(event) => setFeishuSecret(event.target.value)} />
                <button className="settings-button settings-button-secondary" type="button" disabled={saving} onClick={() => void saveFeishuSecret()}><KeyRound size={15} /><span>{t("sync.saveFeishuSecret")}</span></button>
                {settings.feishu.hasAppSecret ? <button className="settings-icon-danger" type="button" title={t("sync.deleteFeishuSecret")} aria-label={t("sync.deleteFeishuSecret")} onClick={() => void removeFeishuSecret()}><Trash2 size={15} /></button> : null}
              </div>
              <span className="sync-field-help">{t("sync.feishuSecretDescription")}</span>
            </div>
            <div className="sync-on-demand-note">
              <strong>{t("sync.feishuOnDemandTitle")}</strong>
              <span>{t("sync.feishuOnDemandDescription")}</span>
            </div>
          </section>
        ) : settings.target === "obsidian" ? (
          <section className="sync-section">
            <div className="sync-section-heading"><div><FolderOpen size={17} /><h3>Obsidian</h3></div><span>{t("sync.obsidianDescription")}</span></div>
            <div className="settings-field">
              <label htmlFor="sync-vault-path">{t("sync.vaultPath")}</label>
              <div className="sync-path-row">
                <input id="sync-vault-path" className="settings-input" value={settings.obsidian.vaultPath} readOnly placeholder={t("sync.vaultPlaceholder")} />
                <button className="settings-button settings-button-secondary" type="button" onClick={() => void chooseVault()}><FolderOpen size={15} /><span>{t("general.choose")}</span></button>
              </div>
            </div>
            <div className="settings-field">
              <label htmlFor="sync-vault-directory">{t("sync.directory")}</label>
              <input id="sync-vault-directory" className="settings-input" value={settings.obsidian.directory} placeholder="Mnemora" onChange={(event) => setSettings((current) => ({ ...current, obsidian: { ...current.obsidian, directory: event.target.value } }))} />
              <span className="sync-field-help">{t("sync.directoryDescription")}</span>
            </div>
          </section>
        ) : (
          <section className="sync-section">
            <div className="sync-section-heading"><div><ExternalLink size={17} /><h3>Notion</h3></div><span>{t("sync.notionDescription")}</span></div>
            <div className="settings-field">
              <label htmlFor="sync-notion-parent">{t("sync.parentPageId")}</label>
              <input id="sync-notion-parent" className="settings-input" value={settings.notion.parentPageId} placeholder={t("sync.parentPagePlaceholder")} onChange={(event) => setSettings((current) => ({ ...current, notion: { ...current.notion, parentPageId: event.target.value } }))} />
              <span className="sync-field-help">{t("sync.parentPageDescription")}</span>
            </div>
            <div className="settings-field">
              <div className="settings-label-row"><label htmlFor="sync-notion-token">Integration Token</label><span className={settings.notion.hasToken ? "sync-token-ready" : ""}>{settings.notion.hasToken ? t("sync.configured") : t("sync.notConfigured")}</span></div>
              <div className="sync-secret-row">
                <input id="sync-notion-token" className="settings-input" type="password" autoComplete="off" value={token} placeholder={settings.notion.hasToken ? t("sync.replaceToken") : "secret_..."} onChange={(event) => setToken(event.target.value)} />
                <button className="settings-button settings-button-secondary" type="button" disabled={saving} onClick={() => void saveToken()}><KeyRound size={15} /><span>{t("sync.saveToken")}</span></button>
                {settings.notion.hasToken ? <button className="settings-icon-danger" type="button" title={t("sync.deleteToken")} aria-label={t("sync.deleteToken")} onClick={() => void removeToken()}><Trash2 size={15} /></button> : null}
              </div>
              <span className="sync-field-help">{t("sync.tokenDescription")}</span>
            </div>
          </section>
        )}

        <section className="sync-section sync-run-section">
          <div><h3>{t("sync.manualTitle")}</h3><p>{t("sync.manualDescription")}</p></div>
          <button className="settings-button settings-button-primary" type="button" disabled={loading || saving || syncing || !settings.enabled} onClick={() => void runSync()}>
            {syncing ? <LoaderCircle className="settings-spin" size={15} /> : <RefreshCw size={15} />}
            <span>{syncing ? t("sync.syncing") : t("sync.syncAll")}</span>
          </button>
        </section>

        {result?.items.length ? (
          <section className="sync-result-list" aria-label={t("sync.results")}>
            {result.items.map((item) => (
              <div className="sync-result-row" key={`${item.noteId}-${item.status}`}>
                <span className={`sync-result-status sync-result-${item.status}`} />
                <div><strong>{item.title || item.noteId}</strong><span>{item.message}</span></div>
              </div>
            ))}
          </section>
        ) : null}
      </div>
    </section>
  );
}

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}
