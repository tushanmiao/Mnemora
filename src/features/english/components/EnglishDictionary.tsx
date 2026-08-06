import { ChevronLeft, ChevronRight, Download, ExternalLink, Headphones, LoaderCircle, Plus, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "../../../i18n/I18nProvider";
import { createManagedAudio, type ManagedAudio } from "../../../runtime/media/managedAudio";
import {
  deleteEnglishDictionary,
  downloadEnglishDictionary,
  getEnglishWord,
  searchEnglishDictionary,
  type EnglishDictionaryStatus,
  type EnglishDownloadProgress,
  type EnglishGroupSummary,
  type EnglishWordEntry,
  type EnglishWordSummary,
} from "../api/english";

const SOURCE_URL = "https://isdc.pages.dev/";

type Props = {
  status: EnglishDictionaryStatus;
  onStatusChange: (status: EnglishDictionaryStatus) => void;
  onGroupsChange: (groups: EnglishGroupSummary[]) => void;
  hasPlan: boolean;
  pageSize: number;
  onAddWord: (wordId: number) => Promise<void>;
};

export default function EnglishDictionary({ status, onStatusChange, onGroupsChange, hasPlan, pageSize, onAddWord }: Props) {
  const { t } = useI18n();
  const [groups, setGroups] = useState<EnglishGroupSummary[]>([]);
  const [items, setItems] = useState<EnglishWordSummary[]>([]);
  const [resultTotal, setResultTotal] = useState(0);
  const [selected, setSelected] = useState<EnglishWordEntry | null>(null);
  const [query, setQuery] = useState("");
  const [groupId, setGroupId] = useState<number | null>(null);
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState<"download" | "search" | "delete" | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<EnglishDownloadProgress | null>(null);
  const [error, setError] = useState("");
  const [addingWordId, setAddingWordId] = useState<number | null>(null);
  const searchRequestRef = useRef(0);

  const search = async (nextQuery = query, nextGroupId = groupId, nextPage = page) => {
    const request = ++searchRequestRef.current;
    setBusy("search");
    setError("");
    try {
      const result = await searchEnglishDictionary(nextQuery, nextGroupId, pageSize, nextPage * pageSize);
      if (request !== searchRequestRef.current) return;
      setItems(result.items);
      setResultTotal(result.total);
      setGroups(result.groups);
      onGroupsChange(result.groups);
      const nextSelected = result.items.length > 0 ? await getEnglishWord(result.items[0].id) : null;
      if (request === searchRequestRef.current) setSelected(nextSelected);
    } catch (reason) {
      if (request === searchRequestRef.current) setError(formatError(reason));
    } finally {
      if (request === searchRequestRef.current) setBusy(null);
    }
  };

  useEffect(() => {
    if (!status.installed) return;
    const timer = window.setTimeout(() => void search(), 180);
    return () => {
      window.clearTimeout(timer);
      searchRequestRef.current += 1;
    };
  }, [status.installed, query, groupId, page, pageSize]);

  useEffect(() => setPage(0), [pageSize]);

  const download = async () => {
    setBusy("download");
    setError("");
    setDownloadProgress({ phase: "download", downloadedBytes: 0, totalBytes: null, indexedWords: 0, totalWords: 0, progress: null, finished: false });
    try {
      const next = await downloadEnglishDictionary(setDownloadProgress);
      onStatusChange(next);
      setPage(0);
      await search("", null, 0);
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(null);
      setDownloadProgress(null);
    }
  };

  const remove = async () => {
    if (!window.confirm(t("english.deleteConfirm"))) return;
    searchRequestRef.current += 1;
    setBusy("delete");
    try {
      await deleteEnglishDictionary();
      onStatusChange({ ...status, installed: false, wordCount: 0, dataSizeBytes: 0, downloadedAt: null });
      setItems([]);
      setGroups([]);
      onGroupsChange([]);
      setSelected(null);
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(null);
    }
  };

  const openSource = async () => {
    if (isTauri()) await openUrl(SOURCE_URL);
    else window.open(SOURCE_URL, "_blank", "noopener,noreferrer");
  };

  const selectedGroupName = useMemo(() => groups.find((group) => group.id === groupId)?.name, [groupId, groups]);
  const totalPages = Math.max(1, Math.ceil(resultTotal / pageSize));

  if (!status.installed) {
    return <section className="english-install-panel">
      <h2>{t("english.downloadTitle")}</h2>
      <p>{t("english.downloadDescription")}</p>
      <p className="english-source-note">{t("english.sourceNote")}</p>
      <div className="english-install-actions">
        <button type="button" onClick={() => void download()} disabled={busy !== null}>{busy === "download" ? <LoaderCircle className="english-spinner" size={16} /> : <Download size={16} />}{busy === "download" ? t("english.downloading") : t("english.download")}</button>
        <button type="button" className="english-secondary-button" onClick={() => void openSource()}><ExternalLink size={16} />{t("english.openSource")}</button>
      </div>
      {downloadProgress ? <EnglishDownloadProgressView progress={downloadProgress} /> : null}
      {error ? <p className="english-error" role="alert">{error}</p> : null}
    </section>;
  }

  return <div className="english-dictionary-page">
    <div className="english-toolbar">
      <label className="english-search"><Search size={16} /><input value={query} onChange={(event) => { setQuery(event.target.value); setPage(0); }} placeholder={t("english.searchPlaceholder")} /></label>
      <select value={groupId ?? "all"} onChange={(event) => { setGroupId(event.target.value === "all" ? null : Number(event.target.value)); setPage(0); }} aria-label={t("english.groupFilter")}>
        <option value="all">{t("english.allGroups")}</option>
        {groups.map((group) => <option key={group.id} value={group.id}>{group.name} ({group.count})</option>)}
      </select>
      <button className="english-icon-button" type="button" onClick={() => void remove()} disabled={busy !== null} title={t("english.deleteDictionary")} aria-label={t("english.deleteDictionary")}><Trash2 size={16} /></button>
    </div>
    <div className="english-main">
      <section className="english-results" aria-label={t("english.results")}>
        <div className="english-results-heading"><span>{selectedGroupName ?? t("english.allGroups")}</span><small>{resultTotal.toLocaleString()} 个</small></div>
        {busy === "search" ? <div className="english-inline-state"><LoaderCircle className="english-spinner" size={16} />{t("english.searching")}</div> : null}
        {items.map((item) => <button key={item.id} type="button" className={`english-result${selected?.id === item.id ? " is-active" : ""}`} onClick={() => void getEnglishWord(item.id).then(setSelected).catch((reason) => setError(formatError(reason)))}><strong>{item.word}</strong><span>{item.pronunciation}</span><small>{item.groupName}{item.occurrence ? ` · ${item.occurrence}` : ""}</small></button>)}
        {items.length === 0 && busy !== "search" ? <p className="english-empty">{t("english.noResults")}</p> : null}
        {resultTotal > pageSize ? <div className="english-results-pagination">
          <button type="button" title="上一页" aria-label="上一页" disabled={page === 0 || busy === "search"} onClick={() => setPage((current) => Math.max(0, current - 1))}><ChevronLeft size={16} /></button>
          <span>第 {page + 1} / {totalPages} 页</span>
          <button type="button" title="下一页" aria-label="下一页" disabled={page + 1 >= totalPages || busy === "search"} onClick={() => setPage((current) => Math.min(totalPages - 1, current + 1))}><ChevronRight size={16} /></button>
        </div> : null}
      </section>
      <EnglishEntry entry={selected} hasPlan={hasPlan} addingWordId={addingWordId} onAddWord={async (wordId) => {
        setAddingWordId(wordId);
        setError("");
        try { await onAddWord(wordId); } catch (reason) { setError(formatError(reason)); }
        finally { setAddingWordId(null); }
      }} />
    </div>
    {error ? <p className="english-error" role="alert">{error}</p> : null}
    <footer className="english-attribution">{t("english.attribution")} <button type="button" onClick={() => void openSource()}>{status.sourceName}</button><span> · {formatSize(status.dataSizeBytes)}</span></footer>
  </div>;
}

function EnglishEntry({ entry, hasPlan, addingWordId, onAddWord }: { entry: EnglishWordEntry | null; hasPlan: boolean; addingWordId: number | null; onAddWord: (wordId: number) => Promise<void> }) {
  const { t } = useI18n();
  const audioRef = useRef<ManagedAudio | null>(null);
  useEffect(() => () => audioRef.current?.release(), [entry?.id]);
  if (!entry) return <section className="english-entry english-entry-empty">{t("english.selectWord")}</section>;
  const play = (url: string) => {
    audioRef.current?.release();
    const managed = createManagedAudio(url, `english-entry:${entry.id}`);
    audioRef.current = managed;
    void managed.audio.play().catch(() => undefined);
  };
  return <section className="english-entry">
    <div className="english-entry-header"><div><h2>{entry.word}</h2><p>/{entry.pronunciation}/</p></div><div className="english-audio-actions">{hasPlan ? <button type="button" onClick={() => void onAddWord(entry.id)} disabled={addingWordId !== null} title="加入当前词书" aria-label="加入当前词书">{addingWordId === entry.id ? <LoaderCircle className="english-spinner" size={16} /> : <Plus size={16} />}</button> : null}{entry.britishAudio ? <button type="button" onClick={() => play(entry.britishAudio)} title={t("english.britishAudio")}><Headphones size={16} />UK</button> : null}{entry.americanAudio ? <button type="button" onClick={() => play(entry.americanAudio)} title={t("english.americanAudio")}><Headphones size={16} />US</button> : null}</div></div>
    {entry.translation ? <DetailSection title={t("english.translation")}><p>{entry.translation}</p></DetailSection> : null}
    {entry.example ? <DetailSection title={t("english.example")}><p>{entry.example}</p>{entry.exampleTranslation ? <p className="english-muted">{entry.exampleTranslation}</p> : null}</DetailSection> : null}
    {entry.englishDefinition ? <DetailSection title={t("english.definition")}><p>{entry.englishDefinition}</p></DetailSection> : null}
    {entry.mnemonic || entry.rootAffixes ? <DetailSection title={t("english.wordFormation")}>{entry.rootAffixes ? <p>{entry.rootAffixes}</p> : null}{entry.mnemonic ? <p className="english-muted">{entry.mnemonic}</p> : null}</DetailSection> : null}
    {entry.derivedWords.length > 0 ? <DetailSection title={t("english.derivedWords")}><p>{entry.derivedWords.map((item) => `${item.word} ${item.partOfSpeech} ${item.definition}`).join(" · ")}</p></DetailSection> : null}
    {entry.examExamples.length > 0 ? <DetailSection title={t("english.examExamples")}><div className="english-example-list">{entry.examExamples.slice(0, 10).map((item, index) => <div key={`${item.source}-${index}`}><p>{item.sentence}</p><small>{item.source} {item.section}</small></div>)}</div></DetailSection> : null}
  </section>;
}

function EnglishDownloadProgressView({ progress }: { progress: EnglishDownloadProgress }) {
  const percent = progress.progress ?? 0;
  const detail = progress.phase === "index" ? `正在建立索引：${progress.indexedWords.toLocaleString()} / ${progress.totalWords.toLocaleString()}` : progress.phase === "decode" ? "正在解压并校验词库" : progress.totalBytes ? `${formatSize(progress.downloadedBytes)} / ${formatSize(progress.totalBytes)}` : formatSize(progress.downloadedBytes);
  return <div className="english-download-progress" role="status"><div className={`english-progress-track${progress.progress === null ? " is-indeterminate" : ""}`}><span style={{ width: progress.progress === null ? "28%" : `${percent}%` }} /></div><div className="english-progress-meta"><span>{detail}</span><strong>{progress.progress === null ? "..." : `${percent}%`}</strong></div></div>;
}

function DetailSection({ title, children }: { title: string; children: React.ReactNode }) { return <div className="english-detail"><h3>{title}</h3>{children}</div>; }
function formatSize(bytes: number) { return bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }
function formatError(error: unknown) { return error instanceof Error ? error.message : String(error); }
