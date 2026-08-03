import { BookOpenText, FileText, MessageCircle, RefreshCw, StickyNote } from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import { useOverview } from "../hooks/useOverview";
import { useOverviewViewRuntime } from "../../workspace/runtime/OverviewViewRuntime";
import "../styles/overview.css";

function formatDate(timestamp: number, language: string) {
  return new Intl.DateTimeFormat(language === "en" ? "en-US" : "zh-CN", {
    month: "short",
    day: "numeric",
  }).format(new Date(timestamp));
}

export default function OverviewView() {
  const { t, language } = useI18n();
  const { snapshot, loading, error, refresh } = useOverview();
  const runtime = useOverviewViewRuntime();

  return (
    <div className="overview-view">
      <header className="overview-header">
        <div>
          <p className="overview-eyebrow">Mnemora</p>
          <h1>{t("overview.title")}</h1>
          <p className="overview-subtitle">{t("overview.subtitle")}</p>
        </div>
        <button className="overview-icon-button" type="button" onClick={refresh} title={t("overview.refresh")} aria-label={t("overview.refresh")}>
          <RefreshCw size={16} />
        </button>
      </header>

      <section className="overview-actions" aria-label={t("overview.quickActions")}>
        <button type="button" onClick={runtime.onNewChat}><MessageCircle size={17} /><span>{t("overview.newChat")}</span></button>
        <button type="button" onClick={runtime.onOpenNotes}><StickyNote size={17} /><span>{t("overview.openNotes")}</span></button>
        <button type="button" onClick={runtime.onOpenWork}><BookOpenText size={17} /><span>{t("overview.openLibrary")}</span></button>
      </section>

      {loading ? <div className="overview-state" role="status">{t("overview.loading")}</div> : null}
      {error ? <div className="overview-state overview-state-error" role="alert">{t("overview.loadFailed")}</div> : null}
      {!loading && !error && snapshot ? (
        <>
          <section className="overview-section">
            <div className="overview-section-heading"><h2>{t("overview.recent")}</h2><span>{t("overview.recentDescription")}</span></div>
            {snapshot.recentItems.length === 0 ? <p className="overview-empty">{t("overview.empty")}</p> : (
              <div className="overview-activity-list">
                {snapshot.recentItems.map((item) => (
                  <button key={`${item.kind}:${item.id}`} type="button" className="overview-activity-item" onClick={() => runtime.onOpenItem(item)}>
                    <span className="overview-activity-icon">{item.kind === "conversation" ? <MessageCircle size={16} /> : item.kind === "note" ? <StickyNote size={16} /> : <FileText size={16} />}</span>
                    <span className="overview-activity-copy"><strong>{item.title}</strong><span>{item.description || t("overview.noDescription")}</span></span>
                    <time dateTime={new Date(item.updatedAt).toISOString()}>{formatDate(item.updatedAt, language)}</time>
                  </button>
                ))}
              </div>
            )}
          </section>
          <section className="overview-section">
            <div className="overview-section-heading"><h2>{t("overview.assets")}</h2><span>{t("overview.assetsDescription")}</span></div>
            <div className="overview-asset-grid">
              <div className="overview-asset"><MessageCircle size={18} /><strong>{snapshot.conversationCount}</strong><span>{t("overview.conversations")}</span></div>
              <div className="overview-asset"><StickyNote size={18} /><strong>{snapshot.noteCount}</strong><span>{t("overview.notes")}</span></div>
              <div className="overview-asset"><BookOpenText size={18} /><strong>{snapshot.literatureCount}</strong><span>{t("overview.literature")}</span></div>
            </div>
          </section>
        </>
      ) : null}
    </div>
  );
}
