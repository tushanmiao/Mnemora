import { BookOpen, ChartNoAxesCombined, CircleAlert, Download, LoaderCircle, RefreshCw, Settings2, Sun, Trash2, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../../i18n/I18nProvider";
import {
  getEnglishDictionaryStatus,
  searchEnglishDictionary,
  type EnglishDictionaryStatus,
  type EnglishGroupSummary,
} from "../api/english";
import {
  archiveEnglishItem,
  addEnglishWordToPlan,
  createEnglishLearningPlan,
  clearEnglishAudioCache,
  defaultEnglishPlanSettings,
  exportEnglishWordBook,
  getEnglishAudioCacheStatus,
  getEnglishLearningBatch,
  getEnglishLearningOverview,
  getEnglishLearningStats,
  markEnglishItemMastered,
  importEnglishWordBook,
  prefetchEnglishAudio,
  restoreEnglishItem,
  updateEnglishLearningPlan,
  type EnglishAudioCacheStatus,
  type EnglishLearningOverview,
  type EnglishLearningStats,
  type EnglishPlanSettings,
  type EnglishQueueItem,
  type EnglishQueueMode,
} from "../api/learning";
import EnglishDictionary from "./EnglishDictionary";
import EnglishHome from "./EnglishHome";
import EnglishLearningSession from "./EnglishLearningSession";
import EnglishPlanSetup from "./EnglishPlanSetup";
import EnglishProgress from "./EnglishProgress";
import "../styles/english.css";

type Tab = "today" | "dictionary" | "progress" | "settings";

const emptyStats: EnglishLearningStats = { attempts7d: 0, correct7d: 0, hintUses7d: 0, averageResponseMs7d: 0, dueBacklog: 0, activeDays7d: 0, currentStreakDays: 0, skills: [] };

export default function EnglishView() {
  const { t } = useI18n();
  const [status, setStatus] = useState<EnglishDictionaryStatus | null>(null);
  const [overview, setOverview] = useState<EnglishLearningOverview | null>(null);
  const [stats, setStats] = useState<EnglishLearningStats>(emptyStats);
  const [groups, setGroups] = useState<EnglishGroupSummary[]>([]);
  const [tab, setTab] = useState<Tab>("today");
  const [queue, setQueue] = useState<EnglishQueueItem[]>([]);
  const [queueIndex, setQueueIndex] = useState(0);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const loadRequestRef = useRef(0);

  const refresh = async () => {
    const request = ++loadRequestRef.current;
    if (!status || !overview) setLoading(true);
    setError("");
    try {
      const [nextStatus, nextOverview] = await withTimeout(
        Promise.all([getEnglishDictionaryStatus(), getEnglishLearningOverview()]),
        12_000,
        "英语模块加载超时，请重试。",
      );
      if (request !== loadRequestRef.current) return;
      setStatus(nextStatus);
      setOverview(nextOverview);
      setLoading(false);
      void loadSecondaryData(request, nextStatus.installed && !nextOverview.activePlan);
    } catch (reason) {
      if (request !== loadRequestRef.current) return;
      setError(formatError(reason));
    } finally {
      if (request === loadRequestRef.current) setLoading(false);
    }
  };

  const loadSecondaryData = async (request: number, needsGroups: boolean) => {
    const [statsResult, groupsResult] = await Promise.allSettled([
      getEnglishLearningStats(),
      needsGroups ? searchEnglishDictionary("", null, 1) : Promise.resolve(null),
    ]);
    if (request !== loadRequestRef.current) return;
    if (statsResult.status === "fulfilled") setStats(statsResult.value);
    if (groupsResult.status === "fulfilled" && groupsResult.value) setGroups(groupsResult.value.groups);
    const failed = [statsResult, groupsResult].find((result) => result.status === "rejected");
    if (failed?.status === "rejected") setError(`部分英语数据加载失败：${formatError(failed.reason)}`);
  };

  useEffect(() => {
    void refresh();
    return () => { loadRequestRef.current += 1; };
  }, []);

  const createPlan = async (name: string, groupIds: number[], settings: EnglishPlanSettings) => {
    setBusy(true);
    setError("");
    try {
      await createEnglishLearningPlan({ name, groupIds, settings });
      const [nextOverview, nextStats] = await Promise.all([getEnglishLearningOverview(), getEnglishLearningStats()]);
      setOverview(nextOverview);
      setStats(nextStats);
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(false);
    }
  };

  const importBook = async () => {
    setBusy(true);
    setError("");
    try {
      const imported = await importEnglishWordBook();
      if (!imported) return;
      await refresh();
      setTab("today");
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(false);
    }
  };

  const exportBook = async () => {
    const plan = overview?.activePlan;
    if (!plan) return;
    setBusy(true);
    setError("");
    try {
      await exportEnglishWordBook(plan.bookName);
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(false);
    }
  };

  const startSession = async (mode: EnglishQueueMode) => {
    setBusy(true);
    setError("");
    try {
      const items = await getEnglishLearningBatch(mode);
      if (items.length === 0) {
        setError(mode === "review" ? "当前没有到期复习。" : "当前模式没有可练习的单词。");
        return;
      }
      setQueue(items);
      setQueueIndex(0);
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(false);
    }
  };

  const finishOrAdvance = async () => {
    if (queueIndex + 1 < queue.length) {
      setQueueIndex((index) => index + 1);
      return;
    }
    setQueue([]);
    setQueueIndex(0);
    const [nextOverview, nextStats] = await Promise.all([
      getEnglishLearningOverview(),
      getEnglishLearningStats(),
    ]);
    setOverview(nextOverview);
    setStats(nextStats);
  };

  if (loading && (!status || !overview)) {
    return <div className="english-view english-state" role="status"><LoaderCircle className="english-spinner" size={18} />{t("english.loading")}</div>;
  }

  if (!status || !overview) {
    return <div className="english-view english-load-failure" role="alert">
      <CircleAlert size={22} />
      <div><h2>英语模块暂时无法加载</h2><p>{error || "没有读取到词库或学习进度。"}</p></div>
      <button type="button" onClick={() => void refresh()}><RefreshCw size={16} />重试</button>
    </div>;
  }

  if (queue.length > 0 && overview.activePlan) {
    const current = queue[queueIndex];
    return <div className="english-view english-view-session"><EnglishLearningSession
      key={current.progressId}
      item={current}
      position={queueIndex}
      total={queue.length}
      settings={overview.activePlan.settings}
      onBack={() => { setQueue([]); setQueueIndex(0); }}
      onAdvance={() => void finishOrAdvance()}
      onCompleted={(result) => setOverview(result.overview)}
      onMastered={async () => { setOverview(await markEnglishItemMastered(current.progressId)); }}
      onArchive={async () => { setOverview(await archiveEnglishItem(current.progressId)); }}
    /></div>;
  }

  const showSetup = !overview.activePlan;
  return (
    <div className="english-view">
      <header className="english-header">
        <div><p className="english-eyebrow">Mnemora / English</p><h1>{t("english.title")}</h1><p>单词学习、主动回忆与到期复习</p></div>
        <button className="english-icon-button" type="button" onClick={() => void refresh()} title={t("english.refresh")} aria-label={t("english.refresh")}><RefreshCw size={16} /></button>
      </header>

      {!showSetup ? <nav className="english-tabs" aria-label="英语学习视图">
        <TabButton active={tab === "today"} onClick={() => setTab("today")} icon={<Sun size={16} />} label="今日" />
        <TabButton active={tab === "dictionary"} onClick={() => setTab("dictionary")} icon={<BookOpen size={16} />} label="词典" />
        <TabButton active={tab === "progress"} onClick={() => setTab("progress")} icon={<ChartNoAxesCombined size={16} />} label="进度" />
        <TabButton active={tab === "settings"} onClick={() => setTab("settings")} icon={<Settings2 size={16} />} label="设置" />
      </nav> : null}

      <div className="english-workspace">
        {showSetup && status.installed ? <EnglishPlanSetup groups={groups} busy={busy} onCreate={createPlan} onImport={importBook} /> : null}
        {showSetup && !status.installed ? <div className="english-setup-with-import">
          <EnglishDictionary status={status} onStatusChange={setStatus} onGroupsChange={setGroups} hasPlan={false} pageSize={20} onAddWord={async () => undefined} />
          <button className="english-secondary-button" type="button" disabled={busy} onClick={() => void importBook()}><Upload size={16} />不安装词典，导入自定义词书</button>
        </div> : null}
        {!showSetup && tab === "today" ? <EnglishHome overview={overview} busy={busy} onStart={(mode) => void startSession(mode)} onOpenSettings={() => setTab("settings")} /> : null}
        {!showSetup && tab === "dictionary" ? <EnglishDictionary status={status} onStatusChange={setStatus} onGroupsChange={setGroups} hasPlan={Boolean(overview.activePlan)} pageSize={overview.activePlan?.settings.dictionaryPageSize ?? 20} onAddWord={async (wordId) => { setOverview(await addEnglishWordToPlan(wordId)); }} /> : null}
        {!showSetup && tab === "progress" ? <EnglishProgress
          overview={overview}
          stats={stats}
          onRestore={async (progressId) => {
            setError("");
            try {
              const nextOverview = await restoreEnglishItem(progressId);
              setOverview(nextOverview);
            } catch (reason) {
              setError(formatError(reason));
              throw reason;
            }
          }}
        /> : null}
        {!showSetup && tab === "settings" && overview.activePlan ? <EnglishPlanSettingsPanel
          settings={overview.activePlan.settings}
          bookName={overview.activePlan.bookName}
          busy={busy}
          onExport={exportBook}
          onImport={importBook}
          onError={setError}
          onSave={async (settings) => {
            setBusy(true);
            try {
              await updateEnglishLearningPlan(overview.activePlan!.id, settings);
              setOverview(await getEnglishLearningOverview());
              setTab("today");
            } catch (reason) { setError(formatError(reason)); }
            finally { setBusy(false); }
          }}
        /> : null}
      </div>
      {error ? <p className="english-error" role="alert">{error}</p> : null}
    </div>
  );
}

function TabButton({ active, onClick, icon, label }: { active: boolean; onClick: () => void; icon: React.ReactNode; label: string }) {
  return <button type="button" className={active ? "is-active" : ""} onClick={onClick}>{icon}{label}</button>;
}

function EnglishPlanSettingsPanel({
  settings: initial,
  bookName,
  busy,
  onSave,
  onExport,
  onImport,
  onError,
}: {
  settings: EnglishPlanSettings;
  bookName: string;
  busy: boolean;
  onSave: (settings: EnglishPlanSettings) => Promise<void>;
  onExport: () => Promise<void>;
  onImport: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [settings, setSettings] = useState(initial ?? defaultEnglishPlanSettings);
  const [cacheStatus, setCacheStatus] = useState<EnglishAudioCacheStatus | null>(null);
  const [cacheBusy, setCacheBusy] = useState(false);
  useEffect(() => {
    void getEnglishAudioCacheStatus().then(setCacheStatus).catch((reason) => onError(formatError(reason)));
  }, []);
  const runCacheAction = async (action: "prefetch" | "clear") => {
    setCacheBusy(true);
    onError("");
    try {
      setCacheStatus(await (action === "prefetch" ? prefetchEnglishAudio() : clearEnglishAudioCache()));
    } catch (reason) {
      onError(formatError(reason));
    } finally {
      setCacheBusy(false);
    }
  };
  const toggleRestDay = (day: number) => {
    const restDays = settings.restDays.includes(day)
      ? settings.restDays.filter((value) => value !== day)
      : [...settings.restDays, day].sort((left, right) => left - right);
    setSettings({ ...settings, restDays });
  };
  return <section className="english-settings-panel">
    <div className="english-section-heading"><div><h2>学习设置</h2><p>每日复习是软目标；到期单词不会被自动丢弃。</p></div></div>
    <div className="english-plan-form">
      <SettingsNumber label="每组新词数" value={settings.newBatchSize} min={1} max={100} onChange={(value) => setSettings({ ...settings, newBatchSize: value })} />
      <SettingsNumber label="每日新词目标" value={settings.dailyNewTarget} min={1} max={500} onChange={(value) => setSettings({ ...settings, dailyNewTarget: value })} />
      <SettingsNumber label="每组复习数" value={settings.reviewBatchSize} min={1} max={100} onChange={(value) => setSettings({ ...settings, reviewBatchSize: value })} />
      <SettingsNumber label="每日复习软目标" value={settings.dailyReviewTarget} min={1} max={2000} onChange={(value) => setSettings({ ...settings, dailyReviewTarget: value })} />
      <label className="english-field"><span>目标保持率</span><select value={settings.desiredRetention} onChange={(event) => setSettings({ ...settings, desiredRetention: Number(event.target.value) })}><option value={0.85}>轻量 · 85%</option><option value={0.9}>标准 · 90%</option><option value={0.95}>强化 · 95%</option></select></label>
      <label className="english-field"><span>首选发音</span><select value={settings.preferredAccent} onChange={(event) => setSettings({ ...settings, preferredAccent: event.target.value as "british" | "american" })}><option value="british">英音</option><option value="american">美音</option></select></label>
      <label className="english-field"><span>播放速度</span><select value={settings.playbackRate} onChange={(event) => setSettings({ ...settings, playbackRate: Number(event.target.value) })}><option value={0.8}>0.8x</option><option value={1}>1.0x</option><option value={1.2}>1.2x</option></select></label>
      <SettingsNumber label="音频缓存上限（MB）" value={settings.audioCacheMaxMb} min={0} max={2048} onChange={(value) => setSettings({ ...settings, audioCacheMaxMb: value })} />
      <SettingsNumber label="预下载未来天数" value={settings.audioPrefetchDays} min={0} max={30} onChange={(value) => setSettings({ ...settings, audioPrefetchDays: value })} />
      <PageSizeField label="词典每页数量" value={settings.dictionaryPageSize} onChange={(value) => setSettings({ ...settings, dictionaryPageSize: value })} />
      <PageSizeField label="归档单词每页数量" value={settings.archivePageSize} onChange={(value) => setSettings({ ...settings, archivePageSize: value })} />
      <PageSizeField label="最近答题每页数量" value={settings.historyPageSize} onChange={(value) => setSettings({ ...settings, historyPageSize: value })} />
      <label className="english-toggle"><input type="checkbox" checked={settings.autoPlay} onChange={(event) => setSettings({ ...settings, autoPlay: event.target.checked })} /><span>听写时自动播放</span></label>
      <label className="english-toggle"><input type="checkbox" checked={settings.masteredAudits} onChange={(event) => setSettings({ ...settings, masteredAudits: event.target.checked })} /><span>启用已掌握抽查</span></label>
      <label className="english-toggle"><input type="checkbox" checked={settings.pauseNewWords} onChange={(event) => setSettings({ ...settings, pauseNewWords: event.target.checked })} /><span>暂停引入新词</span></label>
    </div>
    <div className="english-rest-days">
      <span>休息日（仍可复习到期单词，不引入新词）</span>
      <div>{["日", "一", "二", "三", "四", "五", "六"].map((label, day) => <button key={label} type="button" className={settings.restDays.includes(day) ? "is-selected" : ""} onClick={() => toggleRestDay(day)}>周{label}</button>)}</div>
    </div>
    <div className="english-settings-tools">
      <div><strong>自定义词书</strong><span>{bookName} · 可导出为开放 JSON 文件，并在其他设备重新导入。</span></div>
      <div className="english-settings-tool-actions">
        <button className="english-secondary-button" type="button" disabled={busy} onClick={() => void onExport()}><Download size={15} />导出词书</button>
        <button className="english-secondary-button" type="button" disabled={busy} onClick={() => { if (window.confirm("导入词书会暂停当前计划并切换到新计划，是否继续？")) void onImport(); }}><Upload size={15} />导入词书</button>
      </div>
    </div>
    <div className="english-settings-tools">
      <div><strong>音频缓存</strong><span>{cacheStatus ? `${cacheStatus.files} 个文件 · ${formatBytes(cacheStatus.bytes)} / ${formatBytes(cacheStatus.maxBytes)}` : "正在读取缓存状态"}</span></div>
      <div className="english-settings-tool-actions">
        <button className="english-secondary-button" type="button" disabled={cacheBusy || settings.audioCacheMaxMb === 0} onClick={() => void runCacheAction("prefetch")}><Download size={15} />预下载</button>
        <button className="english-secondary-button" type="button" disabled={cacheBusy || !cacheStatus?.files} onClick={() => void runCacheAction("clear")}><Trash2 size={15} />清理缓存</button>
      </div>
    </div>
    <div className="english-settings-save"><button type="button" disabled={busy} onClick={() => void onSave(settings)}>{busy ? "正在保存" : "保存设置"}</button></div>
  </section>;
}

function SettingsNumber({ label, value, min, max, onChange }: { label: string; value: number; min: number; max: number; onChange: (value: number) => void }) {
  return <label className="english-field"><span>{label}</span><input type="number" value={value} min={min} max={max} onChange={(event) => onChange(Math.min(max, Math.max(min, Number(event.target.value) || min)))} /></label>;
}

function PageSizeField({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return <label className="english-field"><span>{label}</span><select value={value} onChange={(event) => onChange(Number(event.target.value))}><option value={20}>20 个</option><option value={40}>40 个</option></select></label>;
}

function formatError(error: unknown) { return error instanceof Error ? error.message : String(error); }
function formatBytes(bytes: number) { return bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timer = 0;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timer = window.setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
    ]);
  } finally {
    window.clearTimeout(timer);
  }
}
