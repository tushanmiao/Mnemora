import { getName, getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  BookOpen,
  CheckCircle2,
  Cpu,
  Database,
  ExternalLink,
  FileText,
  Gauge,
  LoaderCircle,
  MessagesSquare,
  RefreshCw,
  Save,
  ShieldCheck,
  Sparkles,
  Wifi,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useI18n } from "../../../i18n/I18nProvider";
import type { AppSettings, UpdateProxyMode } from "../../../types/appSettings";
import type { SignedUpdateInfo, UpdateCheckResult } from "../../../types/appUpdate";
import {
  checkApplicationUpdate,
  checkSignedApplicationUpdate,
  discardSignedApplicationUpdate,
  downloadAndInstallSignedUpdate,
} from "../api/appUpdate";
import { testWebNetworkConnection } from "../api/network";
import "../styles/about-settings.css";

type AppMetadata = {
  name: string;
  version: string;
  tauriVersion: string;
};

const FALLBACK_METADATA: AppMetadata = {
  name: "Mnemora",
  version: "开发环境",
  tauriVersion: "不可用",
};

const PROJECT_URL = "https://github.com/tushanmiao/Mnemora";
const RELEASE_URL = PROJECT_URL + "/releases";
const ISSUES_URL = PROJECT_URL + "/issues";

type AboutSettingsPanelProps = {
  settings: AppSettings;
  onSaveSettings: (settings: AppSettings) => Promise<AppSettings>;
};

export function AboutSettingsPanel({ settings, onSaveSettings }: AboutSettingsPanelProps) {
  const { t } = useI18n();
  const [metadata, setMetadata] = useState<AppMetadata>(FALLBACK_METADATA);
  const [metadataLoading, setMetadataLoading] = useState(true);
  const [metadataError, setMetadataError] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<"idle" | "checking" | "complete" | "downloading" | "installing" | "error">("idle");
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null);
  const [signedUpdate, setSignedUpdate] = useState<SignedUpdateInfo | null>(null);
  const [updateError, setUpdateError] = useState("");
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [downloadTotal, setDownloadTotal] = useState<number | null>(null);
  const [proxyMode, setProxyMode] = useState<UpdateProxyMode>(settings.updateProxy.mode);
  const [proxyUrl, setProxyUrl] = useState(settings.updateProxy.url);
  const [proxySaving, setProxySaving] = useState(false);
  const [proxyTesting, setProxyTesting] = useState(false);
  const [proxyFeedback, setProxyFeedback] = useState<{ kind: "success" | "error"; message: string } | null>(null);

  useEffect(() => {
    setProxyMode(settings.updateProxy.mode);
    setProxyUrl(settings.updateProxy.url);
  }, [settings.updateProxy.mode, settings.updateProxy.url]);

  useEffect(() => () => {
    void discardSignedApplicationUpdate();
  }, []);

  useEffect(() => {
    let active = true;

    if (!isTauri()) {
      setMetadataLoading(false);
      return () => {
        active = false;
      };
    }

    void Promise.all([getName(), getVersion(), getTauriVersion()])
      .then(([name, version, tauriVersion]) => {
        if (!active) return;
        setMetadata({ name, version, tauriVersion });
      })
      .catch(() => {
        if (active) setMetadataError(true);
      })
      .finally(() => {
        if (active) setMetadataLoading(false);
      });

    return () => {
      active = false;
    };
  }, []);

  const openExternal = async (url: string) => {
    try {
      if (isTauri()) {
        await openUrl(url);
      } else {
        window.open(url, "_blank", "noopener,noreferrer");
      }
    } catch {
      setMetadataError(true);
    }
  };

  const desktopCore = metadata.tauriVersion === "不可用"
    ? "Tauri 2"
    : "Tauri " + metadata.tauriVersion;

  const proxyDirty = proxyMode !== settings.updateProxy.mode || proxyUrl.trim() !== settings.updateProxy.url;
  const proxyValidationKey = proxyMode === "manual" ? validateProxyUrl(proxyUrl) : null;
  const proxyValidationError = proxyValidationKey ? t(proxyValidationKey) : null;

  const saveProxySettings = async () => {
    if (proxyValidationError) {
      setProxyFeedback({ kind: "error", message: proxyValidationError });
      throw new Error(proxyValidationError);
    }
    setProxySaving(true);
    setProxyFeedback(null);
    try {
      const saved = await onSaveSettings({
        ...settings,
        updateProxy: { mode: proxyMode, url: proxyUrl.trim() },
      });
      setProxyMode(saved.updateProxy.mode);
      setProxyUrl(saved.updateProxy.url);
      setProxyFeedback({ kind: "success", message: t("about.proxySaved") });
      await discardSignedApplicationUpdate();
      setSignedUpdate(null);
      return saved;
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setProxyFeedback({ kind: "error", message });
      throw reason;
    } finally {
      setProxySaving(false);
    }
  };

  const testProxyConnection = async () => {
    setProxyTesting(true);
    setProxyFeedback(null);
    try {
      if (proxyDirty) await saveProxySettings();
      const report = await testWebNetworkConnection();
      const summary = report.probes
        .map((probe) => `${probe.id === "search" ? t("about.proxyProbe.search") : t("about.proxyProbe.page")}：${probe.ok ? t("about.proxyProbe.ok") : probe.message} (${probe.durationMs} ms)`)
        .join("；");
      const allOk = report.probes.every((probe) => probe.ok);
      const route = report.proxyAddress
        ? `${t("about.proxyProbe.route")} ${report.proxyAddress}`
        : t("about.proxyProbe.directRoute");
      setProxyFeedback({
        kind: allOk ? "success" : "error",
        message: `${route}；${summary}`,
      });
    } catch (reason) {
      setProxyFeedback({
        kind: "error",
        message: reason instanceof Error ? reason.message : String(reason),
      });
    } finally {
      setProxyTesting(false);
    }
  };

  const checkUpdate = async () => {
    setUpdateStatus("checking");
    setUpdateError("");
    setSignedUpdate(null);
    setDownloadProgress(0);
    setDownloadedBytes(0);
    setDownloadTotal(null);
    try {
      if (proxyDirty) await saveProxySettings();
      const result = await checkApplicationUpdate();
      setUpdateInfo(result);
      if (result.available) {
        try {
          setSignedUpdate(await checkSignedApplicationUpdate());
        } catch (reason) {
          setUpdateError(reason instanceof Error ? reason.message : String(reason));
        }
      } else {
        await discardSignedApplicationUpdate();
      }
      setUpdateStatus("complete");
    } catch (reason) {
      setUpdateInfo(null);
      setUpdateError(reason instanceof Error ? reason.message : String(reason));
      setUpdateStatus("error");
    }
  };

  const installUpdate = async () => {
    if (!signedUpdate) return;
    setUpdateStatus("downloading");
    setUpdateError("");
    setDownloadProgress(0);
    setDownloadedBytes(0);
    setDownloadTotal(null);
    try {
      let receivedBytes = 0;
      let totalBytes: number | null = null;
      await downloadAndInstallSignedUpdate((progress) => {
        if (progress.finished) {
          setDownloadProgress(100);
          setUpdateStatus("installing");
          return;
        }
        receivedBytes = progress.downloadedBytes;
        totalBytes = progress.totalBytes;
        setDownloadedBytes(receivedBytes);
        setDownloadTotal(totalBytes);
        if (totalBytes) setDownloadProgress(Math.min(100, Math.round((receivedBytes / totalBytes) * 100)));
      });
    } catch (reason) {
      setUpdateError(reason instanceof Error ? reason.message : String(reason));
      setUpdateStatus("error");
    }
  };

  const updateBusy = proxySaving || proxyTesting || updateStatus === "checking" || updateStatus === "downloading" || updateStatus === "installing";

  return (
    <section className="settings-content about-settings-content" aria-label="关于 Mnemora">
      <div className="settings-content-heading">
        <div>
          <h2>关于</h2>
          <span>项目状态、运行环境和安全边界</span>
        </div>
        <div className="about-version-mark">
          <CheckCircle2 size={15} />
          <span>测试版</span>
        </div>
      </div>

      <div className="settings-scroll settings-scroll-measure">
        <section className="about-hero" aria-labelledby="about-product-name">
          <div className="about-hero-icon" aria-hidden="true">
            <Sparkles size={25} />
          </div>
          <div className="about-hero-copy">
            <h3 id="about-product-name">{metadata.name}</h3>
            <p>轻量、流畅、可扩展的桌面 AI 对话与阅读工作台。</p>
            <span>
              使用 Tauri 2、Rust、React 和 TypeScript 构建，重点优化多模型接入、流式体验和本地数据边界。
            </span>
          </div>
        </section>

        <section className="settings-section" aria-labelledby="about-runtime-heading">
          <div className="settings-section-head">
            <Cpu size={16} />
            <h3 id="about-runtime-heading">运行环境</h3>
          </div>
          <div className="settings-stat-grid">
            <MetadataItem label="应用版本" value={metadata.version} loading={metadataLoading} />
            <MetadataItem label="桌面核心" value={desktopCore} loading={metadataLoading} />
            <MetadataItem label="前端运行时" value="React 19 · TypeScript 5.8" />
            <MetadataItem label="模型协议" value="OpenAI · Anthropic · Gemini" />
          </div>
          {metadataError ? (
            <p className="about-inline-warning">部分运行环境信息读取失败，当前显示的是可用的备用信息。</p>
          ) : null}
        </section>

        <section className="settings-section" aria-labelledby="about-update-heading">
          <div className="settings-section-head">
            <RefreshCw size={16} />
            <h3 id="about-update-heading">{t("about.updateTitle")}</h3>
            <div className="settings-section-head-actions">
              <button className="settings-button settings-button-secondary" type="button" disabled={updateBusy || !isTauri()} onClick={() => void checkUpdate()}>
                {updateBusy ? <LoaderCircle className="settings-spin" size={15} /> : <RefreshCw size={15} />}
                <span>{updateStatus === "checking" ? t("about.checkingUpdate") : t("about.checkUpdate")}</span>
              </button>
            </div>
          </div>
          <p className="settings-section-note">{t("about.updateDescription")}</p>
          <div className="about-proxy-settings" aria-labelledby="about-proxy-heading">
            <div className="about-proxy-copy">
              <div className="about-proxy-title">
                <Gauge size={15} />
                <strong id="about-proxy-heading">{t("about.proxyTitle")}</strong>
              </div>
              <span>{t(`about.proxyDescription.${proxyMode}`)}</span>
            </div>
            <div className="about-proxy-controls">
              <div className="settings-segmented about-proxy-modes" aria-label={t("about.proxyMode")}>
                {(["system", "direct", "manual"] as const).map((mode) => (
                  <button
                    className={proxyMode === mode ? "settings-segmented-active" : ""}
                    type="button"
                    key={mode}
                    aria-pressed={proxyMode === mode}
                    disabled={updateBusy}
                    onClick={() => {
                      setProxyMode(mode);
                      if (mode !== "manual" && validateProxyUrl(proxyUrl)) setProxyUrl("");
                      setProxyFeedback(null);
                    }}
                  >
                    {t(`about.proxyMode.${mode}`)}
                  </button>
                ))}
              </div>
              {proxyMode === "manual" ? (
                <div className="about-proxy-address">
                  <label htmlFor="about-update-proxy-url">{t("about.proxyAddress")}</label>
                  <input
                    id="about-update-proxy-url"
                    className={`settings-input${proxyValidationError ? " settings-input-error" : ""}`}
                    type="url"
                    inputMode="url"
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    value={proxyUrl}
                    placeholder="http://127.0.0.1:7890"
                    disabled={updateBusy}
                    aria-invalid={Boolean(proxyValidationError)}
                    aria-describedby={proxyValidationError ? "about-update-proxy-error" : undefined}
                    onChange={(event) => {
                      setProxyUrl(event.target.value);
                      setProxyFeedback(null);
                    }}
                  />
                  {proxyValidationError ? <span id="about-update-proxy-error" className="settings-field-error">{proxyValidationError}</span> : null}
                </div>
              ) : null}
              <div className="about-proxy-actions">
                <button
                  className="settings-button settings-button-secondary about-proxy-save"
                  type="button"
                  disabled={updateBusy || !proxyDirty || Boolean(proxyValidationError)}
                  onClick={() => void saveProxySettings().catch(() => undefined)}
                >
                  {proxySaving ? <LoaderCircle className="settings-spin" size={15} /> : <Save size={15} />}
                  <span>{proxySaving ? t("common.saving") : t("about.saveProxy")}</span>
                </button>
                <button
                  className="settings-button settings-button-secondary"
                  type="button"
                  disabled={updateBusy || Boolean(proxyValidationError) || !isTauri()}
                  onClick={() => void testProxyConnection()}
                >
                  {proxyTesting ? <LoaderCircle className="settings-spin" size={15} /> : <Wifi size={15} />}
                  <span>{proxyTesting ? t("about.proxyTesting") : t("about.testProxy")}</span>
                </button>
              </div>
            </div>
          </div>
          {proxyFeedback ? (
            <div className={`settings-feedback settings-feedback-${proxyFeedback.kind} about-proxy-feedback`} role="status">
              <span>{proxyFeedback.message}</span>
            </div>
          ) : null}
          {updateInfo && updateStatus !== "checking" && updateStatus !== "error" ? (
            <div className={`about-update-result${updateInfo.available ? " about-update-available" : ""}`}>
              <div className="about-update-result-heading">
                <div>
                  <strong>{updateInfo.available ? t("about.updateAvailable") : t("about.upToDate")}</strong>
                  <span>{t("about.versionComparison", { current: updateInfo.currentVersion, latest: updateInfo.latestVersion })}</span>
                </div>
                {updateInfo.publishedAt ? <time dateTime={updateInfo.publishedAt}>{formatReleaseDate(updateInfo.publishedAt)}</time> : null}
              </div>
              {updateInfo.releaseNotes ? <pre className="about-release-notes">{updateInfo.releaseNotes}</pre> : null}
              {updateInfo.available && signedUpdate ? (
                <>
                  <button className="settings-button settings-button-primary" type="button" disabled={updateBusy} onClick={() => void installUpdate()}>
                    {updateBusy ? <LoaderCircle className="settings-spin" size={15} /> : <RefreshCw size={15} />}
                    <span>{updateStatus === "downloading" ? t("about.downloadingUpdate") : updateStatus === "installing" ? t("about.installingUpdate") : t("about.installUpdate")}</span>
                  </button>
                  {updateStatus === "downloading" || updateStatus === "installing" ? (
                    <div className="about-update-progress" aria-label={t("about.downloadProgress", { progress: downloadProgress })}>
                      <span style={{ width: `${downloadProgress}%` }} />
                      <small>{downloadTotal ? `${downloadProgress}% · ${formatFileSize(downloadedBytes)} / ${formatFileSize(downloadTotal)}` : formatFileSize(downloadedBytes)}</small>
                    </div>
                  ) : null}
                </>
              ) : updateInfo.available ? (
                <div className="about-update-signature-warning">
                  <span>{updateError || t("about.signedManifestUnavailable")}</span>
                  <button className="settings-button settings-button-secondary" type="button" onClick={() => void openExternal(updateInfo.releaseUrl)}>
                    <ExternalLink size={15} />
                    <span>{t("about.openRelease")}</span>
                  </button>
                </div>
              ) : null}
            </div>
          ) : null}
          {updateStatus === "error" ? (
            <div className="about-update-error">
              <span>{updateError || t("about.updateFailed")}</span>
              <button className="settings-button settings-button-secondary" type="button" onClick={() => void openExternal(RELEASE_URL)}>
                <ExternalLink size={15} />
                <span>{t("about.openReleases")}</span>
              </button>
            </div>
          ) : null}
        </section>

        {/* 「当前能力」与「数据与安全」原先是两个分区、两套内部布局，
            但它们是同一种东西：图标 + 标题 + 一句说明。合成一个分区、
            一套卡片网格，页面从八条横带收敛到三组，也不再需要读者
            在两种排版之间切换视觉模式。 */}
        <section className="settings-section" aria-labelledby="about-capability-heading">
          <div className="settings-section-head">
            <BookOpen size={16} />
            <h3 id="about-capability-heading">能力与边界</h3>
            <p>能做什么，以及数据留在哪里</p>
          </div>
          <div className="about-fact-grid">
            <FactItem icon={<MessagesSquare size={15} />} title="多供应商对话" text="支持四种模型协议、自定义中转站、Display Name 映射和手动连接测试。" />
            <FactItem icon={<Gauge size={15} />} title="流式与长对话" text="流式思考、块级 Markdown、上下文用量提示和接近 90% 时的自动压缩。" />
            <FactItem icon={<Sparkles size={15} />} title="Skill 与记忆" text="支持来源可追溯的 Skill、L1/L2 记忆和受权限控制的工具循环。" />
            <FactItem icon={<FileText size={15} />} title="附件与安全预览" text="支持常见文本、图片、PDF、DOCX、XLSX 附件及受限 HTML 预览。" />
            <FactItem icon={<Database size={15} />} title="本地优先" text="会话、记忆和用量保存在本地文件；不使用远程同步数据库。" tone="guard" />
            <FactItem icon={<ShieldCheck size={15} />} title="凭据隔离" text="API Key 使用系统凭据存储，普通设置读取只返回凭据是否存在。" tone="guard" />
          </div>
          <div className="settings-callout">
            <Sparkles size={17} />
            <div>
              <strong>后续方向</strong>
              <span>PDF 阅读、Zotero 类笔记与批注、Office 受控工具和对话导出正在规划中。当前 Agent 不提供任意 Shell 或桌面自动化权限。</span>
            </div>
          </div>
        </section>

        <section className="settings-section about-footer">
          <div className="about-link-row" aria-label="项目链接">
            <button type="button" className="settings-button settings-button-secondary" onClick={() => void openExternal(PROJECT_URL)}>
              <ExternalLink size={15} />
              <span>项目主页</span>
            </button>
            <button type="button" className="settings-button settings-button-secondary" onClick={() => void openExternal(RELEASE_URL)}>
              <ExternalLink size={15} />
              <span>下载与版本</span>
            </button>
            <button type="button" className="settings-button settings-button-secondary" onClick={() => void openExternal(ISSUES_URL)}>
              <ExternalLink size={15} />
              <span>反馈问题</span>
            </button>
          </div>
          <p className="about-footer-note">
            Mnemora 当前为测试版本。导出的设置备份可能包含 API Key，请勿公开分享。
          </p>
        </section>
      </div>
    </section>
  );
}

function formatReleaseDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString();
}

function formatFileSize(bytes: number) {
  if (bytes < 1024 * 1024) return `${Math.max(0, bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function validateProxyUrl(value: string): ProxyValidationKey | null {
  const trimmed = value.trim();
  if (!trimmed) return "about.proxyError.required";
  if (trimmed.length > 2048) return "about.proxyError.tooLong";
  const normalized = trimmed.includes("://") ? trimmed : `http://${trimmed}`;
  try {
    const url = new URL(normalized);
    if (url.protocol !== "http:" && url.protocol !== "https:") return "about.proxyError.scheme";
    if (!url.hostname) return "about.proxyError.host";
    if (url.username || url.password) return "about.proxyError.credentials";
    if (url.search || url.hash) return "about.proxyError.parameters";
    return null;
  } catch {
    return "about.proxyError.invalid";
  }
}

type ProxyValidationKey =
  | "about.proxyError.required"
  | "about.proxyError.tooLong"
  | "about.proxyError.scheme"
  | "about.proxyError.host"
  | "about.proxyError.credentials"
  | "about.proxyError.parameters"
  | "about.proxyError.invalid";

function MetadataItem({
  label,
  value,
  loading = false,
}: {
  label: string;
  value: string;
  loading?: boolean;
}) {
  return (
    <div className="settings-stat">
      <span>{label}</span>
      <strong title={value}>{loading ? "读取中..." : value}</strong>
    </div>
  );
}

/** tone="guard" 用于「数据边界」类条目，图标换成成功色以区别于能力条目。 */
function FactItem({
  icon,
  title,
  text,
  tone,
}: {
  icon: React.ReactNode;
  title: string;
  text: string;
  tone?: "guard";
}) {
  return (
    <div className="about-fact-item" data-tone={tone}>
      <span className="about-fact-icon">{icon}</span>
      <div>
        <strong>{title}</strong>
        <span>{text}</span>
      </div>
    </div>
  );
}
