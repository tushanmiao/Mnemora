import { useCallback, useEffect, useMemo, useState, type FormEvent, type ReactNode } from "react";
import {
  AlertTriangle,
  Ban,
  BookOpenCheck,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock3,
  Cloud,
  Database,
  FileSearch,
  FileText,
  Image,
  Info,
  LoaderCircle,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  StickyNote,
  X,
} from "lucide-react";
import { useI18n } from "../../../i18n/I18nProvider";
import type { AppSettings } from "../../../types/appSettings";
import {
  cancelKnowledgeJob,
  enqueueKnowledgeLiterature,
  getGlobalLiteratureConsentStatus,
  getLiteratureConsentStatus,
  getKnowledgeOverview,
  grantLiteratureConsent,
  grantGlobalLiteratureConsent,
  isKnowledgeRuntime,
  listKnowledgeDocuments,
  listKnowledgeJobs,
  readKnowledgeChunk,
  revokeGlobalLiteratureConsent,
  revokeLiteratureConsent,
  rebuildKnowledgeAll,
  rebuildKnowledgeEmbeddings,
  rebuildKnowledgeNote,
  searchKnowledge,
} from "../api/knowledge";
import type {
  KnowledgeChunkView,
  KnowledgeConsentStatus,
  KnowledgeDocumentStatus,
  KnowledgeGlobalConsentStatus,
  KnowledgeJobView,
  KnowledgeOverview,
  KnowledgeSearchHit,
  KnowledgeSearchResponse,
} from "../types";
import type { TranslationKey } from "../../../i18n/translations";
import "../styles/knowledge-center.css";

type Props = {
  knowledgeSettings: AppSettings["knowledge"];
  onOpenWork: () => void;
  onOpenNotes: () => void;
  onOpenSettings: () => void;
};

type SourceFilter = "all" | "literature" | "note";

const ACTIVE_JOB_STATES = new Set(["queued", "running", "cancelling", "paused"]);
const RUNNING_EXTRACTION_JOB_STATES = new Set(["running", "cancelling"]);

const DOCUMENT_STATE_KEYS: Record<string, TranslationKey> = {
  ready: "knowledge.stateReady",
  lexical_ready: "knowledge.stateReady",
  pending: "knowledge.statePending",
  awaiting_consent: "knowledge.stateAwaitingConsent",
  remote_pending: "knowledge.statePending",
  remote_running: "knowledge.stateRunning",
  normalizing: "knowledge.stateBuilding",
  failed: "knowledge.stateFailed",
  degraded: "knowledge.statePartial",
  partial: "knowledge.statePartial",
  stale: "knowledge.stateStale",
  deleted: "knowledge.stateDeleted",
};

const CONSENT_STATE_KEYS: Record<string, TranslationKey> = {
  granted: "knowledge.consentGranted",
  awaiting: "knowledge.consentAwaiting",
  revoked: "knowledge.consentRevoked",
  stale: "knowledge.consentStale",
  not_required: "knowledge.consentNotRequired",
};

const JOB_STATE_KEYS: Record<string, TranslationKey> = {
  queued: "knowledge.stateQueued",
  running: "knowledge.stateRunning",
  cancelling: "knowledge.stateCancelling",
  paused: "knowledge.statePaused",
  succeeded: "knowledge.stateSucceeded",
  partial: "knowledge.statePartial",
  failed: "knowledge.stateFailed",
  cancelled: "knowledge.stateCancelled",
  stale: "knowledge.stateStale",
};

const JOB_STAGE_KEYS: Record<string, TranslationKey> = {
  queued: "knowledge.stageQueued",
  validating: "knowledge.stageValidating",
  awaiting_consent: "knowledge.stageAwaitingConsent",
  planning_batches: "knowledge.stagePlanningBatches",
  requesting_upload_url: "knowledge.stageRequestingUpload",
  uploading: "knowledge.stageUploading",
  remote_pending: "knowledge.stageRemotePending",
  remote_running: "knowledge.stageRemoteRunning",
  downloading: "knowledge.stageDownloading",
  validating_archive: "knowledge.stageValidatingArchive",
  normalizing_elements: "knowledge.stageNormalizing",
  analyzing_asset: "knowledge.stageAnalyzingAsset",
  cloud_failed_local_fallback: "knowledge.stageCloudFallback",
  local_text_fallback: "knowledge.stageLocalFallback",
  chunking: "knowledge.stageChunking",
  indexing: "knowledge.stageIndexing",
  writing_revision: "knowledge.stageWritingRevision",
  building_fts: "knowledge.stageBuildingFts",
  waiting_embedding: "knowledge.stageWaitingEmbedding",
  embedding: "knowledge.stageEmbedding",
  committing: "knowledge.stageCommitting",
  cleaning: "knowledge.stageCleaning",
  done: "knowledge.stageDone",
};

export function KnowledgeCenter({
  knowledgeSettings,
  onOpenWork,
  onOpenNotes,
  onOpenSettings,
}: Props) {
  const { t, language } = useI18n();
  const runtimeAvailable = isKnowledgeRuntime();
  const [overview, setOverview] = useState<KnowledgeOverview | null>(null);
  const [documents, setDocuments] = useState<KnowledgeDocumentStatus[]>([]);
  const [jobs, setJobs] = useState<KnowledgeJobView[]>([]);
  const [globalConsent, setGlobalConsent] = useState<KnowledgeGlobalConsentStatus | null>(null);
  const [consentDetails, setConsentDetails] = useState<Record<string, KnowledgeConsentStatus | null>>({});
  const [consentLoading, setConsentLoading] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>("all");
  const [query, setQuery] = useState("");
  const [searchResponse, setSearchResponse] = useState<KnowledgeSearchResponse | null>(null);
  const [searching, setSearching] = useState(false);
  const [expandedChunkId, setExpandedChunkId] = useState<string | null>(null);
  const [chunk, setChunk] = useState<KnowledgeChunkView | null>(null);
  const [chunkLoading, setChunkLoading] = useState(false);
  const [chunkError, setChunkError] = useState<string | null>(null);

  const loadData = useCallback(async (showLoading: boolean) => {
    if (!runtimeAvailable) {
      setOverview(emptyOverview());
      setDocuments([]);
      setJobs([]);
      setGlobalConsent({ state: "none", granted: false, grantedAt: null, revokedAt: null });
      setConsentDetails({});
      setLoading(false);
      return;
    }
    if (showLoading) {
      setLoading(true);
      setRefreshing(true);
    }
    setError(null);
    try {
      const [overviewResult, documentsResult, jobsResult, globalConsentResult] = await Promise.allSettled([
        getKnowledgeOverview(),
        listKnowledgeDocuments(),
        listKnowledgeJobs(),
        getGlobalLiteratureConsentStatus(),
      ]);
      const failures: string[] = [];
      if (overviewResult.status === "fulfilled") {
        setOverview(overviewResult.value);
      } else {
        failures.push(`${t("knowledge.indexStatus")}: ${toErrorMessage(overviewResult.reason)}`);
      }
      if (documentsResult.status === "fulfilled") {
        const nextDocuments = documentsResult.value;
        setDocuments(nextDocuments);
        setConsentDetails((current) => {
          const next: Record<string, KnowledgeConsentStatus | null> = {};
          for (const document of nextDocuments) {
            const detail = current[document.sourceId];
            if (detail && detail.sourceHash === document.sourceHash) {
              next[document.sourceId] = detail;
            }
          }
          return next;
        });
      } else {
        failures.push(`${t("knowledge.documentList")}: ${toErrorMessage(documentsResult.reason)}`);
      }
      if (jobsResult.status === "fulfilled") {
        setJobs(jobsResult.value);
      } else {
        failures.push(`${t("knowledge.jobList")}: ${toErrorMessage(jobsResult.reason)}`);
      }
      if (globalConsentResult.status === "fulfilled") {
        setGlobalConsent(globalConsentResult.value);
      } else {
        // Do not leave the panel saying "checking" forever after a partial
        // database/read failure.  Preserve a known value, or show an explicit
        // empty state while the alert carries the actual error.
        setGlobalConsent((current) => current ?? emptyGlobalConsent());
        failures.push(`${t("knowledge.consentGlobalStatus")}: ${toErrorMessage(globalConsentResult.reason)}`);
      }
      setError(failures.length > 0 ? failures.join(" · ") : null);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [runtimeAvailable, t]);

  useEffect(() => {
    void loadData(true);
  }, [loadData]);

  const activeJobs = useMemo(
    () => jobs.some((job) => ACTIVE_JOB_STATES.has(job.state)),
    [jobs],
  );

  const runningExtractionJobDocumentIds = useMemo(
    () => new Set(
      jobs
        .filter((job) => job.jobKind === "extract" && job.documentId && RUNNING_EXTRACTION_JOB_STATES.has(job.state))
        .map((job) => job.documentId as string),
    ),
    [jobs],
  );

  useEffect(() => {
    if (!runtimeAvailable || !activeJobs) return undefined;
    const timer = window.setInterval(() => {
      void loadData(false);
    }, 2_000);
    return () => window.clearInterval(timer);
  }, [activeJobs, loadData, runtimeAvailable]);

  const documentById = useMemo(
    () => new Map(documents.map((document) => [document.id, document])),
    [documents],
  );

  const visibleDocuments = useMemo(
    () => documents.filter((document) => sourceFilter === "all" || document.sourceClass === sourceFilter),
    [documents, sourceFilter],
  );

  const pdfDocuments = useMemo(
    () => documents.filter((document) => document.sourceClass === "literature" && document.sourceKind === "pdf" && document.state !== "deleted"),
    [documents],
  );

  const visibleHits = useMemo(() => {
    const hits = searchResponse?.hits ?? [];
    if (sourceFilter === "all") return hits;
    return hits.filter((hit) => hit.sourceClass === sourceFilter);
  }, [searchResponse, sourceFilter]);

  const runAction = useCallback(async (key: string, action: () => Promise<unknown>) => {
    setActionBusy(key);
    setActionError(null);
    try {
      await action();
      await loadData(false);
    } catch (reason) {
      setActionError(toErrorMessage(reason));
    } finally {
      setActionBusy(null);
    }
  }, [loadData]);

  const loadConsentDetail = useCallback(async (sourceId: string) => {
    if (!runtimeAvailable) return;
    setConsentLoading(sourceId);
    setActionError(null);
    try {
      const status = await getLiteratureConsentStatus(sourceId);
      setConsentDetails((current) => ({ ...current, [sourceId]: status }));
    } catch (reason) {
      setActionError(toErrorMessage(reason));
    } finally {
      setConsentLoading((current) => current === sourceId ? null : current);
    }
  }, [runtimeAvailable]);

  const readConsentForAction = useCallback(async (sourceId: string) => {
    if (Object.prototype.hasOwnProperty.call(consentDetails, sourceId)) {
      return consentDetails[sourceId];
    }
    try {
      const status = await getLiteratureConsentStatus(sourceId);
      setConsentDetails((current) => ({ ...current, [sourceId]: status }));
      return status;
    } catch {
      // The action itself will provide the authoritative error.  A status
      // lookup is only an enhancement used to explain stale consent.
      return undefined;
    }
  }, [consentDetails]);

  const confirmPdfUpload = useCallback(async (document: KnowledgeDocumentStatus) => {
    const detail = await readConsentForAction(document.sourceId);
    const hashChanged = detail?.documentConsentState === "stale"
      || (detail?.documentConsentState !== "none" && detail?.documentSourceHashMatches === false);
    const message = [
      hashChanged ? t("knowledge.consentHashChanged") : null,
      t("knowledge.consentUploadConfirm", { title: document.title || t("knowledge.untitled") }),
    ].filter(Boolean).join("\n\n");
    return window.confirm(message);
  }, [readConsentForAction, t]);

  const handleGrantDocument = useCallback(async (document: KnowledgeDocumentStatus) => {
    if (!await confirmPdfUpload(document)) return;
    await runAction(`document:${document.id}`, () => grantLiteratureConsent(document.sourceId, "document"));
  }, [confirmPdfUpload, runAction]);

  const handleReparseDocument = useCallback(async (document: KnowledgeDocumentStatus) => {
    if (!await confirmPdfUpload(document)) return;
    await runAction(`document:${document.id}`, () => enqueueKnowledgeLiterature(document.sourceId));
  }, [confirmPdfUpload, runAction]);

  const handleRevokeDocument = useCallback(async (document: KnowledgeDocumentStatus) => {
    if (!window.confirm(t("knowledge.consentRevokeConfirm", { title: document.title || t("knowledge.untitled") }))) return;
    await runAction(`document:${document.id}`, () => revokeLiteratureConsent(document.sourceId));
  }, [runAction, t]);

  const handleGrantGlobal = useCallback(async () => {
    const confirmation = pdfDocuments.length > 0
      ? t("knowledge.consentGlobalUploadConfirm", { count: pdfDocuments.length })
      : t("knowledge.consentGlobalUploadConfirmUnknown");
    if (!window.confirm(confirmation)) return;
    // The command discovers active library PDFs in the repository itself.  A
    // renderer-side document list can briefly lag after import, so global
    // consent must not depend on a first row already being registered.
    await runAction("consent:global", grantGlobalLiteratureConsent);
  }, [pdfDocuments.length, runAction, t]);

  const handleRevokeGlobal = useCallback(async () => {
    if (!window.confirm(t("knowledge.consentGlobalRevokeConfirm"))) return;
    await runAction("consent:global", revokeGlobalLiteratureConsent);
  }, [runAction, t]);

  const handleSearch = useCallback(async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const normalized = query.trim();
    if (!normalized) {
      setSearchResponse(null);
      return;
    }
    setSearching(true);
    setActionError(null);
    setExpandedChunkId(null);
    setChunk(null);
    setChunkError(null);
    try {
      const response = await searchKnowledge({
        query: normalized,
        scope: "library",
        limit: knowledgeSettings.topK,
      });
      setSearchResponse(response);
    } catch (reason) {
      setActionError(toErrorMessage(reason));
      setSearchResponse(null);
    } finally {
      setSearching(false);
    }
  }, [knowledgeSettings.topK, query]);

  const handleReadChunk = useCallback(async (hit: KnowledgeSearchHit) => {
    if (expandedChunkId === hit.chunkId) {
      setExpandedChunkId(null);
      return;
    }
    setExpandedChunkId(hit.chunkId);
    setChunk(null);
    setChunkError(null);
    if (!runtimeAvailable) return;
    setChunkLoading(true);
    try {
      setChunk(await readKnowledgeChunk(hit.chunkId));
    } catch (reason) {
      setChunkError(toErrorMessage(reason));
    } finally {
      setChunkLoading(false);
    }
  }, [expandedChunkId, runtimeAvailable]);

  const statOverview = overview ?? emptyOverview();
  const vectorConfigured = knowledgeSettings.enabled
    && knowledgeSettings.embeddingEnabled
    && knowledgeSettings.allowRemoteEmbedding
    && Boolean(knowledgeSettings.embeddingProvider.trim() && knowledgeSettings.embeddingModel.trim());
  const lexicalLabel = !knowledgeSettings.enabled
    ? t("knowledge.disabled")
    : statOverview.fts5Available
      ? `${t("knowledge.ready")} · ${statOverview.tokenizer}`
      : t("knowledge.degraded");
  const vectorLabel = !vectorConfigured
    ? t("knowledge.disabled")
    : statOverview.embeddingReadyCount > 0
      ? t("knowledge.vectorReady", {
          count: statOverview.embeddingReadyCount,
          dimensions: statOverview.embeddingDimensions.join(", ") || "—",
        })
      : statOverview.embeddingPendingCount > 0
        ? t("knowledge.vectorPending", { count: statOverview.embeddingPendingCount })
        : statOverview.embeddingFailedCount > 0
          ? t("knowledge.vectorFailed", { count: statOverview.embeddingFailedCount })
          : t("knowledge.pending");

  return (
    <div className="knowledge-center">
      <header className="knowledge-center-header">
        <div className="knowledge-center-heading">
          <div className="knowledge-center-mark" aria-hidden="true"><BookOpenCheck size={22} /></div>
          <div>
            <h1>{t("knowledge.title")}</h1>
            <p>{t("knowledge.subtitle")}</p>
          </div>
        </div>
        <div className="knowledge-center-actions">
          <button
            className="knowledge-button knowledge-button-primary"
            type="button"
            disabled={!runtimeAvailable || actionBusy !== null}
            onClick={() => void runAction("rebuild-all", rebuildKnowledgeAll)}
          >
            {actionBusy === "rebuild-all" ? <LoaderCircle size={15} className="knowledge-spin" /> : <RefreshCw size={15} />}
            <span>{actionBusy === "rebuild-all" ? t("knowledge.rebuilding") : t("knowledge.rebuildAll")}</span>
          </button>
          <button
            className="knowledge-button knowledge-button-secondary"
            type="button"
            disabled={!runtimeAvailable || !vectorConfigured || actionBusy !== null}
            onClick={() => {
              if (window.confirm(t("knowledge.rebuildVectorsConfirm"))) {
                void runAction("rebuild-vectors", () => rebuildKnowledgeEmbeddings(undefined, true));
              }
            }}
          >
            {actionBusy === "rebuild-vectors" ? <LoaderCircle size={15} className="knowledge-spin" /> : <Database size={15} />}
            <span>{actionBusy === "rebuild-vectors" ? t("knowledge.rebuildingVectors") : t("knowledge.rebuildVectors")}</span>
          </button>
          <button
            className="knowledge-button knowledge-button-secondary"
            type="button"
            onClick={() => void loadData(true)}
            disabled={loading || refreshing}
          >
            <RefreshCw size={15} className={loading || refreshing ? "knowledge-spin" : undefined} />
            <span>{t("knowledge.refresh")}</span>
          </button>
          <button className="knowledge-button knowledge-button-secondary" type="button" onClick={onOpenSettings}>
            <Settings2 size={15} />
            <span>{t("knowledge.openSettings")}</span>
          </button>
        </div>
      </header>

      {!runtimeAvailable ? (
        <div className="knowledge-callout knowledge-callout-info" role="status">
          <AlertTriangle size={17} />
          <span>{t("knowledge.desktopOnly")}</span>
        </div>
      ) : null}

      <div className="knowledge-center-scroll">
        <section className="knowledge-stat-grid" aria-label={t("knowledge.sources")}>
          <KnowledgeStat icon={<Database size={18} />} label={t("knowledge.totalDocuments")} value={loading ? "—" : String(statOverview.documentCount)} />
          <KnowledgeStat icon={<CheckCircle2 size={18} />} label={t("knowledge.readyDocuments")} value={loading ? "—" : String(statOverview.readyCount)} />
          <KnowledgeStat icon={<FileText size={18} />} label={t("knowledge.pdfCount")} value={loading ? "—" : String(statOverview.literatureCount)} />
          <KnowledgeStat icon={<StickyNote size={18} />} label={t("knowledge.noteCount")} value={loading ? "—" : String(statOverview.noteCount)} />
        </section>

        <section className="knowledge-capability-grid" aria-label={t("knowledge.indexStatus")}>
          <CapabilityCard icon={<Search size={16} />} label={t("knowledge.lexical")} value={lexicalLabel} tone={statOverview.lexicalDegraded ? "warning" : "ready"} />
          <CapabilityCard
            icon={<Image size={16} />}
            label={t("knowledge.vector")}
            value={vectorLabel}
             tone={!vectorConfigured
               ? "neutral"
               : statOverview.embeddingFailedCount > 0
              ? "warning"
              : statOverview.embeddingPendingCount > 0
                ? "busy"
                : statOverview.embeddingReadyCount > 0
                  ? "ready"
                  : "neutral"}
          />
          <CapabilityCard icon={<Clock3 size={16} />} label={t("knowledge.activeJobs")} value={String(statOverview.activeJobCount)} tone={statOverview.activeJobCount > 0 ? "busy" : "neutral"} />
          <CapabilityCard icon={<Cloud size={16} />} label={t("knowledge.lastIndexed")} value={statOverview.lastIndexedAt ? formatDateTime(statOverview.lastIndexedAt, language) : t("knowledge.notIndexed")} tone="neutral" />
        </section>

        <section className="knowledge-boundary" aria-labelledby="knowledge-boundary-title">
          <div className="knowledge-boundary-icon"><ShieldCheck size={18} /></div>
          <div>
            <h2 id="knowledge-boundary-title">{t("knowledge.scopeTitle")}</h2>
            <p>{t("knowledge.scopeDescription")}</p>
            <p className="knowledge-boundary-subtle">{t("knowledge.privacyDescription")}</p>
          </div>
        </section>

        <LiteratureConsentPanel
          enabled={knowledgeSettings.mineruCloudEnabled}
          runtimeAvailable={runtimeAvailable}
          globalConsent={globalConsent}
          pdfCount={pdfDocuments.length}
          pageBudget={knowledgeSettings.remotePageBudgetPerDay}
          taskBudget={knowledgeSettings.remoteTaskBudgetPerDay}
          busy={actionBusy === "consent:global"}
          hasRunningJobs={runningExtractionJobDocumentIds.size > 0}
          onGrantAll={() => void handleGrantGlobal()}
          onRevokeAll={() => void handleRevokeGlobal()}
          onOpenSettings={onOpenSettings}
          t={t}
        />

        {error ? (
          <div className="knowledge-callout knowledge-callout-error" role="alert">
            <AlertTriangle size={17} />
            <span>{`${t("knowledge.loadFailed")}: ${error}`}</span>
          </div>
        ) : null}
        {actionError ? (
          <div className="knowledge-callout knowledge-callout-error" role="alert">
            <AlertTriangle size={17} />
            <span>{`${t("knowledge.actionFailed")}: ${actionError}`}</span>
            <button className="knowledge-callout-close" type="button" title={t("common.close")} aria-label={t("common.close")} onClick={() => setActionError(null)}><X size={15} /></button>
          </div>
        ) : null}

        <section className="knowledge-search-section" aria-labelledby="knowledge-search-title">
          <div className="knowledge-section-heading">
            <div>
              <h2 id="knowledge-search-title">{t("knowledge.searchTitle")}</h2>
              <p>{t("knowledge.searchDescription")}</p>
            </div>
          </div>
          <form className="knowledge-search-form" onSubmit={(event) => void handleSearch(event)}>
            <label className="knowledge-search-input-wrap">
              <Search size={17} aria-hidden="true" />
              <span className="knowledge-visually-hidden">{t("knowledge.searchTitle")}</span>
              <input
                value={query}
                maxLength={500}
                placeholder={t("knowledge.searchPlaceholder")}
                onChange={(event) => setQuery(event.target.value)}
              />
              {query ? <button type="button" title={t("knowledge.clearSearch")} aria-label={t("knowledge.clearSearch")} onClick={() => { setQuery(""); setSearchResponse(null); }}><X size={15} /></button> : null}
            </label>
            <button className="knowledge-button knowledge-button-primary" type="submit" disabled={!runtimeAvailable || searching || !query.trim()}>
              {searching ? <LoaderCircle size={15} className="knowledge-spin" /> : <FileSearch size={15} />}
              <span>{searching ? t("knowledge.searching") : t("knowledge.searchButton")}</span>
            </button>
          </form>
          {searchResponse ? (
            <div className="knowledge-search-results" aria-live="polite">
              <div className="knowledge-result-summary">
                <strong>{t("knowledge.searchResultCount", { count: visibleHits.length })}</strong>
                <span className="knowledge-status-pill knowledge-state-ready">
                  {t("knowledge.searchMode", { mode: searchResponse.actualMode })}
                  {searchResponse.vectorDimensions ? ` · ${searchResponse.vectorDimensions}d` : ""}
                </span>
                {searchResponse.fallbackReason ? (
                  <span className="knowledge-inline-warning" title={searchResponse.fallbackReason}>
                    <AlertTriangle size={14} />
                    {t("knowledge.searchFallback", { mode: searchResponse.requestedMode })}
                  </span>
                ) : null}
                {searchResponse.lexicalDegraded ? <span className="knowledge-inline-warning"><Info size={14} />{t("knowledge.searchDegraded")}</span> : null}
              </div>
              {searchResponse.insufficientEvidence || visibleHits.length === 0 ? (
                <div className="knowledge-search-empty"><Search size={20} /><span>{t("knowledge.searchNoResults")}</span></div>
              ) : (
                <div className="knowledge-hit-list">
                  {visibleHits.map((hit) => (
                    <SearchHitCard
                      key={hit.chunkId}
                      hit={hit}
                      expanded={expandedChunkId === hit.chunkId}
                      chunk={expandedChunkId === hit.chunkId ? chunk : null}
                      chunkLoading={expandedChunkId === hit.chunkId && chunkLoading}
                      chunkError={expandedChunkId === hit.chunkId ? chunkError : null}
                      onRead={() => void handleReadChunk(hit)}
                    />
                  ))}
                </div>
              )}
            </div>
          ) : null}
        </section>

        <section className="knowledge-index-section" aria-labelledby="knowledge-documents-title">
          <div className="knowledge-section-heading knowledge-section-heading-with-controls">
            <div>
              <h2 id="knowledge-documents-title">{t("knowledge.documentList")}</h2>
              <p>{t("knowledge.documentListDescription")}</p>
            </div>
            <div className="knowledge-filter-tabs" role="tablist" aria-label={t("knowledge.sourceFilter")}>
              <FilterTab active={sourceFilter === "all"} label={t("knowledge.allSources")} onClick={() => setSourceFilter("all")} />
              <FilterTab active={sourceFilter === "literature"} label={t("knowledge.filterLiterature")} onClick={() => setSourceFilter("literature")} />
              <FilterTab active={sourceFilter === "note"} label={t("knowledge.filterNotes")} onClick={() => setSourceFilter("note")} />
            </div>
          </div>
          {loading ? <LoadingRows label={t("knowledge.loading")} /> : visibleDocuments.length === 0 ? (
            <div className="knowledge-empty knowledge-empty-compact">
              <BookOpenCheck size={23} />
              <strong>{documents.length === 0 ? t("knowledge.noDocuments") : t("knowledge.noFilteredDocuments")}</strong>
              <span>{t("knowledge.noDocumentsDescription")}</span>
              <div className="knowledge-empty-actions">
                <button className="knowledge-button knowledge-button-primary" type="button" onClick={onOpenWork}><FileText size={15} />{t("knowledge.openWork")}</button>
                <button className="knowledge-button knowledge-button-secondary" type="button" onClick={onOpenNotes}><StickyNote size={15} />{t("knowledge.openNotes")}</button>
              </div>
            </div>
          ) : (
            <div className="knowledge-document-list">
              {visibleDocuments.map((document) => (
                <DocumentRow
                  key={document.id}
                  document={document}
                  busy={actionBusy === `document:${document.id}`}
                  consent={consentDetails[document.sourceId]}
                  consentLoaded={Object.prototype.hasOwnProperty.call(consentDetails, document.sourceId)}
                  consentLoading={consentLoading === document.sourceId}
                  hasRunningExtractionJob={runningExtractionJobDocumentIds.has(document.id)}
                  onLoadConsent={() => void loadConsentDetail(document.sourceId)}
                  onGrantConsent={() => void handleGrantDocument(document)}
                  onRevokeConsent={() => void handleRevokeDocument(document)}
                  onReparse={() => void handleReparseDocument(document)}
                  onRebuildNote={() => void runAction(
                    `document:${document.id}`,
                    () => rebuildKnowledgeNote(document.sourceId),
                  )}
                  t={t}
                  language={language}
                />
              ))}
            </div>
          )}
        </section>

        <section className="knowledge-jobs-section" aria-labelledby="knowledge-jobs-title">
          <div className="knowledge-section-heading">
            <div>
              <h2 id="knowledge-jobs-title">{t("knowledge.jobList")}</h2>
              <p>{t("knowledge.jobListDescription")}</p>
            </div>
            {activeJobs ? <span className="knowledge-live-indicator"><span />{t("knowledge.autoRefreshing")}</span> : null}
          </div>
          {jobs.length === 0 ? (
            <div className="knowledge-empty knowledge-empty-compact"><Clock3 size={21} /><span>{t("knowledge.noJobs")}</span></div>
          ) : (
            <div className="knowledge-job-list">
              {jobs.map((job) => (
                <JobRow
                  key={job.id}
                  job={job}
                  documentTitle={job.documentId ? documentById.get(job.documentId)?.title ?? null : null}
                  busy={actionBusy === `job:${job.id}`}
                  onCancel={() => void runAction(`job:${job.id}`, () => cancelKnowledgeJob(job.id))}
                  t={t}
                />
              ))}
            </div>
          )}
        </section>

        <div className="knowledge-footer-note">
          <span className="knowledge-footer-icon" aria-hidden="true"><ShieldCheck size={15} /></span>
          <span>{t("knowledge.noAutomaticChat")}</span>
        </div>
      </div>
    </div>
  );
}

function LiteratureConsentPanel({
  enabled,
  runtimeAvailable,
  globalConsent,
  pdfCount,
  pageBudget,
  taskBudget,
  busy,
  hasRunningJobs,
  onGrantAll,
  onRevokeAll,
  onOpenSettings,
  t,
}: {
  enabled: boolean;
  runtimeAvailable: boolean;
  globalConsent: KnowledgeGlobalConsentStatus | null;
  pdfCount: number;
  pageBudget: number;
  taskBudget: number;
  busy: boolean;
  hasRunningJobs: boolean;
  onGrantAll: () => void;
  onRevokeAll: () => void;
  onOpenSettings: () => void;
  t: (key: TranslationKey, values?: Record<string, string | number>) => string;
}) {
  const globalState = globalConsent?.granted
    ? "granted"
    : globalConsent?.state === "revoked"
      ? "revoked"
      : "none";
  return (
    <section className={`knowledge-consent-panel${enabled ? "" : " is-disabled"}`} aria-labelledby="knowledge-consent-title" aria-busy={globalConsent === null || busy}>
      <div className="knowledge-consent-panel-topline">
        <div className="knowledge-consent-icon" aria-hidden="true"><Cloud size={18} /></div>
        <div className="knowledge-consent-copy">
          <div className="knowledge-consent-heading-line">
            <h2 id="knowledge-consent-title">{t("knowledge.consentPanelTitle")}</h2>
            <span className={`knowledge-status-pill knowledge-consent-state-${globalState}`}>
              {globalConsent === null
                ? t("knowledge.consentChecking")
                : globalState === "granted"
                  ? t("knowledge.consentGlobalGranted")
                  : globalState === "revoked"
                    ? t("knowledge.consentGlobalRevoked")
                    : t("knowledge.consentGlobalNone")}
            </span>
          </div>
          <p>{t("knowledge.consentPanelDescription")}</p>
        </div>
      </div>

      {!enabled ? (
        <div className="knowledge-consent-disabled-note">
          <Info size={14} />
          <span>{t("knowledge.consentCloudDisabled")}</span>
        </div>
      ) : null}

      <div className="knowledge-consent-facts">
        <div>
          <strong>{t("knowledge.consentLimitsTitle")}</strong>
          <span>{t("knowledge.consentLimits")}</span>
        </div>
        <div>
          <strong>{t("knowledge.consentFallbackTitle")}</strong>
          <span>{t("knowledge.consentFallback")}</span>
        </div>
        <div>
          <strong>{t("knowledge.consentGlobalStatus")}</strong>
          <span>{t("knowledge.consentBudget", { pages: pageBudget, tasks: taskBudget })}</span>
        </div>
      </div>
      <p className="knowledge-consent-free-note"><Info size={14} />{t("knowledge.consentFreeTier")}</p>
      {hasRunningJobs ? <p className="knowledge-consent-detail-warning"><AlertTriangle size={14} />{t("knowledge.consentStopJob")}</p> : null}

      <div className="knowledge-consent-actions">
        {globalState === "granted" ? (
          <button
            className="knowledge-button knowledge-button-secondary"
            type="button"
            disabled={!runtimeAvailable || !enabled || busy || hasRunningJobs}
            onClick={onRevokeAll}
          >
            {busy ? <LoaderCircle size={15} className="knowledge-spin" /> : <ShieldCheck size={15} />}
            <span>{busy ? t("knowledge.rebuilding") : t("knowledge.consentRevokeAll")}</span>
          </button>
        ) : (
          <button
            className="knowledge-button knowledge-button-primary"
            type="button"
            disabled={!runtimeAvailable || !enabled || busy}
            onClick={onGrantAll}
          >
            {busy ? <LoaderCircle size={15} className="knowledge-spin" /> : <Cloud size={15} />}
            <span>{busy ? t("knowledge.rebuilding") : t("knowledge.consentGrantAll")}</span>
          </button>
        )}
        <span className="knowledge-consent-count">
          {pdfCount > 0 ? `${pdfCount} ${t("knowledge.filterLiterature")}` : t("knowledge.consentPdfsPendingSync")}
        </span>
        <button className="knowledge-button knowledge-button-secondary" type="button" onClick={onOpenSettings}>
          <Settings2 size={15} />
          <span>{t("knowledge.consentOpenSettings")}</span>
        </button>
      </div>
    </section>
  );
}

function KnowledgeStat({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return <div className="knowledge-stat"><span className="knowledge-stat-icon">{icon}</span><span>{label}</span><strong>{value}</strong></div>;
}

function CapabilityCard({ icon, label, value, tone }: { icon: ReactNode; label: string; value: string; tone: "ready" | "warning" | "busy" | "neutral" }) {
  return <div className={`knowledge-capability-card knowledge-capability-${tone}`}><span>{icon}</span><div><small>{label}</small><strong>{value}</strong></div></div>;
}

function FilterTab({ active, label, onClick }: { active: boolean; label: string; onClick: () => void }) {
  return <button className={`knowledge-filter-tab${active ? " is-active" : ""}`} type="button" role="tab" aria-selected={active} onClick={onClick}>{label}</button>;
}

function DocumentRow({
  document,
  busy,
  consent,
  consentLoaded,
  consentLoading,
  onLoadConsent,
  onGrantConsent,
  onRevokeConsent,
  onReparse,
  onRebuildNote,
  t,
  language,
  hasRunningExtractionJob,
}: {
  document: KnowledgeDocumentStatus;
  busy: boolean;
  consent: KnowledgeConsentStatus | null | undefined;
  consentLoaded: boolean;
  consentLoading: boolean;
  onLoadConsent: () => void;
  onGrantConsent: () => void;
  onRevokeConsent: () => void;
  onReparse: () => void;
  onRebuildNote: () => void;
  t: (key: TranslationKey, values?: Record<string, string | number>) => string;
  language: string;
  hasRunningExtractionJob: boolean;
}) {
  const isNote = document.sourceClass === "note";
  const canAct = document.state !== "deleted";
  const consentState = isNote ? "not_required" : documentConsentState(document, consent);
  const cloudGranted = !isNote && (consent ? consent.granted : document.cloudConsentState === "granted");
  const primaryAction = cloudGranted ? onReparse : onGrantConsent;
  const primaryLabel = cloudGranted ? t("knowledge.consentReparse") : t("knowledge.consentGrant");
  const consentDetailId = `knowledge-consent-detail-${document.id}`;
  const consentDetailButtonId = `knowledge-consent-detail-button-${document.id}`;
  return (
    <article className={`knowledge-document-row${consentState === "stale" ? " has-stale-consent" : ""}`}>
      <span className={`knowledge-document-icon ${isNote ? "is-note" : "is-literature"}`} aria-hidden="true">{isNote ? <StickyNote size={17} /> : <FileText size={17} />}</span>
      <div className="knowledge-document-copy">
        <div className="knowledge-document-title-line">
          <strong title={document.title}>{document.title || t("knowledge.untitled")}</strong>
          <StatusPill state={document.state} label={documentStateLabel(document.state, t)} />
        </div>
        <span className="knowledge-document-kind">{isNote ? t("knowledge.noteKind") : t("knowledge.literatureKind")} · {shortHash(document.sourceHash)}</span>
        {!isNote ? (
          <div className={`knowledge-document-consent knowledge-consent-inline-${consentState}`}>
            <Cloud size={13} aria-hidden="true" />
            <span>{t("knowledge.consentGlobalStatus")}: {consentStateLabel(consentState, t)}</span>
            {consent?.effectiveScope ? <small>{consent.effectiveScope === "global" ? t("knowledge.consentGlobalActive") : t("knowledge.consentDocumentActive")}</small> : null}
          </div>
        ) : null}
        <div className="knowledge-document-metrics">
          <span>{t("knowledge.chunkCount", { count: document.chunkCount })}</span>
          <span>{t("knowledge.assetCount", { count: document.assetCount })}</span>
          {document.warningCount > 0 ? <span className="knowledge-warning-text"><AlertTriangle size={13} />{t("knowledge.warningCount", { count: document.warningCount })}</span> : null}
          <time dateTime={new Date(document.updatedAt).toISOString()}>{formatDateTime(document.updatedAt, language)}</time>
        </div>
      </div>
      <div className="knowledge-document-actions">
        {isNote ? (
          <button
            className="knowledge-button knowledge-button-small knowledge-button-secondary"
            type="button"
            disabled={!canAct || busy}
            onClick={onRebuildNote}
            title={t("knowledge.rebuildNote")}
          >
            {busy ? <LoaderCircle size={14} className="knowledge-spin" /> : <RefreshCw size={14} />}
            <span>{busy ? t("knowledge.rebuilding") : t("knowledge.rebuildNote")}</span>
          </button>
        ) : (
          <>
            <button
              className={`knowledge-button knowledge-button-small ${cloudGranted ? "knowledge-button-secondary" : "knowledge-button-primary"}`}
              type="button"
              disabled={!canAct || busy || consentLoading || hasRunningExtractionJob}
              onClick={primaryAction}
              title={cloudGranted ? t("knowledge.consentReparse") : t("knowledge.consentGrant")}
            >
              {busy ? <LoaderCircle size={14} className="knowledge-spin" /> : cloudGranted ? <RefreshCw size={14} /> : <Cloud size={14} />}
              <span>{busy ? t("knowledge.rebuilding") : primaryLabel}</span>
            </button>
            <button
              className="knowledge-button knowledge-button-small knowledge-button-secondary knowledge-consent-detail-button"
              type="button"
              disabled={!canAct || busy || consentLoading}
              onClick={onLoadConsent}
              title={t("knowledge.consentDetail")}
              aria-expanded={consentLoaded}
              aria-controls={consentDetailId}
              id={consentDetailButtonId}
            >
              {consentLoading ? <LoaderCircle size={14} className="knowledge-spin" /> : <Info size={14} />}
              <span>{consentLoading ? t("knowledge.consentRefreshing") : t("knowledge.consentDetail")}</span>
            </button>
          </>
        )}
      </div>
      {!isNote && consentLoaded ? (
        <ConsentDetail
          status={consent ?? null}
          loading={consentLoading}
          busy={busy}
          hasRunningJob={hasRunningExtractionJob}
          id={consentDetailId}
          labelledBy={consentDetailButtonId}
          onRefresh={onLoadConsent}
          onRevoke={onRevokeConsent}
          t={t}
          language={language}
        />
      ) : null}
    </article>
  );
}

function ConsentDetail({
  status,
  loading,
  busy,
  hasRunningJob,
  id,
  labelledBy,
  onRefresh,
  onRevoke,
  t,
  language,
}: {
  status: KnowledgeConsentStatus | null;
  loading: boolean;
  busy: boolean;
  hasRunningJob: boolean;
  id: string;
  labelledBy: string;
  onRefresh: () => void;
  onRevoke: () => void;
  t: (key: TranslationKey, values?: Record<string, string | number>) => string;
  language: string;
}) {
  return (
    <div id={id} className="knowledge-consent-detail" role="region" aria-labelledby={labelledBy} aria-busy={loading}>
      {loading && !status ? (
        <div className="knowledge-consent-detail-status" role="status"><LoaderCircle size={14} className="knowledge-spin" />{t("knowledge.consentRefreshing")}</div>
      ) : status ? (
        <>
          <div className="knowledge-consent-detail-status">
            <span className={`knowledge-consent-detail-pill knowledge-consent-inline-${documentConsentStateFromStatus(status)}`}>
              <Cloud size={13} aria-hidden="true" />
              {consentStateLabel(documentConsentStateFromStatus(status), t)}
            </span>
            {status.effectiveScope === "global" ? <span>{t("knowledge.consentGlobalActive")}</span> : status.effectiveScope === "document" ? <span>{t("knowledge.consentDocumentActive")}</span> : null}
            {status.grantedAt ? <span>{t("knowledge.consentGrantedAt", { time: formatDateTime(status.grantedAt, language) })}</span> : null}
          </div>
          {status.documentConsentState === "stale" ? <p className="knowledge-consent-detail-warning"><AlertTriangle size={14} />{t("knowledge.consentHashChanged")}</p> : null}
          {!status.granted && status.documentConsentState !== "stale" ? <p className="knowledge-consent-detail-muted">{t("knowledge.consentNoPermission")}</p> : null}
          {hasRunningJob ? <p className="knowledge-consent-detail-warning"><AlertTriangle size={14} />{t("knowledge.consentStopJob")}</p> : null}
          <div className="knowledge-consent-detail-actions">
            {status.documentGranted ? (
              <button className="knowledge-button knowledge-button-small knowledge-button-secondary" type="button" disabled={busy || hasRunningJob} onClick={onRevoke}>
                <ShieldCheck size={14} />
                <span>{t("knowledge.consentRevoke")}</span>
              </button>
            ) : null}
            <button className="knowledge-button knowledge-button-small knowledge-button-secondary" type="button" disabled={busy || loading} onClick={onRefresh}>
              {loading ? <LoaderCircle size={14} className="knowledge-spin" /> : <RefreshCw size={14} />}
              <span>{loading ? t("knowledge.consentRefreshing") : t("knowledge.consentRefresh")}</span>
            </button>
          </div>
        </>
      ) : (
        <div className="knowledge-consent-detail-status knowledge-consent-detail-muted">
          <AlertTriangle size={14} />{t("knowledge.consentStatusUnavailable")}
          <button className="knowledge-button knowledge-button-small knowledge-button-secondary" type="button" disabled={loading || busy} onClick={onRefresh}>
            {loading ? <LoaderCircle size={14} className="knowledge-spin" /> : <RefreshCw size={14} />}
            <span>{loading ? t("knowledge.consentRefreshing") : t("knowledge.consentRefresh")}</span>
          </button>
        </div>
      )}
    </div>
  );
}

function JobRow({
  job,
  documentTitle,
  busy,
  onCancel,
  t,
}: {
  job: KnowledgeJobView;
  documentTitle: string | null;
  busy: boolean;
  onCancel: () => void;
  t: (key: TranslationKey, values?: Record<string, string | number>) => string;
}) {
  const progress = job.totalUnits > 0
    ? Math.min(100, Math.round((job.completedUnits / job.totalUnits) * 100))
    : null;
  const cancellable = ACTIVE_JOB_STATES.has(job.state) && job.state !== "cancelling";
  return (
    <article className="knowledge-job-row">
      <span className={`knowledge-job-icon knowledge-job-${job.state}`} aria-hidden="true">{job.state === "succeeded" ? <CheckCircle2 size={16} /> : job.state === "failed" || job.state === "stale" ? <AlertTriangle size={16} /> : job.state === "cancelled" ? <Ban size={16} /> : <Clock3 size={16} />}</span>
      <div className="knowledge-job-copy">
        <div className="knowledge-job-title-line">
          <strong>{documentTitle ?? t("knowledge.globalJob")}</strong>
          <StatusPill state={job.state} label={jobStateLabel(job.state, t)} />
        </div>
        <span className="knowledge-job-meta">{job.jobKind} · {t("knowledge.stageLabel", { stage: jobStageLabel(job.stage, t) })} · {shortHash(job.id)}</span>
        {progress !== null ? (
          <div
            className="knowledge-progress-line"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={progress}
            aria-valuetext={t("knowledge.jobProgress", { current: job.completedUnits, total: job.totalUnits })}
          >
            <span aria-hidden="true"><span style={{ transform: `scaleX(${progress / 100})` }} /></span>
            <small>{t("knowledge.jobProgress", { current: job.completedUnits, total: job.totalUnits })}</small>
          </div>
        ) : null}
        {job.errorMessage || job.errorCode ? <p className="knowledge-job-error">{job.errorCode ? `${job.errorCode}: ` : ""}{job.errorMessage ?? t("knowledge.jobFailed")}</p> : null}
      </div>
      {cancellable ? <button className="knowledge-icon-button" type="button" title={t("knowledge.cancelJob")} aria-label={t("knowledge.cancelJob")} disabled={busy} onClick={onCancel}>{busy ? <LoaderCircle size={15} className="knowledge-spin" /> : <Ban size={15} />}</button> : null}
    </article>
  );
}

function SearchHitCard({
  hit,
  expanded,
  chunk,
  chunkLoading,
  chunkError,
  onRead,
}: {
  hit: KnowledgeSearchHit;
  expanded: boolean;
  chunk: KnowledgeChunkView | null;
  chunkLoading: boolean;
  chunkError: string | null;
  onRead: () => void;
}) {
  const { t } = useI18n();
  return (
    <article className={`knowledge-hit-card${expanded ? " is-expanded" : ""}`}>
      <button className="knowledge-hit-main" type="button" onClick={onRead} aria-expanded={expanded}>
        <span className="knowledge-hit-toggle" aria-hidden="true">{expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}</span>
        <span className="knowledge-hit-copy">
          <strong title={hit.title}>{hit.title || t("knowledge.untitled")}</strong>
          <span>{hit.snippet || hit.text}</span>
          <small>
            <span>{hit.sourceClass === "note" ? t("knowledge.noteKind") : t("knowledge.literatureKind")}</span>
            <span>{formatCitation(hit, t)}</span>
            {hit.headingPath.length > 0 ? <span title={hit.headingPath.join(" / ")}>{hit.headingPath.join(" / ")}</span> : null}
            {hit.elementTypes.map((type) => <span key={type}>{type}</span>)}
          </small>
        </span>
      </button>
      {expanded ? (
        <div className="knowledge-hit-detail">
          {chunkLoading ? <div className="knowledge-chunk-loading"><LoaderCircle size={16} className="knowledge-spin" />{t("knowledge.chunkLoading")}</div> : null}
          {chunkError ? <div className="knowledge-inline-error"><AlertTriangle size={14} />{chunkError}</div> : null}
          {chunk ? <><div className="knowledge-chunk-location"><Info size={14} />{formatCitation(chunk, t)} · {t("knowledge.sourceHash")}: {shortHash(chunk.sourceHash)}</div><pre>{chunk.text}</pre></> : null}
        </div>
      ) : null}
    </article>
  );
}

function StatusPill({ state, label }: { state: string; label: string }) {
  return <span className={`knowledge-status-pill knowledge-state-${state.replace(/[^a-z0-9_-]/gi, "-")}`}>{label}</span>;
}

function LoadingRows({ label }: { label: string }) {
  return <div className="knowledge-loading" role="status" aria-label={label}><span /><span /><span /></div>;
}

function documentConsentState(
  document: KnowledgeDocumentStatus,
  consent: KnowledgeConsentStatus | null | undefined,
) {
  if (consent?.documentConsentState === "stale") return "stale";
  if (consent?.granted || (!consent && document.cloudConsentState === "granted")) return "granted";
  if (consent?.documentConsentState === "revoked" || document.cloudConsentState === "revoked") return "revoked";
  return "awaiting";
}

function documentConsentStateFromStatus(status: KnowledgeConsentStatus) {
  if (status.documentConsentState === "stale") return "stale";
  if (status.granted) return "granted";
  if (status.documentConsentState === "revoked" || status.globalConsentState === "revoked") return "revoked";
  return "awaiting";
}

function consentStateLabel(state: string, t: (key: TranslationKey) => string) {
  return CONSENT_STATE_KEYS[state] ? t(CONSENT_STATE_KEYS[state]) : state;
}

function documentStateLabel(state: string, t: (key: TranslationKey) => string) {
  return DOCUMENT_STATE_KEYS[state] ? t(DOCUMENT_STATE_KEYS[state]) : state;
}

function jobStateLabel(state: string, t: (key: TranslationKey) => string) {
  return JOB_STATE_KEYS[state] ? t(JOB_STATE_KEYS[state]) : state;
}

function jobStageLabel(stage: string, t: (key: TranslationKey) => string) {
  return JOB_STAGE_KEYS[stage] ? t(JOB_STAGE_KEYS[stage]) : stage;
}

function formatCitation(
  value: Pick<KnowledgeSearchHit, "pageStart" | "pageEnd" | "lineStart" | "lineEnd">,
  t: (key: TranslationKey, values?: Record<string, string | number>) => string,
) {
  if (value.pageStart !== null && value.pageStart !== undefined) {
    const start = value.pageStart + 1;
    const end = value.pageEnd !== null && value.pageEnd !== undefined ? value.pageEnd + 1 : start;
    return start === end ? t("knowledge.page", { page: start }) : t("knowledge.pages", { start, end });
  }
  if (value.lineStart !== null && value.lineStart !== undefined) {
    const end = value.lineEnd ?? value.lineStart;
    return value.lineStart === end ? t("knowledge.line", { line: value.lineStart }) : t("knowledge.lines", { start: value.lineStart, end });
  }
  return t("knowledge.locationUnknown");
}

function emptyOverview(): KnowledgeOverview {
  return {
    documentCount: 0,
    literatureCount: 0,
    noteCount: 0,
    readyCount: 0,
    pendingCount: 0,
    failedCount: 0,
    activeJobCount: 0,
    fts5Available: false,
    tokenizer: "none",
    lexicalDegraded: true,
    embeddingReadyCount: 0,
    embeddingPendingCount: 0,
    embeddingFailedCount: 0,
    embeddingDimensions: [],
    lastIndexedAt: null,
  };
}

function emptyGlobalConsent(): KnowledgeGlobalConsentStatus {
  return {
    state: "none",
    granted: false,
    grantedAt: null,
    revokedAt: null,
  };
}

function shortHash(value: string) {
  const normalized = value.trim();
  return normalized.length > 16 ? `${normalized.slice(0, 8)}…${normalized.slice(-6)}` : normalized || "—";
}

function formatDateTime(timestamp: number, language: string) {
  return new Intl.DateTimeFormat(language === "en" ? "en-US" : "zh-CN", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp));
}

function toErrorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}
