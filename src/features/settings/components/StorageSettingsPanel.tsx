import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArchiveRestore,
  Database,
  FolderCog,
  FolderOpen,
  HardDrive,
  LoaderCircle,
  MessageSquareText,
  RefreshCw,
} from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import type { TranslationKey } from "../../../i18n/translations";
import {
  chooseStorageDirectory,
  getStorageStatus,
  migrateStorageData,
  openStorageDirectory,
  type KnownStorageCategoryId,
  type StorageCategoryUsage,
  type StorageStatus,
} from "../api/storage";

const CATEGORY_ICONS: Record<KnownStorageCategoryId, typeof Database> = {
  conversations: FolderCog,
  library: Database,
  memory: ArchiveRestore,
  prompts: MessageSquareText,
  skills: FolderCog,
  usage: HardDrive,
  sync: RefreshCw,
  english: Database,
};

/**
 * 环形扇区可用的配色槽位数，与 `--chart-series-*` 一一对应。
 *
 * 8 是硬上限：这组颜色只在「相邻配对」下通过色盲与正常视觉的可分辨门槛，
 * 而扇区按体积排序，所以下面按排名取色 —— 排名相邻即槽位相邻，校验前提才成立。
 * 换色值必须重跑校验器，别靠眼睛判断；取值与结论见 tokens.css 的 --chart-series-*。
 */
const CHART_SERIES_SLOTS = 8;

/** 超出槽位的分类退回中性色：它们必然是尾部的极小片，不值得为此破坏配色可分辨性。 */
const CHART_SERIES_OVERFLOW = "var(--color-faint)";

function chartSeriesColor(rank: number) {
  return rank < CHART_SERIES_SLOTS ? `var(--chart-series-${rank + 1})` : CHART_SERIES_OVERFLOW;
}

const CATEGORY_TRANSLATION_KEYS: Record<KnownStorageCategoryId, TranslationKey> = {
  conversations: "storage.category.conversations",
  library: "storage.category.library",
  memory: "storage.category.memory",
  prompts: "storage.category.prompts",
  skills: "storage.category.skills",
  usage: "storage.category.usage",
  sync: "storage.category.sync",
  english: "storage.category.english",
};

/**
 * 后端增加新目录时也不能把整个设置页渲染成 React #130。
 *
 * 图标承担「这是什么」，颜色只承担「对应环上哪一段」，所以颜色不在这里定 ——
 * 它由 `chartSeriesColor` 按体积排名给出。
 */
export function getStorageCategoryPresentation(id: string) {
  const knownId = id as KnownStorageCategoryId;
  return {
    Icon: CATEGORY_ICONS[knownId] ?? Database,
    translationKey: (CATEGORY_TRANSLATION_KEYS as Partial<Record<string, TranslationKey>>)[id],
  };
}

type StorageSlice = {
  id: StorageCategoryUsage["id"];
  bytes: number;
  share: number;
  color: string;
};

type StorageDonutSegment = StorageSlice & {
  offset: number;
};

/** 每个 SVG 圆弧使用 0–1 的 pathLength，offset 就是前面分类累计的占比。 */
export function buildStorageDonutSegments(slices: StorageSlice[]): StorageDonutSegment[] {
  let cursor = 0;
  return slices.filter((slice) => slice.share > 0).map((slice) => {
    const segment = { ...slice, offset: cursor };
    cursor += slice.share;
    return segment;
  });
}

/**
 * 环形几何：圆心、半径与描边宽度都在这里定，SVG 与标注共用同一套数字。
 *
 * viewBox 比环大一圈，留给引线和文字：左右各留到 `cx ± (labelRadius + 52)`
 * ——52 是四个中文字在 11px 下的宽度，够最长的分类名「英语学习」不被裁掉。
 */
const DONUT = { cx: 144, cy: 106, radius: 62, band: 28, labelRadius: 86 } as const;

/** cx/cy 取 width/2、height/2，中心挖空的定位才能在 CSS 里写成对称百分比。 */
const DONUT_VIEWBOX = { width: 288, height: 212 } as const;

/**
 * 相邻扇区之间留 2px 底色缝（规范的 mark spec）。
 *
 * pathLength 归一到 1，所以 2px 换算成占比要除以环的周长。
 */
const DONUT_GAP_SHARE = 2 / (2 * Math.PI * DONUT.radius);

/**
 * 扇区描边的 dash 参数。
 *
 * 缝是从扇区里减出来的，所以极小片先判一次：减完只剩不到半条缝就不减了，
 * 否则 1% 的分类会被缝吃光，环上直接消失。
 */
export function donutSegmentDash(share: number) {
  const visible = share - DONUT_GAP_SHARE > DONUT_GAP_SHARE / 2 ? share - DONUT_GAP_SHARE : share;
  return `${visible} ${Math.max(0, 1 - visible)}`;
}

/**
 * 低于这个占比就不在环上直接标注。
 *
 * 规范要求标注「有选择性」：每段都标反而没人读，且极小片的引线会互相压叠。
 * 落选的分类由图例和悬停承载，信息不丢。
 */
const DONUT_LABEL_MIN_SHARE = 0.06;

export type StorageDonutLabel = {
  id: string;
  share: number;
  /** 引线起点（贴着环外沿）与文字锚点，均为 viewBox 坐标。 */
  line: { x1: number; y1: number; x2: number; y2: number };
  x: number;
  y: number;
  anchor: "start" | "end";
};

/**
 * 算出值得直接标注的扇区，以及每条标注的引线和文字位置。
 *
 * 角度从 12 点起顺时针，与描边的 `strokeDashoffset` 起点一致；文字按左右半圆
 * 换锚点，避免伸出 viewBox。
 */
export function buildStorageDonutLabels(
  segments: StorageDonutSegment[],
  minShare = DONUT_LABEL_MIN_SHARE,
): StorageDonutLabel[] {
  return segments
    .filter((segment) => segment.share >= minShare)
    .map((segment) => {
      const angle = (segment.offset + segment.share / 2) * Math.PI * 2;
      const sin = Math.sin(angle);
      const cos = Math.cos(angle);
      const outer = DONUT.radius + DONUT.band / 2;
      const anchor: "start" | "end" = sin >= 0 ? "start" : "end";
      return {
        id: segment.id,
        share: segment.share,
        line: {
          x1: DONUT.cx + outer * sin,
          y1: DONUT.cy - outer * cos,
          x2: DONUT.cx + DONUT.labelRadius * sin,
          y2: DONUT.cy - DONUT.labelRadius * cos,
        },
        x: DONUT.cx + DONUT.labelRadius * sin + (sin >= 0 ? 5 : -5),
        y: DONUT.cy - DONUT.labelRadius * cos,
        anchor,
      };
    });
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
  const [activeCategoryId, setActiveCategoryId] = useState<string | null>(null);

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
      // 取色在排序之后：颜色跟随体积排名，与扇区绘制顺序严格一致。
      .map((category, rank) => ({
        id: category.id,
        bytes: category.bytes,
        share: total > 0 ? category.bytes / total : 0,
        color: chartSeriesColor(rank),
      }));
  }, [status?.categories, status?.totalBytes]);
  const donutSegments = useMemo(() => buildStorageDonutSegments(slices), [slices]);
  const donutLabels = useMemo(() => buildStorageDonutLabels(donutSegments), [donutSegments]);
  const activeSlice = activeCategoryId === null
    ? null
    : slices.find((slice) => slice.id === activeCategoryId) ?? null;

  const categoryName = useCallback((id: string) => {
    const { translationKey } = getStorageCategoryPresentation(id);
    return translationKey ? t(translationKey) : id;
  }, [t]);

  const categoryLabel = useCallback((slice: StorageSlice) => t("storage.categoryBreakdown", {
    name: categoryName(slice.id),
    size: formatBytes(slice.bytes),
    share: formatShare(slice.share),
  }), [categoryName, t]);

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
                <div className="storage-donut" onPointerLeave={() => setActiveCategoryId(null)}>
                  <svg
                    className="storage-donut-svg"
                    viewBox={`0 0 ${DONUT_VIEWBOX.width} ${DONUT_VIEWBOX.height}`}
                    role="group"
                    aria-label={t("storage.usageDescription")}
                  >
                    {/* 只转描边组：文字若跟着转 90° 就没法读了 */}
                    <g transform={`rotate(-90 ${DONUT.cx} ${DONUT.cy})`}>
                      <circle
                        className="storage-donut-track"
                        cx={DONUT.cx}
                        cy={DONUT.cy}
                        r={DONUT.radius}
                        pathLength="1"
                      />
                      {donutSegments.map((segment) => {
                        const isActive = activeCategoryId === segment.id;
                        return (
                          <circle
                            key={segment.id}
                            className="storage-donut-segment"
                            data-active={isActive ? "true" : undefined}
                            data-muted={activeCategoryId !== null && !isActive ? "true" : undefined}
                            cx={DONUT.cx}
                            cy={DONUT.cy}
                            r={DONUT.radius}
                            pathLength="1"
                            strokeDasharray={donutSegmentDash(segment.share)}
                            strokeDashoffset={-segment.offset}
                            style={{ stroke: segment.color }}
                            tabIndex={0}
                            role="img"
                            aria-label={categoryLabel(segment)}
                            onPointerEnter={() => setActiveCategoryId(segment.id)}
                            onFocus={() => setActiveCategoryId(segment.id)}
                            onBlur={() => setActiveCategoryId(null)}
                          >
                            <title>{categoryLabel(segment)}</title>
                          </circle>
                        );
                      })}
                    </g>
                    {donutLabels.map((label) => (
                      <g
                        key={label.id}
                        className="storage-donut-label"
                        data-active={activeCategoryId === label.id ? "true" : undefined}
                        data-muted={activeCategoryId !== null && activeCategoryId !== label.id ? "true" : undefined}
                      >
                        <line x1={label.line.x1} y1={label.line.y1} x2={label.line.x2} y2={label.line.y2} />
                        <text x={label.x} y={label.y} textAnchor={label.anchor}>
                          <tspan className="storage-donut-label-name">{categoryName(label.id)}</tspan>
                          <tspan className="storage-donut-label-share" x={label.x} dy="13">
                            {formatShare(label.share)}
                          </tspan>
                        </text>
                      </g>
                    ))}
                  </svg>
                  <div className="storage-donut-hole" data-active={activeSlice ? "true" : undefined} aria-live="polite" aria-atomic="true">
                    <strong title={activeSlice ? categoryName(activeSlice.id) : undefined}>
                      {activeSlice ? categoryName(activeSlice.id) : formatBytes(status.totalBytes)}
                    </strong>
                    <span>
                      {activeSlice
                        ? `${formatBytes(activeSlice.bytes)} · ${formatShare(activeSlice.share)}`
                        : t("storage.usage")}
                    </span>
                  </div>
                </div>
                <ul className="storage-usage-legend">
                  {slices.map((slice) => {
                    const { Icon, translationKey } = getStorageCategoryPresentation(slice.id);
                    return (
                      <li
                        key={slice.id}
                        data-active={activeCategoryId === slice.id ? "true" : undefined}
                        onPointerEnter={() => setActiveCategoryId(slice.id)}
                        onPointerLeave={() => setActiveCategoryId(null)}
                      >
                        {/* 图标本身就是色标：再单独放一个色块是同一信息编码两次，白占宽度 */}
                        <Icon size={15} style={{ color: slice.color }} />
                        <span>{translationKey ? t(translationKey) : slice.id}</span>
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
