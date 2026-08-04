import { CalendarClock, Headphones, Keyboard, Play, RotateCcw, Settings2, TriangleAlert } from "lucide-react";
import type { EnglishLearningOverview, EnglishQueueMode } from "../api/learning";

type Props = {
  overview: EnglishLearningOverview;
  busy: boolean;
  onStart: (mode: EnglishQueueMode) => void;
  onOpenSettings: () => void;
};

export default function EnglishHome({ overview, busy, onStart, onOpenSettings }: Props) {
  const plan = overview.activePlan;
  if (!plan) return null;
  const newRemaining = overview.isRestDay ? 0 : Math.max(0, plan.settings.dailyNewTarget - overview.todayNewDone);
  const reviewProgress = Math.min(100, Math.round(overview.todayReviewDone / Math.max(1, plan.settings.dailyReviewTarget) * 100));
  const newProgress = Math.min(100, Math.round(overview.todayNewDone / Math.max(1, plan.settings.dailyNewTarget) * 100));

  return (
    <div className="english-dashboard">
      <section className="english-today-band">
        <div className="english-section-heading">
          <div>
            <h2>今天</h2>
            <p>{plan.bookName} · 已学习 {overview.learnedCount.toLocaleString()} / {plan.itemCount.toLocaleString()}{overview.isRestDay ? " · 今天是休息日" : overview.estimatedCompletionAt ? ` · 预计 ${new Date(overview.estimatedCompletionAt).toLocaleDateString()}` : ""}</p>
          </div>
          <button className="english-icon-button" type="button" onClick={onOpenSettings} title="计划设置" aria-label="计划设置"><Settings2 size={16} /></button>
        </div>
        <div className="english-metrics">
          <Metric label="到期复习" value={overview.dueCount} detail={overview.overdueCount > 0 ? `${overview.overdueCount} 个已逾期` : "无逾期"} tone={overview.overdueCount > 0 ? "warning" : "normal"} />
          <Metric label="今日复习" value={overview.todayReviewDone} detail={`软目标 ${plan.settings.dailyReviewTarget}`} progress={reviewProgress} />
          <Metric label="今日新词" value={overview.todayNewDone} detail={`还可学习 ${newRemaining}`} progress={newProgress} />
          <Metric label="已掌握" value={overview.masteredCount} detail={overview.masteredDueCount > 0 ? `${overview.masteredDueCount} 个待抽查` : "暂无抽查"} />
        </div>
      </section>

      <section className="english-actions-section">
        <div className="english-section-heading"><div><h2>开始学习</h2><p>到期复习始终优先，新词按每日额度引入。</p></div></div>
        <div className="english-primary-actions">
          <button type="button" className="is-primary" disabled={busy || overview.dueCount === 0} onClick={() => onStart("review")}><RotateCcw size={18} /><span><strong>开始复习</strong><small>{overview.dueCount} 个到期</small></span></button>
          <button type="button" disabled={busy || newRemaining === 0 || plan.settings.pauseNewWords || overview.newAvailable === 0} onClick={() => onStart("new")}><Play size={18} /><span><strong>学习新词</strong><small>下一组 {Math.min(plan.settings.newBatchSize, newRemaining)}</small></span></button>
          <button type="button" disabled={busy || (overview.dueCount === 0 && newRemaining === 0)} onClick={() => onStart("mixed")}><CalendarClock size={18} /><span><strong>继续今日队列</strong><small>复习优先</small></span></button>
        </div>
        {overview.dueCount > plan.settings.dailyReviewTarget ? <p className="english-soft-notice"><TriangleAlert size={15} />达到软目标后仍有到期单词时，可以继续清理积压，也可以稍后再完成。</p> : null}
        {overview.isRestDay ? <p className="english-soft-notice"><CalendarClock size={15} />休息日不会引入新词；到期复习仍可正常完成，连续学习记录不会受到惩罚。</p> : null}
      </section>

      <section className="english-actions-section">
        <div className="english-section-heading"><div><h2>专项练习</h2><p>{overview.weakSkill ? `当前薄弱项：${skillLabel(overview.weakSkill)}` : "完成练习后会根据真实答题记录识别薄弱项。"}</p></div></div>
        <div className="english-secondary-actions">
          <button type="button" disabled={busy || overview.learnedCount === 0} onClick={() => onStart("dictation")}><Headphones size={17} />单词听写</button>
          <button type="button" disabled={busy || overview.learnedCount === 0} onClick={() => onStart("spelling")}><Keyboard size={17} />拼写强化</button>
          <button type="button" disabled={busy || overview.learnedCount === 0} onClick={() => onStart("mistakes")}><RotateCcw size={17} />错词复盘</button>
          <button type="button" disabled={busy || overview.masteredDueCount === 0} onClick={() => onStart("mastered")}><CalendarClock size={17} />已掌握抽查</button>
        </div>
      </section>
    </div>
  );
}

function Metric({ label, value, detail, progress, tone = "normal" }: { label: string; value: number; detail: string; progress?: number; tone?: "normal" | "warning" }) {
  return <div className={`english-metric is-${tone}`}><span>{label}</span><strong>{value.toLocaleString()}</strong><small>{detail}</small>{progress !== undefined ? <div><i style={{ width: `${progress}%` }} /></div> : null}</div>;
}

function skillLabel(skill: string) {
  return ({ meaning: "释义回忆", spelling: "拼写", listening: "听辨" } as Record<string, string>)[skill] ?? skill;
}
