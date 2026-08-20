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

  const categories = useMemo(() => (
    [...(status?.categories ?? [])].sort((left, right) => right.bytes - left.bytes)
  ), [status?.categories]);

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

      <div className="storage-settings-scroll">
        {error ? (
          <div className="storage-notice storage-notice-error" role="alert" aria-live="polite">
            <AlertTriangle size={17} />
            <div><strong>{t("storage.operationFailed")}</strong><span>{error}</span></div>
          </div>
        ) : null}

        {loading && !status ? (
          <div className="storage-loading" role="status"><LoaderCircle className="settings-spin" size={20} />{t("storage.loading")}</div>
        ) : status ? (
          <>
            {!status.available ? (
              <div className="storage-notice storage-notice-error" role="alert" aria-live="polite">
                <AlertTriangle size={18} />
                <div>
                  <strong>{t("storage.unavailable")}</strong>
                  <span>{status.availabilityError ?? t("storage.unavailableDescription")}</span>
                </div>
              </div>
            ) : null}

            {status.lastMigration ? (
              <div
                className={`storage-notice ${status.lastMigration.succeeded ? "storage-notice-success" : "storage-notice-error"}`}
                role={status.lastMigration.succeeded ? "status" : "alert"}
              >
                {status.lastMigration.succeeded ? <ArchiveRestore size={17} /> : <AlertTriangle size={17} />}
                <div>
                  <strong>{status.lastMigration.succeeded ? t("storage.migrationSucceeded") : t("storage.migrationFailed")}</strong>
                  <span>{status.lastMigration.error ?? t("storage.migrationSucceededDescription")}</span>
                </div>
              </div>
            ) : null}

            <section className="storage-location-section">
              <div className="storage-section-heading">
                <div><h3>{t("storage.location")}</h3><span>{t("storage.locationDescription")}</span></div>
                <span className={`storage-location-mode ${status.isCustom ? "storage-location-mode-custom" : ""}`}>
                  {status.isCustom ? t("storage.customLocation") : t("storage.defaultLocation")}
                </span>
              </div>
              <div className="storage-path-row">
                <HardDrive size={18} />
                <code title={status.currentPath}>{status.currentPath}</code>
              </div>
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
              <p className="storage-migration-note">{t("storage.migrationDescription")}</p>
              <p className="storage-migration-note">{t("storage.configurationDescription")}</p>
            </section>

            <section className="storage-usage-section">
              <div className="storage-section-heading">
                <div><h3>{t("storage.usage")}</h3><span>{t("storage.usageDescription")}</span></div>
                <strong className="storage-total">{formatBytes(status.totalBytes)}</strong>
              </div>
              <div className="storage-usage-list">
                {categories.map((category) => {
                  const Icon = CATEGORY_ICONS[category.id];
                  const ratio = status.totalBytes > 0 ? Math.max(2, (category.bytes / status.totalBytes) * 100) : 0;
                  return (
                    <div className="storage-usage-row" key={category.id}>
                      <Icon size={16} />
                      <div className="storage-usage-copy">
                        <div><span>{t(`storage.category.${category.id}`)}</span><strong>{formatBytes(category.bytes)}</strong></div>
                        <div className="storage-usage-track" aria-hidden="true"><span style={{ width: `${ratio}%` }} /></div>
                      </div>
                    </div>
                  );
                })}
              </div>
            </section>

            {status.previousPath ? (
              <section className="storage-previous-section">
                <h3>{t("storage.previousCopy")}</h3>
                <p>{t("storage.previousCopyDescription")}</p>
                <code title={status.previousPath}>{status.previousPath}</code>
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
