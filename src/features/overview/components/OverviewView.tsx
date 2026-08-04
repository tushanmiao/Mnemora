import {
  ArrowUpRight,
  BookOpenText,
  FileText,
  MessageCircle,
  RefreshCw,
  StickyNote,
  type LucideIcon,
} from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import { useOverviewViewRuntime } from "../../workspace/runtime/OverviewViewRuntime";
import { useOverview } from "../hooks/useOverview";
import type { OverviewRecentItem } from "../types";
import "../styles/overview.css";

type TimelineGroup = {
  key: "today" | "yesterday" | "earlier";
  items: OverviewRecentItem[];
};

const ITEM_ICONS: Record<OverviewRecentItem["kind"], LucideIcon> = {
  conversation: MessageCircle,
  note: StickyNote,
  literature: FileText,
};

const GROUP_LABEL_KEYS = {
  today: "overview.today",
  yesterday: "overview.yesterday",
  earlier: "overview.earlier",
} as const;

const ITEM_KIND_LABEL_KEYS = {
  conversation: "overview.kind.conversation",
  note: "overview.kind.note",
  literature: "overview.kind.literature",
} as const;

function dateGroup(timestamp: number): TimelineGroup["key"] {
  const date = new Date(timestamp);
  const today = new Date();
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const startOfDate = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  if (startOfDate === startOfToday) return "today";
  if (startOfDate === startOfToday - 86_400_000) return "yesterday";
  return "earlier";
}

function groupRecentItems(items: OverviewRecentItem[]): TimelineGroup[] {
  const groups = new Map<TimelineGroup["key"], OverviewRecentItem[]>();
  items.forEach((item) => {
    const key = dateGroup(item.updatedAt);
    groups.set(key, [...(groups.get(key) ?? []), item]);
  });
  return (["today", "yesterday", "earlier"] as const)
    .filter((key) => groups.has(key))
    .map((key) => ({ key, items: groups.get(key) ?? [] }));
}

function formatTime(timestamp: number, language: string, group: TimelineGroup["key"]) {
  if (group === "today") {
    return new Intl.DateTimeFormat(language === "en" ? "en-US" : "zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: language === "en",
    }).format(new Date(timestamp));
  }
  return new Intl.DateTimeFormat(language === "en" ? "en-US" : "zh-CN", {
    month: "short",
    day: "numeric",
  }).format(new Date(timestamp));
}

export default function OverviewView() {
  const { t, language } = useI18n();
  const { snapshot, loading, error, refresh } = useOverview();
  const runtime = useOverviewViewRuntime();
  const groups = groupRecentItems(snapshot?.recentItems ?? []);

  return (
    <div className="overview-view">
      <header className="overview-header">
        <div className="overview-heading-copy">
          <h1>{t("overview.title")}</h1>
          <p>{t("overview.subtitle")}</p>
        </div>
        <button
          className="overview-icon-button"
          type="button"
          onClick={refresh}
          title={t("overview.refresh")}
          aria-label={t("overview.refresh")}
        >
          <RefreshCw size={16} />
        </button>
      </header>

      <nav className="overview-actions" aria-label={t("overview.quickActions")}>
        <button className="overview-action-primary" type="button" onClick={runtime.onNewChat}>
          <MessageCircle size={16} />
          <span>{t("overview.newChat")}</span>
        </button>
        <button type="button" onClick={runtime.onOpenNotes}>
          <StickyNote size={16} />
          <span>{t("overview.openNotes")}</span>
        </button>
        <button type="button" onClick={runtime.onOpenWork}>
          <BookOpenText size={16} />
          <span>{t("overview.openLibrary")}</span>
        </button>
      </nav>

      <section className="overview-index" aria-labelledby="overview-index-title">
        <div className="overview-index-heading">
          <div>
            <h2 id="overview-index-title">{t("overview.recent")}</h2>
            <span>{t("overview.recentDescription")}</span>
          </div>
          {snapshot ? (
            <p className="overview-counts" aria-label={t("overview.assetsDescription")}>
              <span>{snapshot.conversationCount} {t("overview.conversations")}</span>
              <span>{snapshot.noteCount} {t("overview.notes")}</span>
              <span>{snapshot.literatureCount} {t("overview.literature")}</span>
            </p>
          ) : null}
        </div>

        {loading ? (
          <div className="overview-index-loading" role="status" aria-label={t("overview.loading")}>
            <span /><span /><span /><span />
          </div>
        ) : null}

        {error ? (
          <div className="overview-state overview-state-error" role="alert">
            <p>{t("overview.loadFailed")}</p>
            <button type="button" onClick={refresh}><RefreshCw size={14} />{t("view.retry")}</button>
          </div>
        ) : null}

        {!loading && !error && snapshot && groups.length === 0 ? (
          <div className="overview-state overview-empty">
            <p>{t("overview.empty")}</p>
            <button type="button" onClick={runtime.onNewChat}><MessageCircle size={14} />{t("overview.newChat")}</button>
          </div>
        ) : null}

        {!loading && !error && groups.length > 0 ? (
          <div className="overview-timeline">
            {groups.map((group) => (
              <section className="overview-timeline-group" key={group.key} aria-label={t(GROUP_LABEL_KEYS[group.key])}>
                <h3>{t(GROUP_LABEL_KEYS[group.key])}</h3>
                <div className="overview-activity-list">
                  {group.items.map((item, index) => {
                    const Icon = ITEM_ICONS[item.kind];
                    return (
                      <button
                        key={`${item.kind}:${item.id}`}
                        type="button"
                        className="overview-activity-item"
                        data-kind={item.kind}
                        onClick={() => runtime.onOpenItem(item)}
                      >
                        <span className="overview-activity-index" aria-hidden="true">
                          {String(index + 1).padStart(2, "0")}
                        </span>
                        <span className="overview-activity-icon" aria-hidden="true"><Icon size={15} /></span>
                        <span className="overview-activity-copy">
                          <strong>{item.title}</strong>
                          <span>{item.description || t("overview.noDescription")}</span>
                        </span>
                        <span className="overview-activity-meta">
                          <span>{t(ITEM_KIND_LABEL_KEYS[item.kind])}</span>
                          <time dateTime={new Date(item.updatedAt).toISOString()}>
                            {formatTime(item.updatedAt, language, group.key)}
                          </time>
                        </span>
                        <ArrowUpRight className="overview-activity-open" size={15} aria-hidden="true" />
                      </button>
                    );
                  })}
                </div>
              </section>
            ))}
          </div>
        ) : null}
      </section>
    </div>
  );
}
