import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  BookOpen,
  BrainCircuit,
  Check,
  CheckCircle2,
  CircleDot,
  Clock3,
  FileText,
  Gauge,
  LoaderCircle,
  Network,
  PauseCircle,
  RotateCcw,
  ShieldCheck,
  Square,
  XCircle,
} from "lucide-react";
import type {
  DeepNoteSectionProgress,
  DeepNoteSectionStatus,
  NotePipelinePhase,
} from "../../chat/api/notePipeline";
import type { DeepNoteSection } from "../../chat/notePipeline/outlineSchema";
import { useDeepNoteViewRuntime } from "../runtime/DeepNoteViewRuntime";
import "../styles/deep-note-workspace.css";

const PHASE_LABELS: Record<NotePipelinePhase, string> = {
  preflight: "检查输入",
  analyzing: "分析知识结构",
  awaitingOutline: "等待计划确认",
  compiling: "编译执行计划",
  queued: "等待执行",
  drafting: "扩写章节",
  validating: "验证章节",
  replanning: "调整计划",
  assembling: "组装笔记",
  persisting: "保存笔记",
  paused: "已暂停",
  blocked: "等待处理",
  done: "已完成",
  cancelled: "已取消",
  error: "生成失败",
};

const SECTION_STATUS_LABELS: Record<DeepNoteSectionStatus, string> = {
  pending: "等待",
  ready: "就绪",
  inProgress: "生成中",
  completed: "已完成",
  needsReview: "待检查",
  needsRevision: "待修订",
  failed: "失败",
  blocked: "已阻塞",
  skipped: "已跳过",
  interrupted: "已中断",
};

const TERMINAL_PHASES = new Set<NotePipelinePhase>(["done", "cancelled", "error"]);

function phaseTone(phase: NotePipelinePhase): "active" | "success" | "warning" | "danger" | "muted" {
  if (phase === "done") return "success";
  if (phase === "error") return "danger";
  if (phase === "cancelled" || phase === "paused" || phase === "blocked") return "warning";
  if (phase === "awaitingOutline") return "muted";
  return "active";
}

function statusIcon(phase: NotePipelinePhase): ReactNode {
  if (phase === "done") return <CheckCircle2 size={18} />;
  if (phase === "error") return <XCircle size={18} />;
  if (phase === "cancelled" || phase === "paused") return <PauseCircle size={18} />;
  if (phase === "blocked") return <AlertTriangle size={18} />;
  if (phase === "awaitingOutline") return <Clock3 size={18} />;
  return <LoaderCircle size={18} className="deep-note-progress-spinner" />;
}

function formatUpdatedAt(value: number | undefined): string {
  if (!value) return "尚无更新时间";
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(value));
}

function SectionItem({
  section,
  selected,
  disabled,
  onChange,
}: {
  section: DeepNoteSection;
  selected: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="deep-note-plan-section">
      <input
        type="checkbox"
        checked={selected}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="deep-note-plan-section-copy">
        <strong>{section.heading}</strong>
        <small>{section.purpose || section.brief}</small>
        <span className="deep-note-plan-meta">
          {(section.dependsOn?.length ?? 0) > 0 ? `依赖 ${section.dependsOn?.length} 项` : "可独立执行"}
          <i>{section.successCriteria?.length ?? 0} 条成功标准</i>
          {section.allowAiSupplement ? <i>允许 AI 补充</i> : null}
        </span>
      </span>
    </label>
  );
}

function SectionProgressItem({
  section,
  heading,
}: {
  section: DeepNoteSectionProgress;
  heading: string;
}) {
  return (
    <div className="deep-note-section-progress" data-status={section.status}>
      <span className="deep-note-section-progress-dot" />
      <span className="deep-note-section-progress-copy">
        <strong>{heading}</strong>
        <small>
          {SECTION_STATUS_LABELS[section.status]}
          {section.attemptCount > 0 ? ` · 尝试 ${section.attemptCount} 次` : ""}
          {section.revisionCount > 0 ? ` · 修订 ${section.revisionCount} 次` : ""}
        </small>
        {section.errorMessage ? <em>{section.errorMessage}</em> : null}
      </span>
    </div>
  );
}

export default function DeepNoteView() {
  const runtime = useDeepNoteViewRuntime();
  const outline = runtime.review?.outline ?? runtime.detail?.planVersion?.plan ?? null;
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [requirement, setRequirement] = useState("");
  const [clock, setClock] = useState(Date.now());

  useEffect(() => {
    setSelected(new Set(outline?.sections.map((section) => section.id) ?? []));
  }, [outline]);

  useEffect(() => {
    if (!runtime.progress?.activity || runtime.progress.terminal) return undefined;
    setClock(Date.now());
    const timer = window.setInterval(() => setClock(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [runtime.progress?.activity, runtime.progress?.terminal]);

  const phase = runtime.progress?.phase
    ?? runtime.detail?.run.phase
    ?? (runtime.review ? "awaitingOutline" : "preflight");
  const tone = phaseTone(phase);
  const terminal = runtime.progress?.terminal || TERMINAL_PHASES.has(phase);
  const sections = runtime.detail?.sections ?? [];
  const completedSections = sections.filter((section) => section.status === "completed").length;
  const failedSections = sections.filter((section) => section.status === "failed").length;
  const activeSections = sections.filter((section) => section.status === "inProgress").length;
  const sectionProgressActive = phase === "drafting" || phase === "validating" || phase === "replanning";
  const totalSections = (sectionProgressActive ? runtime.progress?.total : null)
    ?? runtime.detail?.run.selectedSectionIds.length
    ?? outline?.sections.length
    ?? 0;
  const processedSections = (sectionProgressActive ? runtime.progress?.current : null)
    ?? completedSections + failedSections;
  const taskProgress = totalSections > 0
    ? Math.min(100, Math.round((processedSections / totalSections) * 100))
    : 0;
  const budget = runtime.detail?.budget;
  const contextBudget = runtime.detail?.contextBudget;
  const contextCoverage = contextBudget && contextBudget.totalMessageCount > 0
    ? Math.min(100, Math.round(
        (contextBudget.processedMessageCount / contextBudget.totalMessageCount) * 100,
      ))
    : 0;
  const budgetProgress = budget && budget.semanticCallLimit > 0
    ? Math.min(100, Math.round((budget.semanticCallsUsed / budget.semanticCallLimit) * 100))
    : 0;
  const nodes = runtime.detail?.nodes ?? [];
  const completedNodes = useMemo(
    () => nodes.filter((node) => node.status === "completed").length,
    [nodes],
  );
  const sectionHeadings = useMemo(() => new Map(
    outline?.sections.map((section) => [section.id, section.heading]) ?? [],
  ), [outline]);
  const statusMessage = runtime.progress?.message
    ?? runtime.detail?.run.errorMessage
    ?? (runtime.review ? "计划已生成，等待确认后开始执行。" : "正在检查输入与模型上下文预算…");
  const activity = runtime.progress?.activity;
  const activityElapsed = activity
    ? Math.max(0, Math.floor((clock - activity.startedAt) / 1_000))
    : null;

  return (
    <section className="deep-note-workspace" aria-label="深度笔记工作区">
      <header className="deep-note-workspace-header">
        <button className="icon-button" type="button" title="返回" onClick={runtime.onReturn}>
          <ArrowLeft size={18} />
        </button>
        <div className="deep-note-workspace-title">
          <span><BrainCircuit size={15} />Plan-and-Execute</span>
          <h1>{outline?.title ?? "深度笔记"}</h1>
        </div>
        <div className="deep-note-run-status" data-tone={tone}>
          <span className="deep-note-status-dot" />
          {PHASE_LABELS[phase]}
        </div>
        {!terminal ? (
          <button className="settings-button settings-button-secondary" type="button" onClick={runtime.onCancel}>
            <Square size={14} />停止
          </button>
        ) : runtime.detail?.run.noteId ? (
          <button className="settings-button settings-button-primary" type="button" onClick={runtime.onOpenNote}>
            <BookOpen size={14} />打开笔记库
          </button>
        ) : null}
      </header>

      <aside className="deep-note-source-pane">
        <div className="deep-note-pane-heading"><FileText size={15} /><strong>输入快照</strong></div>
        <dl className="deep-note-stat-list">
          <div><dt>消息</dt><dd>{runtime.detail?.inputSnapshot?.messageIds.length ?? 0}</dd></div>
          <div><dt>附件</dt><dd>{runtime.detail?.inputSnapshot?.attachmentIds.length ?? 0}</dd></div>
          <div><dt>来源分块</dt><dd>{runtime.detail?.sourceChunkCount ?? 0}</dd></div>
          <div><dt>计划版本</dt><dd>v{runtime.detail?.planVersion?.version ?? 1}</dd></div>
        </dl>
        <div className="deep-note-pane-heading"><ShieldCheck size={15} /><strong>能力预检</strong></div>
        <ul className="deep-note-check-list">
          <li><Check size={13} />文本生成可用</li>
          <li className={runtime.detail?.preflight?.model.capabilities.tools ? "" : "is-muted"}>
            <CircleDot size={13} />Tool {runtime.detail?.preflight?.model.capabilities.tools ? "可用" : "未启用"}
          </li>
          <li className={runtime.detail?.preflight?.requiresVision && runtime.detail.preflight.model.capabilities.vision !== true ? "is-warning" : ""}>
            <CircleDot size={13} />视觉 {runtime.detail?.preflight?.model.capabilities.vision === true ? "可用" : "未要求"}
          </li>
        </ul>
        {runtime.detail?.preflight?.warnings.map((warning) => (
          <p className="deep-note-inline-warning" key={warning}><AlertTriangle size={13} />{warning}</p>
        ))}
      </aside>

      <main className="deep-note-main-pane">
        <section className="deep-note-progress-overview" data-tone={tone} aria-live="polite">
          <span className="deep-note-progress-icon">{statusIcon(phase)}</span>
          <div className="deep-note-progress-copy">
            <div>
              <strong>{PHASE_LABELS[phase]}</strong>
              <time>更新于 {formatUpdatedAt(runtime.progress?.updatedAt ?? runtime.detail?.run.updatedAt)}</time>
            </div>
            <p>{statusMessage}</p>
            {activity ? (
              <div className="deep-note-live-activity">
                <span>请求 {activity.attempt}/{activity.maxRetries + 1}</span>
                <span>已重试 {Math.max(0, activity.attempt - 1)}/{activity.maxRetries}</span>
                <span>等待 {activityElapsed ?? 0} 秒</span>
                {activity.delayMs ? <span>{Math.ceil(activity.delayMs / 1_000)} 秒后重试</span> : null}
                {activity.lastError ? <em>{activity.lastError}</em> : null}
              </div>
            ) : null}
            <div className="deep-note-task-meter" aria-label={`任务进度 ${taskProgress}%`}>
              <span style={{ width: `${taskProgress}%` }} />
            </div>
            <dl className="deep-note-progress-stats">
              <div><dt>已完成</dt><dd>{completedSections}</dd></div>
              <div><dt>生成中</dt><dd>{activeSections}</dd></div>
              <div><dt>失败</dt><dd>{failedSections}</dd></div>
              <div><dt>总章节</dt><dd>{totalSections || "-"}</dd></div>
            </dl>
          </div>
        </section>

        {runtime.detail?.run.errorMessage ? (
          <div className="deep-note-terminal-message is-error">
            <XCircle size={16} />
            <div><strong>任务未能完成</strong><p>{runtime.detail.run.errorMessage}</p></div>
          </div>
        ) : null}
        {runtime.progress?.degraded ? (
          <div className="deep-note-terminal-message is-warning">
            <AlertTriangle size={16} />
            <div><strong>已保存部分结果</strong><p>停止前已完成的章节已经写入笔记库，未完成章节没有被伪装为成功。</p></div>
          </div>
        ) : null}
        {(runtime.detail?.run.warnings.length ?? 0) > 0 ? (
          <div className="deep-note-warning-list">
            {runtime.detail?.run.warnings.map((warning) => <p key={warning}><AlertTriangle size={14} />{warning}</p>)}
          </div>
        ) : null}

        {outline ? (
          <>
            <div className="deep-note-plan-intro">
              <div>
                <span>目标</span>
                <p>{outline.goal || outline.summary || "根据当前输入建立可验证、可复习的深度笔记。"}</p>
              </div>
              <div>
                <span>读者与范围</span>
                <p>{[outline.audience, outline.scope].filter(Boolean).join(" · ") || "沿用当前对话语境"}</p>
              </div>
            </div>
            <div className="deep-note-section-toolbar">
              <strong>语义计划</strong>
              <span>{selected.size}/{outline.sections.length} 个章节</span>
            </div>
            <div className="deep-note-plan-list">
              {outline.sections.map((section) => (
                <SectionItem
                  key={section.id}
                  section={section}
                  selected={selected.has(section.id)}
                  disabled={runtime.busy || !runtime.review}
                  onChange={(checked) => setSelected((current) => {
                    const next = new Set(current);
                    if (checked) next.add(section.id);
                    else next.delete(section.id);
                    return next;
                  })}
                />
              ))}
            </div>
            {runtime.review ? (
              <div className="deep-note-plan-actions">
                <label>
                  <span>调整计划要求</span>
                  <textarea
                    value={requirement}
                    disabled={runtime.busy}
                    placeholder="只描述会实质改变章节、证据或范围的要求"
                    onChange={(event) => setRequirement(event.target.value)}
                  />
                </label>
                <div>
                  <button
                    className="settings-button settings-button-secondary"
                    type="button"
                    disabled={runtime.busy || !requirement.trim()}
                    onClick={() => runtime.onAdjust(requirement.trim())}
                  >
                    <RotateCcw size={14} />重新规划
                  </button>
                  <button
                    className="settings-button settings-button-primary"
                    type="button"
                    disabled={runtime.busy || selected.size === 0}
                    onClick={() => runtime.onConfirm(selected)}
                  >
                    <Check size={14} />确认计划并执行
                  </button>
                </div>
              </div>
            ) : null}
            {runtime.detail?.markdownPreview ? (
              <article className="deep-note-preview">
                <div className="deep-note-section-toolbar"><strong>已完成内容预览</strong></div>
                <pre>{runtime.detail.markdownPreview}</pre>
              </article>
            ) : null}
          </>
        ) : (
          <div className="deep-note-empty">
            {statusIcon(phase)}
            <strong>{PHASE_LABELS[phase]}</strong>
            <span>{statusMessage}</span>
          </div>
        )}
      </main>

      <aside className="deep-note-run-pane">
        <div className="deep-note-pane-heading"><Gauge size={15} /><strong>任务进度</strong></div>
        <div className="deep-note-budget-meter is-task"><span style={{ width: `${taskProgress}%` }} /></div>
        <p>{processedSections} / {totalSections || 0} 个章节已处理</p>
        <dl className="deep-note-stat-list">
          <div><dt>已完成</dt><dd>{completedSections}</dd></div>
          <div><dt>失败</dt><dd>{failedSections}</dd></div>
          <div><dt>剩余</dt><dd>{Math.max(0, totalSections - completedSections - failedSections)}</dd></div>
        </dl>

        {sections.length > 0 ? (
          <>
            <div className="deep-note-pane-heading"><Clock3 size={15} /><strong>章节状态</strong></div>
            <div className="deep-note-section-progress-list">
              {sections.map((section) => (
                <SectionProgressItem
                  key={section.sectionId}
                  section={section}
                  heading={sectionHeadings.get(section.sectionId) ?? `章节 ${section.position + 1}`}
                />
              ))}
            </div>
          </>
        ) : null}

        <div className="deep-note-pane-heading"><Gauge size={15} /><strong>运行预算</strong></div>
        <div className="deep-note-budget-meter"><span style={{ width: `${budgetProgress}%` }} /></div>
        <p>{budget?.semanticCallsUsed ?? 0} / {budget?.semanticCallLimit ?? 0} 次语义调用</p>
        <dl className="deep-note-stat-list">
          <div><dt>节点尝试</dt><dd>{budget?.nodeAttemptLimit ?? 5}</dd></div>
          <div><dt>章节修订</dt><dd>{budget?.sectionRevisionLimit ?? 5}</dd></div>
          <div><dt>局部重规划</dt><dd>{budget?.replanLimit ?? 4}</dd></div>
          <div><dt>安全并发</dt><dd>{budget?.maxParallelNodes ?? 2}</dd></div>
        </dl>
        <div className="deep-note-pane-heading"><ShieldCheck size={15} /><strong>上下文覆盖</strong></div>
        <div className="deep-note-budget-meter is-context"><span style={{ width: `${contextCoverage}%` }} /></div>
        <p>{contextBudget?.processedMessageCount ?? 0} / {contextBudget?.totalMessageCount ?? 0} 条消息已处理</p>
        <dl className="deep-note-stat-list">
          <div><dt>预计输入</dt><dd>{formatTokenCount(contextBudget?.estimatedInputTokens)}</dd></div>
          <div><dt>直接规划上限</dt><dd>{formatTokenCount(contextBudget?.directInputLimitTokens)}</dd></div>
          <div><dt>分块</dt><dd>{contextBudget?.processedChunkCount ?? 0}/{contextBudget?.chunkCount ?? 0}</dd></div>
          <div><dt>覆盖状态</dt><dd>{contextBudget?.coverageComplete ? "完整" : "处理中"}</dd></div>
        </dl>
        {(contextBudget?.omittedMessageIds.length ?? 0) > 0 ? (
          <p className="deep-note-inline-warning"><AlertTriangle size={13} />仍有 {contextBudget?.omittedMessageIds.length} 条消息未处理</p>
        ) : null}
        <div className="deep-note-pane-heading"><Network size={15} /><strong>执行图</strong></div>
        <p>{completedNodes}/{nodes.length} 个节点完成</p>
        <div className="deep-note-node-list">
          {nodes.slice(0, 18).map((node) => (
            <div key={node.nodeId} data-status={node.status}>
              <span />
              <small>{node.nodeType}</small>
            </div>
          ))}
        </div>
      </aside>
    </section>
  );
}

function formatTokenCount(value: number | undefined) {
  if (!value) return "-";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value);
}
