import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArchiveRestore,
  Database,
  FolderCog,
  FolderOpen,
  HardDrive,
  LoaderCircle,
  RefreshCw,
} from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import {
  chooseStorageDirectory,
  getStorageStatus,
  migrateStorageData,
  openStorageDirectory,
  type StorageCategoryUsage,
  type StorageStatus,
} from "../api/storage";

const CATEGORY_ICONS: Record<StorageCategoryUsage["id"], typeof Database> = {
  conversations: FolderCog,
  library: Database,
  memory: ArchiveRestore,
  skills: FolderCog,
  usage: HardDrive,
  sync: RefreshCw,
  english: Database,
};

/**
 * 扇区配色取自工作区身份色，因此跟随主题预设与明暗模式，
 * 且与各自对应的工作区在视觉上呼应（对话=chat、文献=work…）。
 * sync 没有对应工作区，借用 info 状态色补足第七种。
 */
const CATEGORY_COLORS: Record<StorageCategoryUsage["id"], string> = {
  english: "var(--workspace-english)",
  conversations: "var(--workspace-chat)",
  library: "var(--workspace-work)",
  memory: "var(--workspace-overview)",
  skills: "var(--workspace-notes)",
  usage: "var(--workspace-settings)",
  sync: "var(--status-info)",
};

type StorageSlice = {
  id: StorageCategoryUsage["id"];
  bytes: number;
  share: number;
  color: string;
};

/**
 * 用 conic-gradient 画环形图：占比本身是「部分与整体」的关系，
 * 一根按总量缩放的条形在 385MB 对 1.4KB 这种量级差下，
 * 小项会全部塌成看不见的一丝。
 * 零字节分类不进渐变（画不出扇区），但仍留在图例里说明「确实是 0」。
 */
function donutGradient(slices: StorageSlice[]) {
  const drawable = slices.filter((slice) => slice.share > 0);
  if (drawable.length === 0) return "var(--color-border-soft)";
  let cursor = 0;
  const stops = drawable.map((slice) => {
    const from = cursor;
    cursor += slice.share * 100;
    return `${slice.color} ${from}% ${cursor}%`;
  });
  return `conic-gradient(from -90deg, ${stops.join(", ")})`;
}

/** 小于 0.1% 的分类标成 <0.1% 而不是 0.0%，避免看起来像没占空间。 */
function formatShare(share: number) {
  const percent = share * 100;
  if (percent <= 0) return "0%";
  if (percent < 0.1) return "<0.1%";
  return `${percent.toFixed(percent < 10 ? 1 : 0)}%`;
}

export function StorageSettingsPanel() {
  const { t } = useI18n();
  const [status, setStatus] = useState<StorageStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setStatus(await getStorageStatus());
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const slices = useMemo<StorageSlice[]>(() => {
    const total = status?.totalBytes ?? 0;
    return [...(status?.categories ?? [])]
      .sort((left, right) => right.bytes - left.bytes)
      .map((category) => ({
        id: category.id,
        bytes: category.bytes,
        share: total > 0 ? category.bytes / total : 0,
        color: CATEGORY_COLORS[category.id],
      }));
  }, [status?.categories, status?.totalBytes]);

  const startMigration = useCallback(async (destination: string) => {
    if (!status || destination === status.currentPath) return;
    const confirmed = window.confirm(t("storage.migrationConfirm", { path: destination }));
    if (!confirmed) return;
    setBusy(true);
    setError(null);
    try {
      await migrateStorageData(destination);
    } catch (migrationError) {
      setBusy(false);
      setError(migrationError instanceof Error ? migrationError.message : String(migrationError));
    }
  }, [status, t]);

  const chooseAndMigrate = useCallback(async () => {
    try {
      const destination = await chooseStorageDirectory(t("storage.chooseDirectoryTitle"));
      if (destination) await startMigration(destination);
    } catch (chooseError) {
      setError(chooseError instanceof Error ? chooseError.message : String(chooseError));
    }
  }, [startMigration, t]);

  return (
    <section className="settings-content storage-settings-content" aria-busy={loading || busy}>
      <div className="settings-content-heading">
        <div>
          <h2>{t("storage.title")}</h2>
          <span>{t("storage.subtitle")}</span>
        </div>
        <button className="settings-button settings-button-secondary" type="button" disabled={loading || busy} onClick={() => void load()}>
          <RefreshCw size={15} className={loading ? "settings-spin" : undefined} />
          <span>{t("storage.refresh")}</span>
        </button>
      </div>

      <div className="settings-scroll settings-scroll-measure">
        {error ? (
          <div className="settings-callout settings-callout-danger" role="alert" aria-live="polite">
            <AlertTriangle size={17} />
            <div><strong>{t("storage.operationFailed")}</strong><span>{error}</span></div>
          </div>
        ) : null}

        {loading && !status ? (
          <div className="settings-loading" role="status"><LoaderCircle className="settings-spin" size={20} />{t("storage.loading")}</div>
        ) : status ? (
          <>
            {!status.available ? (
              <div className="settings-callout settings-callout-danger" role="alert" aria-live="polite">
                <AlertTriangle size={18} />
                <div>
                  <strong>{t("storage.unavailable")}</strong>
                  <span>{status.availabilityError ?? t("storage.unavailableDescription")}</span>
                </div>
              </div>
            ) : null}

            {status.lastMigration ? (
              <div
                className={`settings-callout ${status.lastMigration.succeeded ? "settings-callout-success" : "settings-callout-danger"}`}
                role={status.lastMigration.succeeded ? "status" : "alert"}
              >
                {status.lastMigration.succeeded ? <ArchiveRestore size={17} /> : <AlertTriangle size={17} />}
                <div>
                  <strong>{status.lastMigration.succeeded ? t("storage.migrationSucceeded") : t("storage.migrationFailed")}</strong>
                  <span>{status.lastMigration.error ?? t("storage.migrationSucceededDescription")}</span>
                </div>
              </div>
            ) : null}

            <section className="settings-section">
              <div className="settings-section-head">
                <HardDrive size={16} />
                <h3>{t("storage.location")}</h3>
                <p>{t("storage.locationDescription")}</p>
                <div className="settings-section-head-actions">
                  <span className={`settings-pill${status.isCustom ? " settings-pill-accent" : ""}`}>
                    {status.isCustom ? t("storage.customLocation") : t("storage.defaultLocation")}
                  </span>
                </div>
              </div>
              <code className="settings-code" title={status.currentPath}>{status.currentPath}</code>
              <div className="storage-actions">
                <button className="settings-button settings-button-secondary" type="button" disabled={!status.available || busy} onClick={() => void openStorageDirectory().catch((openError) => setError(openError instanceof Error ? openError.message : String(openError)))}>
                  <FolderOpen size={15} /><span>{t("storage.openDirectory")}</span>
                </button>
                <button className="settings-button settings-button-primary" type="button" disabled={!status.available || busy} onClick={() => void chooseAndMigrate()}>
                  {busy ? <LoaderCircle className="settings-spin" size={15} /> : <FolderCog size={15} />}
                  <span>{busy ? t("storage.restarting") : t("storage.changeLocation")}</span>
                </button>
                {status.isCustom ? (
                  <button className="settings-button settings-button-secondary" type="button" disabled={!status.available || busy} onClick={() => void startMigration(status.defaultPath)}>
                    <ArchiveRestore size={15} /><span>{t("storage.restoreDefault")}</span>
                  </button>
                ) : null}
              </div>
              <p className="settings-section-note">{t("storage.migrationDescription")}</p>
              <p className="settings-section-note">{t("storage.configurationDescription")}</p>
            </section>

            <section className="settings-section">
              <div className="settings-section-head">
                <Database size={16} />
                <h3>{t("storage.usage")}</h3>
                <p>{t("storage.usageDescription")}</p>
                <div className="settings-section-head-actions">
                  <strong className="storage-total">{formatBytes(status.totalBytes)}</strong>
                </div>
              </div>
              <div className="storage-usage-chart">
                <div
                  className="storage-donut"
                  style={{ background: donutGradient(slices) }}
                  role="img"
                  aria-label={t("storage.usageDescription")}
                >
                  <div className="storage-donut-hole">
                    <strong>{formatBytes(status.totalBytes)}</strong>
                    <span>{t("storage.usage")}</span>
                  </div>
                </div>
                <ul className="storage-usage-legend">
                  {slices.map((slice) => {
                    const Icon = CATEGORY_ICONS[slice.id];
                    return (
                      <li key={slice.id}>
                        {/* 图标本身就是色标：再单独放一个色块是同一信息编码两次，白占宽度 */}
                        <Icon size={15} style={{ color: slice.color }} />
                        <span>{t(`storage.category.${slice.id}`)}</span>
                        <b>{formatBytes(slice.bytes)}</b>
                        <em>{formatShare(slice.share)}</em>
                      </li>
                    );
                  })}
                  {slices.length === 0 ? <li className="storage-usage-empty">{t("storage.usage")} 0 B</li> : null}
                </ul>
              </div>
            </section>

            {status.previousPath ? (
              <section className="settings-section">
                <div className="settings-section-head">
                  <ArchiveRestore size={16} />
                  <h3>{t("storage.previousCopy")}</h3>
                </div>
                <p className="settings-section-note">{t("storage.previousCopyDescription")}</p>
                <code className="settings-code" title={status.previousPath}>{status.previousPath}</code>
              </section>
            ) : null}
          </>
        ) : null}
      </div>
    </section>
  );
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const amount = value / (1024 ** index);
  return `${amount >= 100 || index === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[index]}`;
}
