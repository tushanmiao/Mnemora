import { Archive, ChevronDown, ChevronLeft, ChevronRight, ChevronUp, RotateCcw } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  listArchivedEnglishItems,
  listEnglishAttemptHistory,
  type EnglishArchivedItem,
  type EnglishAttemptHistoryItem,
  type EnglishLearningOverview,
  type EnglishLearningStats,
} from "../api/learning";

export default function EnglishProgress({ overview, stats, onRestore }: {
  overview: EnglishLearningOverview;
  stats: EnglishLearningStats;
  onRestore: (progressId: string) => Promise<void>;
}) {
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [archiveItems, setArchiveItems] = useState<EnglishArchivedItem[]>([]);
  const [archivePage, setArchivePage] = useState(0);
  const [archiveLoading, setArchiveLoading] = useState(false);
  const [archiveError, setArchiveError] = useState("");
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [historyItems, setHistoryItems] = useState<EnglishAttemptHistoryItem[]>([]);
  const [historyTotal, setHistoryTotal] = useState(0);
  const [historyPage, setHistoryPage] = useState(0);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState("");
  const historyRequestRef = useRef(0);
  const archivePageSize = overview.activePlan?.settings.archivePageSize ?? 20;
  const historyPageSize = overview.activePlan?.settings.historyPageSize ?? 20;
  const totalWords = overview.activePlan?.itemCount ?? 0;
  const learnedWords = Math.min(totalWords, overview.learnedCount);
  const masteredWords = Math.min(learnedWords, overview.masteredCount);
  const activeWords = Math.max(0, learnedWords - masteredWords);
  const archivedWords = Math.min(Math.max(0, totalWords - learnedWords), overview.archivedCount);
  const newWords = Math.max(0, totalWords - learnedWords - archivedWords);
  const completion = totalWords > 0 ? learnedWords / totalWords * 100 : 0;
  const completionLabel = completion > 0 && completion < 10 ? completion.toFixed(1) : Math.round(completion).toString();
  const masteredAngle = totalWords > 0 ? masteredWords / totalWords * 360 : 0;
  const learnedAngle = totalWords > 0 ? learnedWords / totalWords * 360 : 0;
  const archivedAngle = totalWords > 0 ? (learnedWords + archivedWords) / totalWords * 360 : 0;
  const ringStyle = {
    "--english-mastered-angle": `${masteredAngle}deg`,
    "--english-learned-angle": `${learnedAngle}deg`,
    "--english-archived-angle": `${archivedAngle}deg`,
  } as CSSProperties;
  const accuracy = stats.attempts7d > 0 ? Math.round(stats.correct7d / stats.attempts7d * 100) : 0;
  const hintRate = stats.attempts7d > 0 ? Math.round(stats.hintUses7d / stats.attempts7d * 100) : 0;
  const archivePages = Math.max(1, Math.ceil(overview.archivedCount / archivePageSize));
  const historyPages = Math.max(1, Math.ceil(historyTotal / historyPageSize));

  const loadArchivePage = async (requestedPage: number, total = overview.archivedCount) => {
    const pageCount = Math.max(1, Math.ceil(total / archivePageSize));
    const nextPage = Math.max(0, Math.min(requestedPage, pageCount - 1));
    setArchiveLoading(true);
    setArchiveError("");
    try {
      const items = await listArchivedEnglishItems(archivePageSize, nextPage * archivePageSize);
      setArchiveItems(items);
      setArchivePage(nextPage);
    } catch (reason) {
      setArchiveError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setArchiveLoading(false);
    }
  };

  useEffect(() => {
    setArchivePage(0);
    setArchiveItems([]);
    if (archiveOpen) void loadArchivePage(0);
  }, [archivePageSize]);

  const loadHistoryPage = async (requestedPage: number) => {
    const request = ++historyRequestRef.current;
    const page = Math.max(0, requestedPage);
    setHistoryLoading(true);
    setHistoryError("");
    try {
      let result = await listEnglishAttemptHistory(historyPageSize, page * historyPageSize);
      const pageCount = Math.max(1, Math.ceil(result.total / historyPageSize));
      const nextPage = Math.min(page, pageCount - 1);
      if (nextPage !== page) {
        result = await listEnglishAttemptHistory(historyPageSize, nextPage * historyPageSize);
      }
      if (request !== historyRequestRef.current) return;
      setHistoryItems(result.items);
      setHistoryTotal(result.total);
      setHistoryPage(nextPage);
    } catch (reason) {
      if (request === historyRequestRef.current) {
        setHistoryError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      if (request === historyRequestRef.current) setHistoryLoading(false);
    }
  };

  useEffect(() => {
    void loadHistoryPage(0);
    return () => { historyRequestRef.current += 1; };
  }, [historyPageSize]);

  const toggleArchive = async () => {
    if (archiveOpen) {
      setArchiveOpen(false);
      return;
    }
    setArchiveOpen(true);
    await loadArchivePage(0);
  };

  const restore = async (progressId: string) => {
    setRestoringId(progressId);
    setArchiveError("");
    try {
      await onRestore(progressId);
      const nextTotal = Math.max(0, overview.archivedCount - 1);
      await loadArchivePage(archivePage, nextTotal);
    } catch (reason) {
      setArchiveError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setRestoringId(null);
    }
  };

  return (
    <div className="english-progress-page">
      <section className="english-progress-overview">
        <div className="english-completion-ring" style={ringStyle} role="img" aria-label={`词书学习进度 ${completionLabel}%`}>
          <div><strong>{completionLabel}%</strong><span>词书进度</span></div>
        </div>
        <div className="english-progress-overview-copy">
          <span className="english-progress-kicker">{overview.activePlan?.bookName ?? "当前词书"}</span>
          <h2>已学习 {learnedWords.toLocaleString()} / {totalWords.toLocaleString()}</h2>
          <p>已进入学习或复习流程的单词计入完成进度。</p>
          <div className="english-progress-legend">
            <span><i className="is-mastered" />已掌握 <strong>{masteredWords.toLocaleString()}</strong></span>
            <span><i className="is-learning" />学习中 <strong>{activeWords.toLocaleString()}</strong></span>
            <span><i className="is-archived" />已归档 <strong>{archivedWords.toLocaleString()}</strong></span>
            <span><i className="is-new" />未学习 <strong>{newWords.toLocaleString()}</strong></span>
          </div>
        </div>
      </section>

      <section>
        <div className="english-section-heading"><div><h2>近 7 日</h2><p>只展示可由本地答题记录推导的数据。</p></div></div>
        <div className="english-metrics english-progress-metrics">
          <ProgressMetric tone="blue" label="完成练习" value={stats.attempts7d.toLocaleString()} />
          <ProgressMetric tone="green" label="实际正确率" value={`${accuracy}%`} />
          <ProgressMetric tone="amber" label="提示使用率" value={`${hintRate}%`} />
          <ProgressMetric tone="coral" label="平均响应" value={stats.averageResponseMs7d ? `${(stats.averageResponseMs7d / 1000).toFixed(1)}s` : "-"} />
          <ProgressMetric tone="cyan" label="活跃天数" value={`${stats.activeDays7d} / 7`} />
          <ProgressMetric tone="violet" label="连续学习" value={`${stats.currentStreakDays} 天`} />
        </div>
      </section>

      <section className="english-skill-section">
        <div className="english-section-heading"><div><h2>能力维度</h2><p>错误率用于决定下次复习题型，不会制造多套到期卡。</p></div></div>
        <div className="english-skill-table">
          <div className="is-heading"><span>维度</span><span>练习</span><span>正确率</span><span>提示</span><span>平均响应</span></div>
          {stats.skills.map((skill) => <div key={skill.skill}><strong>{skillLabel(skill.skill)}</strong><span>{skill.attempts}</span><span>{skill.attempts ? `${Math.round(skill.correct / skill.attempts * 100)}%` : "-"}</span><span>{skill.hintUses}</span><span>{skill.averageResponseMs ? `${(skill.averageResponseMs / 1000).toFixed(1)}s` : "-"}</span></div>)}
          {stats.skills.length === 0 ? <p>完成第一组主动回忆后，这里会出现技能统计。</p> : null}
        </div>
      </section>

      <section className="english-archived-section">
        <div className="english-section-heading">
          <div><h2>已归档单词</h2><p>共 {overview.archivedCount.toLocaleString()} 个，按每页 {archivePageSize} 个加载。</p></div>
          <button className="english-archive-toggle" type="button" disabled={overview.archivedCount === 0 || archiveLoading} onClick={() => void toggleArchive()}>
            <Archive size={15} />{archiveOpen ? "收起" : "查看"}{archiveOpen ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
          </button>
        </div>
        {archiveOpen ? <>
          {archiveLoading && archiveItems.length === 0 ? <p className="english-inline-state" role="status">正在读取归档单词</p> : null}
          {archiveError ? <p className="english-error" role="alert">{archiveError}</p> : null}
          {archiveItems.length > 0 ? <div className="english-archived-table">
            <div className="is-heading"><span>单词</span><span>释义</span><span>归档前状态</span><span>归档时间</span><span>操作</span></div>
            {archiveItems.map((item) => <div key={item.progressId}>
              <strong>{item.word}</strong>
              <span title={item.translation}>{item.translation}</span>
              <span>{progressStateLabel(item.previousState)}</span>
              <span>{new Date(item.archivedAt).toLocaleString()}</span>
              <button type="button" disabled={restoringId !== null || archiveLoading} onClick={() => void restore(item.progressId)}><RotateCcw size={14} />{restoringId === item.progressId ? "恢复中" : "恢复"}</button>
            </div>)}
          </div> : null}
          {archiveItems.length === 0 && !archiveLoading && !archiveError ? <p className="english-archive-empty">当前没有归档单词。</p> : null}
          {overview.archivedCount > archivePageSize ? <div className="english-table-pagination">
            <button type="button" title="上一页" aria-label="上一页" disabled={archivePage === 0 || archiveLoading} onClick={() => void loadArchivePage(archivePage - 1)}><ChevronLeft size={16} /></button>
            <span>第 {archivePage + 1} / {archivePages} 页</span>
            <button type="button" title="下一页" aria-label="下一页" disabled={archivePage + 1 >= archivePages || archiveLoading} onClick={() => void loadArchivePage(archivePage + 1)}><ChevronRight size={16} /></button>
          </div> : null}
        </> : null}
      </section>

      <section className="english-history-section">
        <div className="english-section-heading"><div><h2>最近答题</h2><p>数据库最多保留最近 1,000 条；共 {historyTotal.toLocaleString()} 条，每页 {historyPageSize} 条。</p></div></div>
        <div className="english-history-table">
          <div className="is-heading"><span>单词</span><span>题型</span><span>原始答案</span><span>判定</span><span>建议 / 最终</span><span>时间</span></div>
          {historyItems.map((attempt) => <div key={attempt.id}>
            <strong>{attempt.word}</strong>
            <span>{exerciseLabel(attempt.exerciseKind)}</span>
            <span title={attempt.rawAnswer}>{attempt.rawAnswer || "-"}</span>
            <span>{verdictLabel(attempt.verdict)}{attempt.hintCount > 0 ? ` · 提示 ${attempt.hintCount}` : ""}</span>
            <span>{ratingLabel(attempt.suggestedRating)} / {ratingLabel(attempt.finalRating)}</span>
            <span>{new Date(attempt.reviewedAt).toLocaleString()}</span>
          </div>)}
          {historyLoading && historyItems.length === 0 ? <p role="status">正在读取答题记录。</p> : null}
          {historyItems.length === 0 && !historyLoading && !historyError ? <p>还没有答题记录。</p> : null}
        </div>
        {historyError ? <p className="english-error" role="alert">{historyError}</p> : null}
        {historyTotal > historyPageSize ? <div className="english-table-pagination">
          <button type="button" title="上一页" aria-label="上一页" disabled={historyPage === 0 || historyLoading} onClick={() => void loadHistoryPage(historyPage - 1)}><ChevronLeft size={16} /></button>
          <span>第 {historyPage + 1} / {historyPages} 页</span>
          <button type="button" title="下一页" aria-label="下一页" disabled={historyPage + 1 >= historyPages || historyLoading} onClick={() => void loadHistoryPage(historyPage + 1)}><ChevronRight size={16} /></button>
        </div> : null}
      </section>
    </div>
  );
}

function ProgressMetric({ label, value, tone }: { label: string; value: string; tone: "blue" | "green" | "amber" | "coral" | "cyan" | "violet" }) {
  return <div className={`english-metric is-${tone}`}><span>{label}</span><strong>{value}</strong></div>;
}

function skillLabel(skill: string) {
  return ({ meaning: "释义回忆", spelling: "拼写", listening: "听辨" } as Record<string, string>)[skill] ?? skill;
}

function exerciseLabel(kind: EnglishAttemptHistoryItem["exerciseKind"]) { return ({ meaning_recall: "释义", spelling: "拼写", dictation: "听写" } as const)[kind]; }
function verdictLabel(verdict: EnglishAttemptHistoryItem["verdict"]) { return ({ correct: "正确", acceptable: "可接受", incorrect: "错误", skipped: "跳过" } as const)[verdict]; }
function ratingLabel(rating: EnglishAttemptHistoryItem["finalRating"]) { return ({ again: "忘记", hard: "困难", good: "记得", easy: "简单" } as const)[rating]; }
function progressStateLabel(state: EnglishArchivedItem["previousState"]) {
  return ({ new: "未学习", learning: "学习中", review: "复习中", relearning: "重新学习", mastered: "已掌握", archived: "已归档" } as const)[state] ?? state;
}
