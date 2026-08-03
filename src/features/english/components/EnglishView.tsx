import { Download, ExternalLink, Headphones, LoaderCircle, RefreshCw, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "@tauri-apps/api/core";
import { useI18n } from "../../../i18n/I18nProvider";
import {
  deleteEnglishDictionary,
  downloadEnglishDictionary,
  getEnglishDictionaryStatus,
  getEnglishWord,
  releaseEnglishDictionary,
  searchEnglishDictionary,
  type EnglishDictionaryStatus,
  type EnglishGroupSummary,
  type EnglishWordEntry,
  type EnglishWordSummary,
} from "../api/english";
import "../styles/english.css";

const SOURCE_URL = "https://isdc.pages.dev/";

function formatSize(bytes: number) {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export default function EnglishView() {
  const { t } = useI18n();
  const [status, setStatus] = useState<EnglishDictionaryStatus | null>(null);
  const [groups, setGroups] = useState<EnglishGroupSummary[]>([]);
  const [items, setItems] = useState<EnglishWordSummary[]>([]);
  const [resultTotal, setResultTotal] = useState(0);
  const [selected, setSelected] = useState<EnglishWordEntry | null>(null);
  const [query, setQuery] = useState("");
  const [groupId, setGroupId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<"download" | "search" | "delete" | null>(null);
  const [error, setError] = useState("");

  const loadStatus = async () => {
    setLoading(true);
    setError("");
    try {
      const next = await getEnglishDictionaryStatus();
      setStatus(next);
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let active = true;
    void getEnglishDictionaryStatus().then((next) => {
      if (active) { setStatus(next); setLoading(false); }
    }).catch((reason) => {
      if (active) { setError(formatError(reason)); setLoading(false); }
    });
    return () => {
      active = false;
      void releaseEnglishDictionary();
    };
  }, []);

  const search = async (nextQuery = query, nextGroupId = groupId) => {
    setBusy("search");
    setError("");
    try {
      const result = await searchEnglishDictionary(nextQuery, nextGroupId);
      setItems(result.items);
      setResultTotal(result.total);
      setGroups(result.groups);
      if (result.items.length > 0) {
        setSelected(await getEnglishWord(result.items[0].id));
      } else {
        setSelected(null);
      }
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(null);
    }
  };

  useEffect(() => {
    if (!status?.installed) return;
    const timer = window.setTimeout(() => { void search(); }, 180);
    return () => window.clearTimeout(timer);
  }, [status?.installed, query, groupId]);

  const download = async () => {
    setBusy("download");
    setError("");
    try {
      const next = await downloadEnglishDictionary();
      setStatus(next);
      setSelected(null);
      await search("", null);
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(null);
    }
  };

  const remove = async () => {
    if (!window.confirm(t("english.deleteConfirm"))) return;
    setBusy("delete");
    try {
      await deleteEnglishDictionary();
      setStatus((current) => current ? { ...current, installed: false, wordCount: 0, dataSizeBytes: 0, downloadedAt: null } : current);
      setItems([]);
      setResultTotal(0);
      setGroups([]);
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

  if (loading) return <div className="english-view english-state" role="status"><LoaderCircle className="english-spinner" size={18} />{t("english.loading")}</div>;

  return (
    <div className="english-view">
      <header className="english-header">
        <div>
          <p className="english-eyebrow">Mnemora / English</p>
          <h1>{t("english.title")}</h1>
          <p>{t("english.subtitle")}</p>
        </div>
        <button className="english-icon-button" type="button" onClick={() => void loadStatus()} title={t("english.refresh")} aria-label={t("english.refresh")}><RefreshCw size={16} /></button>
      </header>

      {!status?.installed ? (
        <section className="english-install-panel">
          <h2>{t("english.downloadTitle")}</h2>
          <p>{t("english.downloadDescription")}</p>
          <p className="english-source-note">{t("english.sourceNote")}</p>
          <div className="english-install-actions">
            <button type="button" onClick={() => void download()} disabled={busy !== null}><DownloadIcon busy={busy === "download"} />{busy === "download" ? t("english.downloading") : t("english.download")}</button>
            <button type="button" className="english-secondary-button" onClick={() => void openSource()}><ExternalLink size={16} />{t("english.openSource")}</button>
          </div>
        </section>
      ) : (
        <>
          <div className="english-toolbar">
            <label className="english-search"><Search size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("english.searchPlaceholder")} /></label>
            <select value={groupId ?? "all"} onChange={(event) => setGroupId(event.target.value === "all" ? null : Number(event.target.value))} aria-label={t("english.groupFilter")}>
              <option value="all">{t("english.allGroups")}</option>
              {groups.map((group) => <option key={group.id} value={group.id}>{group.name} ({group.count})</option>)}
            </select>
            <button className="english-icon-button" type="button" onClick={() => void remove()} disabled={busy !== null} title={t("english.deleteDictionary")} aria-label={t("english.deleteDictionary")}><Trash2 size={16} /></button>
          </div>
          <div className="english-main">
            <section className="english-results" aria-label={t("english.results")}>
              <div className="english-results-heading"><span>{selectedGroupName ?? t("english.allGroups")}</span><small>{items.length} / {resultTotal}</small></div>
              {busy === "search" ? <div className="english-inline-state"><LoaderCircle className="english-spinner" size={16} />{t("english.searching")}</div> : null}
              {items.map((item) => <button key={item.id} type="button" className={`english-result${selected?.id === item.id ? " is-active" : ""}`} onClick={() => void getEnglishWord(item.id).then(setSelected).catch((reason) => setError(formatError(reason)))}><strong>{item.word}</strong><span>{item.pronunciation}</span><small>{item.groupName}{item.occurrence ? ` · ${item.occurrence}` : ""}</small></button>)}
              {items.length === 0 && busy !== "search" ? <p className="english-empty">{t("english.noResults")}</p> : null}
            </section>
            <EnglishEntry entry={selected} t={t} />
          </div>
        </>
      )}
      {error ? <p className="english-error" role="alert">{error}</p> : null}
      <footer className="english-attribution">
        {t("english.attribution")} <button type="button" onClick={() => void openSource()}>{status?.sourceName ?? "isdc.pages.dev"}</button>
        {status?.installed ? <span> · {formatSize(status.dataSizeBytes)}</span> : null}
      </footer>
    </div>
  );
}

function DownloadIcon({ busy }: { busy: boolean }) { return busy ? <LoaderCircle className="english-spinner" size={16} /> : <Download size={16} />; }

function EnglishEntry({ entry, t }: { entry: EnglishWordEntry | null; t: ReturnType<typeof useI18n>["t"] }) {
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => () => {
    const audio = audioRef.current;
    if (!audio) return;
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
    audioRef.current = null;
  }, [entry?.id]);

  if (!entry) return <section className="english-entry english-entry-empty">{t("english.selectWord")}</section>;
  const play = (url: string) => {
    if (!url) return;
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current.removeAttribute("src");
      audioRef.current.load();
    }
    const audio = new Audio();
    audio.preload = "none";
    audio.src = url;
    audioRef.current = audio;
    void audio.play().catch(() => undefined);
  };
  return <section className="english-entry">
    <div className="english-entry-header"><div><h2>{entry.word}</h2><p>/{entry.pronunciation}/</p></div><div className="english-audio-actions">{entry.britishAudio ? <button type="button" onClick={() => play(entry.britishAudio)} title={t("english.britishAudio")} aria-label={t("english.britishAudio")}><Headphones size={16} />UK</button> : null}{entry.americanAudio ? <button type="button" onClick={() => play(entry.americanAudio)} title={t("english.americanAudio")} aria-label={t("english.americanAudio")}><Headphones size={16} />US</button> : null}</div></div>
    {entry.translation ? <DetailSection title={t("english.translation")}><p>{entry.translation}</p></DetailSection> : null}
    {entry.example ? <DetailSection title={t("english.example")}><p>{entry.example}</p>{entry.exampleTranslation ? <p className="english-muted">{entry.exampleTranslation}</p> : null}</DetailSection> : null}
    {entry.englishDefinition ? <DetailSection title={t("english.definition")}><p>{entry.englishDefinition}</p></DetailSection> : null}
    {entry.mnemonic || entry.rootAffixes ? <DetailSection title={t("english.wordFormation")}>{entry.rootAffixes ? <p>{entry.rootAffixes}</p> : null}{entry.mnemonic ? <p className="english-muted">{entry.mnemonic}</p> : null}</DetailSection> : null}
    {entry.derivedWords.length > 0 ? <DetailSection title={t("english.derivedWords")}><p>{entry.derivedWords.map((item) => `${item.word} ${item.partOfSpeech} ${item.definition}`).join(" · ")}</p></DetailSection> : null}
    {entry.examExamples.length > 0 ? <DetailSection title={t("english.examExamples")}><div className="english-example-list">{entry.examExamples.slice(0, 10).map((item, index) => <div key={`${item.source}-${index}`}><p>{item.sentence}</p><small>{item.source} {item.section}</small></div>)}</div></DetailSection> : null}
  </section>;
}

function DetailSection({ title, children }: { title: string; children: React.ReactNode }) { return <div className="english-detail"><h3>{title}</h3>{children}</div>; }
