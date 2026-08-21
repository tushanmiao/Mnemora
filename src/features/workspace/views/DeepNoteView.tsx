import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  AlertTriangle, ArrowLeft, BookOpen, BrainCircuit, Check, CheckCircle2,
  ChevronDown, CircleDot, Clock3, Copy, Database, Gauge, ListTree,
  LoaderCircle, Network, Pause, PauseCircle, Play, RotateCcw, ShieldCheck,
  Square, XCircle,
} from "lucide-react";
import type {
  DeepNoteSectionProgress, DeepNoteSectionStatus, NotePipelinePhase,
} from "../../chat/api/notePipeline";
import type { DeepNoteSection } from "../../chat/notePipeline/outlineSchema";
import {
  buildDeepNoteWorkflow, describeNotePipelineEvent, diagnoseDeepNoteRuntime,
  formatDuration, type DeepNoteWorkflowStatus,
} from "../runtime/deepNoteDiagnostics";
import { useDeepNoteViewRuntime } from "../runtime/DeepNoteViewRuntime";
import "../styles/deep-note-workspace.css";

const PHASE_LABELS: Record<NotePipelinePhase, string> = {
  preflight: "检查输入", analyzing: "生成知识结构", awaitingOutline: "等待计划确认",
  compiling: "编译执行计划", queued: "等待执行", drafting: "生成章节",
  validating: "验证章节", replanning: "调整计划", assembling: "组装笔记",
  persisting: "保存笔记", paused: "已暂停", blocked: "等待处理",
  done: "已完成", cancelled: "已停止", error: "生成失败",
};

const SECTION_STATUS_LABELS: Record<DeepNoteSectionStatus, string> = {
  pending: "等待", ready: "就绪", inProgress: "生成中", completed: "已完成",
  needsReview: "待检查", needsRevision: "待修订", failed: "失败",
  blocked: "已阻塞", skipped: "已跳过", interrupted: "已中断",
};

const TERMINAL_PHASES = new Set<NotePipelinePhase>(["done", "cancelled", "error"]);
const PAUSABLE_PHASES = new Set<NotePipelinePhase>([
  "analyzing", "compiling", "queued", "drafting", "validating", "replanning",
]);

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

function workflowIcon(status: DeepNoteWorkflowStatus) {
  if (status === "completed") return <Check size={13} />;
  if (status === "failed") return <XCircle size={13} />;
  if (status === "paused" || status === "stopped") return <Pause size={13} />;
  if (status === "active") return <LoaderCircle size={13} className="deep-note-progress-spinner" />;
  return <CircleDot size={12} />;
}

function formatTime(value: number | undefined): string {
  if (!value) return "--:--:--";
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit", minute: "2-digit", second: "2-digit",
  }).format(new Date(value));
}

function capabilityLabel(value: boolean | null | undefined): string {
  if (value === true) return "支持";
  if (value === false) return "不支持";
  return "未识别";
}

function capabilityState(value: boolean | null | undefined): "on" | "off" | "unknown" {
  if (value === true) return "on";
  if (value === false) return "off";
  return "unknown";
}

function SectionItem({ section, selected, disabled, onChange }: {
  section: DeepNoteSection;
  selected: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="deep-note-plan-section">
      <input type="checkbox" checked={selected} disabled={disabled} onChange={(event) => onChange(event.target.checked)} />
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

function SectionProgressItem({ section, heading }: { section: DeepNoteSectionProgress; heading: string }) {
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
  const [logOpen, setLogOpen] = useState(true);
  const [planOpen, setPlanOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [fallbackModelKey, setFallbackModelKey] = useState("");
  const [switchingModel, setSwitchingModel] = useState(false);

  useEffect(() => {
    setSelected(new Set(outline?.sections.map((section) => section.id) ?? []));
  }, [outline]);

  useEffect(() => {
    const progress = runtime.progress;
    if (!progress || progress.terminal) return undefined;
    setClock(Date.now());
    const timer = window.setInterval(() => setClock(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [runtime.progress?.runId, runtime.progress?.terminal]);

  const phase = runtime.progress?.phase
    ?? runtime.detail?.run.phase
    ?? (runtime.review ? "awaitingOutline" : "preflight");
  const tone = phaseTone(phase);
  const terminal = runtime.progress?.terminal || TERMINAL_PHASES.has(phase);
  const paused = phase === "paused";
  const abandoned = Boolean(runtime.detail?.run.abandoned);
  const failed = phase === "error" || phase === "blocked";
  const stopped = phase === "cancelled";
  const canPause = Boolean(runtime.progress?.runId ?? runtime.detail?.run.id) && PAUSABLE_PHASES.has(phase);
  const sections = runtime.detail?.sections ?? [];
  const completedSections = sections.filter((section) => section.status === "completed").length;
  const failedSections = sections.filter((section) => section.status === "failed").length;
  const activeSections = sections.filter((section) => section.status === "inProgress").length;
  const sectionProgressActive = phase === "drafting" || phase === "validating" || phase === "replanning";
  const totalSections = (sectionProgressActive ? runtime.progress?.total : null)
    ?? runtime.detail?.run.selectedSectionIds.length ?? outline?.sections.length ?? 0;
  const processedSections = (sectionProgressActive ? runtime.progress?.current : null)
    ?? completedSections + failedSections;
  const taskProgress = totalSections > 0
    ? Math.min(100, Math.round((processedSections / totalSections) * 100)) : 0;
  const budget = runtime.detail?.budget;
  const contextBudget = runtime.detail?.contextBudget;
  const budgetProgress = budget && budget.semanticCallLimit > 0
    ? Math.min(100, Math.round((budget.semanticCallsUsed / budget.semanticCallLimit) * 100)) : 0;
  const nodes = runtime.detail?.nodes ?? [];
  const completedNodes = useMemo(() => nodes.filter((node) => node.status === "completed").length, [nodes]);
  const sectionHeadings = useMemo(() => new Map(
    outline?.sections.map((section) => [section.id, section.heading]) ?? [],
  ), [outline]);
  const statusMessage = runtime.progress?.message ?? runtime.detail?.run.errorMessage
    ?? (runtime.review ? "计划已生成，等待确认后开始执行。" : "正在检查输入与模型上下文预算…");
  const activity = runtime.progress?.activity;
  const activityElapsed = activity ? Math.max(0, clock - activity.startedAt) : null;
  const diagnosis = diagnoseDeepNoteRuntime(
    phase, activity, runtime.progress?.updatedAt ?? runtime.detail?.run.updatedAt, clock,
  );
  const workflow = buildDeepNoteWorkflow(runtime.detail, phase);
  const events = runtime.detail?.events ?? [];
  const visibleEvents = events.slice(-60).reverse();
  const runProviderId = runtime.detail?.preflight?.model.providerId ?? runtime.detail?.run.providerId ?? null;
  const runModelId = runtime.detail?.preflight?.model.modelId ?? runtime.detail?.run.modelId ?? null;
  const runApiModel = runtime.detail?.preflight?.model.apiModel ?? null;
  const preflight = runtime.detail?.preflight;
  const modelCapabilities = preflight?.model.capabilities;
  const skillProfiles = runtime.detail?.skillProfiles;
  const plannerSkills = skillProfiles?.planner ?? [];
  const writerSkills = skillProfiles?.writer ?? [];
  const reviewerSkills = skillProfiles?.reviewer ?? [];
  const skillCount = plannerSkills.length + writerSkills.length + reviewerSkills.length;
  const skillTitle = [
    plannerSkills.length > 0 ? `Planner：${plannerSkills.map((skill) => skill.name).join("、")}` : "",
    writerSkills.length > 0 ? `Writer：${writerSkills.map((skill) => skill.name).join("、")}` : "",
    reviewerSkills.length > 0 ? `Reviewer：${reviewerSkills.map((skill) => skill.name).join("、")}` : "",
  ].filter(Boolean).join("\n");
  const localReaderFormats = preflight?.localReaders
    ? [
        preflight.localReaders.text ? "TXT" : "",
        preflight.localReaders.pdf ? "PDF" : "",
        preflight.localReaders.docx ? "DOCX" : "",
        preflight.localReaders.xlsx ? "XLSX" : "",
      ].filter(Boolean)
    : [];
  const modelOption = runProviderId && runModelId
    ? runtime.modelOptions.find((option) => option.providerId === runProviderId && option.modelId === runModelId)
    : null;
  const currentModelKey = runProviderId && runModelId ? `${runProviderId}:${runModelId}` : "";
  const fallbackModelOptions = runtime.modelOptions.filter((option) => (
    option.hasApiKey && `${option.providerId}:${option.modelId}` !== currentModelKey
  ));

  useEffect(() => {
    const firstAvailable = fallbackModelOptions[0];
    setFallbackModelKey(firstAvailable ? `${firstAvailable.providerId}:${firstAvailable.modelId}` : "");
  }, [currentModelKey, runtime.modelOptions]);

  const switchModel = async () => {
    const [providerId, modelId] = fallbackModelKey.split(":");
    if (!providerId || !modelId || switchingModel) return;
    setSwitchingModel(true);
    try {
      await runtime.onSwitchModel(providerId, modelId);
    } finally {
      setSwitchingModel(false);
    }
  };

  const diagnosticText = useMemo(() => {
    const lines = [
      "深度笔记运行诊断",
      `Run ID: ${runtime.detail?.run.id ?? runtime.progress?.runId ?? "尚未创建"}`,
      `阶段: ${PHASE_LABELS[phase]} (${phase})`,
      `状态: ${statusMessage}`,
          `模型: ${modelOption?.providerName ?? runProviderId ?? "-"} / ${modelOption?.displayName ?? runModelId ?? "-"} (${runApiModel ?? modelOption?.apiModel ?? "-"})`,
      `输入覆盖: ${contextBudget?.processedMessageCount ?? 0}/${contextBudget?.totalMessageCount ?? 0}`,
      `语义调用: ${budget?.semanticCallsUsed ?? 0}/${budget?.semanticCallLimit ?? 0}`,
      activity
        ? `当前请求: ${activity.callId}，第 ${activity.attempt}/${activity.maxRetries + 1} 次，已等待 ${formatDuration(activityElapsed ?? 0)}，超时 ${formatDuration(activity.timeoutMs)}`
        : "当前请求: 无",
      "", "最近运行记录:",
      ...events.slice(-60).map((event) => {
        const description = describeNotePipelineEvent(event);
        return `${event.sequence}. ${formatTime(event.createdAt)} ${description.label} - ${description.detail}`;
      }),
    ];
    return lines.join("\n");
  }, [activity, activityElapsed, budget, contextBudget, events, phase, runtime.detail, runtime.progress?.runId, statusMessage]);

  const copyDiagnostics = async () => {
    try {
      await navigator.clipboard.writeText(diagnosticText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_800);
    } catch {
      setCopied(false);
    }
  };

  return (
    <section className="deep-note-workspace" aria-label="深度笔记工作区">
      <header className="deep-note-workspace-header">
        <button className="icon-button" type="button" title="返回" onClick={runtime.onReturn}><ArrowLeft size={18} /></button>
        <div className="deep-note-workspace-title">
          <span><BrainCircuit size={15} />Plan-and-Execute</span>
          <h1>{outline?.title ?? "深度笔记"}</h1>
        </div>
        <div className="deep-note-run-status" data-tone={tone}><span className="deep-note-status-dot" />{PHASE_LABELS[phase]}</div>
        {failed ? (
          <div className="deep-note-run-actions">
            <button className="settings-button settings-button-primary" type="button" disabled={runtime.controlBusy} onClick={runtime.onRetry}>
              <RotateCcw size={14} />重试失败步骤
            </button>
            <button className="settings-button settings-button-secondary" type="button" disabled={runtime.controlBusy} onClick={runtime.onRestart}>
              <BrainCircuit size={14} />重新生成
            </button>
          </div>
        ) : stopped && !abandoned ? (
          <div className="deep-note-run-actions">
            <button className="settings-button settings-button-primary" type="button" disabled={runtime.controlBusy} onClick={runtime.onResume}>
              <Play size={14} />从检查点继续
            </button>
            <button className="settings-button settings-button-secondary" type="button" disabled={runtime.controlBusy} onClick={runtime.onRestart}>
              <RotateCcw size={14} />重新生成
            </button>
          </div>
        ) : !terminal ? (
          <div className="deep-note-run-actions">
            {paused ? (
              <button className="settings-button settings-button-primary" type="button" disabled={runtime.controlBusy} onClick={runtime.onResume}>
                <Play size={14} />继续
              </button>
            ) : canPause ? (
              <button className="settings-button settings-button-secondary" type="button" disabled={runtime.controlBusy} onClick={runtime.onPause}>
                <Pause size={14} />暂停
              </button>
            ) : null}
            <button className="settings-button settings-button-secondary deep-note-stop-button" type="button" disabled={runtime.controlBusy} onClick={runtime.onCancel}>
              <Square size={14} />停止
            </button>
          </div>
        ) : runtime.detail?.run.noteId ? (
          <button className="settings-button settings-button-primary" type="button" onClick={runtime.onOpenNote}>
            <BookOpen size={14} />打开笔记库
          </button>
        ) : null}
      </header>

      <main className="deep-note-main-pane">
        <section className="deep-note-progress-overview" data-tone={tone} aria-live="polite">
          <span className="deep-note-progress-icon">{statusIcon(phase)}</span>
          <div className="deep-note-progress-copy">
            <div><strong>{PHASE_LABELS[phase]}</strong><time>最近事件 {formatTime(runtime.progress?.updatedAt ?? runtime.detail?.run.updatedAt)}</time></div>
            <p>{statusMessage}</p>
            <div className="deep-note-runtime-diagnosis" data-tone={diagnosis.tone}>
              <div><strong>{diagnosis.title}</strong><span>{diagnosis.detail}</span></div>
              {diagnosis.elapsedSeconds !== null ? (
                <dl>
                  <div><dt>{activity?.kind === "retryWait" ? "已等待" : "已用时"}</dt><dd>{formatDuration(diagnosis.elapsedSeconds * 1_000)}</dd></div>
                  {diagnosis.timeoutSeconds !== null ? <div><dt>距超时</dt><dd>{formatDuration(diagnosis.timeoutSeconds * 1_000)}</dd></div> : null}
                </dl>
              ) : null}
            </div>
            {totalSections > 0 ? (
              <>
                <div className="deep-note-task-meter" aria-label={`章节处理进度 ${taskProgress}%`}><span style={{ width: `${taskProgress}%` }} /></div>
                <dl className="deep-note-progress-stats">
                  <div><dt>已完成</dt><dd>{completedSections}</dd></div><div><dt>生成中</dt><dd>{activeSections}</dd></div>
                  <div><dt>失败</dt><dd>{failedSections}</dd></div><div><dt>总章节</dt><dd>{totalSections}</dd></div>
                </dl>
              </>
            ) : null}
          </div>
        </section>

        {runtime.detail?.run.errorMessage ? (
          <div className="deep-note-terminal-message is-error"><XCircle size={16} /><div><strong>任务未能完成</strong><p>{runtime.detail.run.errorMessage}</p></div></div>
        ) : null}
        {runtime.progress?.degraded ? (
          <div className="deep-note-terminal-message is-warning"><AlertTriangle size={16} /><div><strong>已保存部分结果</strong><p>停止前已完成的章节已经写入笔记库，未完成章节没有被标记为成功。</p></div></div>
        ) : null}
        {abandoned ? (
          <div className="deep-note-terminal-message is-warning"><PauseCircle size={16} /><div><strong>任务已遗弃</strong><p>来源对话已删除，任务检查点仍保留用于诊断，但不会继续、重试或重新生成。</p></div></div>
        ) : null}
        {(runtime.detail?.run.warnings.length ?? 0) > 0 ? (
          <div className="deep-note-warning-list">{runtime.detail?.run.warnings.map((warning) => <p key={warning}><AlertTriangle size={14} />{warning}</p>)}</div>
        ) : null}

        {outline ? (
          <>
            <section className="deep-note-plan-panel" aria-label="语义计划">
              <div className="deep-note-plan-panel-header">
                <button
                  type="button"
                  onClick={() => setPlanOpen((value) => !value)}
                  aria-expanded={planOpen}
                  aria-controls={planOpen ? "deep-note-plan-content" : undefined}
                >
                  <ChevronDown size={15} data-open={planOpen} />
                  <strong>语义计划</strong>
                  <span>{selected.size}/{outline.sections.length} 个章节</span>
                </button>
                <small>{planOpen ? "在此区域内滚动选择" : "点击展开"}</small>
              </div>
              {planOpen ? (
                <div className="deep-note-plan-content" id="deep-note-plan-content" tabIndex={0}>
                  <div className="deep-note-plan-intro">
                    <div><span>目标</span><p>{outline.goal || outline.summary || "根据当前输入建立可验证、可复习的深度笔记。"}</p></div>
                    <div><span>读者与范围</span><p>{[outline.audience, outline.scope].filter(Boolean).join(" · ") || "沿用当前对话语境"}</p></div>
                  </div>
                  <div className="deep-note-plan-list">
                    {outline.sections.map((section) => (
                      <SectionItem key={section.id} section={section} selected={selected.has(section.id)} disabled={runtime.busy || !runtime.review}
                        onChange={(checked) => setSelected((current) => {
                          const next = new Set(current);
                          if (checked) next.add(section.id); else next.delete(section.id);
                          return next;
                        })}
                      />
                    ))}
                  </div>
                </div>
              ) : null}
            </section>
            {runtime.review ? (
              <div className="deep-note-plan-actions">
                <label><span>调整计划要求</span><textarea value={requirement} disabled={runtime.busy}
                  placeholder="只描述会实质改变章节、证据或范围的要求" onChange={(event) => setRequirement(event.target.value)} /></label>
                <div>
                  <button className="settings-button settings-button-secondary" type="button" disabled={runtime.busy || !requirement.trim()} onClick={() => runtime.onAdjust(requirement.trim())}>
                    <RotateCcw size={14} />重新规划
                  </button>
                  <button className="settings-button settings-button-primary" type="button" disabled={runtime.busy || selected.size === 0} onClick={() => runtime.onConfirm(selected)}>
                    <Check size={14} />确认计划并执行
                  </button>
                </div>
              </div>
            ) : null}
          </>
        ) : (
          <section className="deep-note-waiting-context">
            <ListTree size={20} /><div><strong>章节执行尚未开始</strong><p>当前正在完成提纲生成。右侧工作流会区分已经完成的输入准备与正在进行的模型规划。</p></div>
          </section>
        )}

        <section className="deep-note-event-log" aria-label="深度笔记运行记录">
          <div className="deep-note-event-log-header">
            <button type="button" onClick={() => setLogOpen((value) => !value)} aria-expanded={logOpen}>
              <ChevronDown size={15} data-open={logOpen} /><strong>运行记录</strong><span>{events.length} 条</span>
            </button>
            <div>
              {copied ? <span role="status">已复制</span> : null}
              <button className="icon-button" type="button" title="复制运行诊断" aria-label="复制运行诊断" onClick={copyDiagnostics}><Copy size={15} /></button>
            </div>
          </div>
          {logOpen ? (
            visibleEvents.length > 0 ? (
              <ol className="deep-note-event-list">
                {visibleEvents.map((event) => {
                  const description = describeNotePipelineEvent(event);
                  return (
                    <li key={event.sequence} data-event={event.eventType}>
                      <time>{formatTime(event.createdAt)}</time><span />
                      <div><strong>{description.label}</strong><p>{description.detail}</p></div><small>#{event.sequence}</small>
                    </li>
                  );
                })}
              </ol>
            ) : <p className="deep-note-event-empty">任务创建后，阶段变化和模型请求会记录在这里。</p>
          ) : null}
        </section>

        {runtime.detail?.markdownPreview ? (
          <article className="deep-note-preview" aria-label="已完成内容预览">
            <div className="deep-note-preview-header">
              <button
                type="button"
                onClick={() => setPreviewOpen((value) => !value)}
                aria-expanded={previewOpen}
                aria-controls={previewOpen ? "deep-note-preview-content" : undefined}
              >
                <ChevronDown size={15} data-open={previewOpen} />
                <strong>已完成内容预览</strong>
                <span>{runtime.detail.markdownPreview.length.toLocaleString()} 字符</span>
              </button>
              <small>{previewOpen ? "在此区域内滚动查看" : "点击展开"}</small>
            </div>
            {previewOpen ? (
              <div className="deep-note-preview-content" id="deep-note-preview-content" tabIndex={0}>
                <pre>{runtime.detail.markdownPreview}</pre>
              </div>
            ) : null}
          </article>
        ) : null}
      </main>

      <aside className="deep-note-run-pane">
        <div className="deep-note-pane-heading"><ListTree size={15} /><strong>工作流</strong></div>
        <ol className="deep-note-workflow-list">
          {workflow.map((step) => (
            <li key={step.id} data-status={step.status}>
              <span className="deep-note-workflow-marker">{workflowIcon(step.status)}</span>
              <div><strong>{step.label}</strong><p>{step.description}</p></div>
            </li>
          ))}
        </ol>

        <div className="deep-note-pane-heading"><Database size={15} /><strong>输入与模型</strong></div>
        <dl className="deep-note-stat-list">
          <div><dt>消息</dt><dd>{runtime.detail?.inputSnapshot?.messageIds.length ?? 0}</dd></div>
          <div><dt>附件</dt><dd>{runtime.detail?.inputSnapshot?.attachmentIds.length ?? 0}</dd></div>
          <div><dt>服务商</dt><dd title={modelOption?.providerName ?? runProviderId ?? undefined}>{modelOption?.providerName ?? runProviderId ?? "-"}</dd></div>
          <div><dt>模型</dt><dd title={modelOption?.displayName ?? runModelId ?? undefined}>{modelOption?.displayName ?? runModelId ?? "-"}</dd></div>
          <div><dt>API 标识</dt><dd className="deep-note-model-api" title={runApiModel ?? modelOption?.apiModel ?? undefined}>{runApiModel ?? modelOption?.apiModel ?? "-"}</dd></div>
          <div>
            <dt>模型 Tool</dt>
            <dd className="deep-note-capability-value" data-state={capabilityState(modelCapabilities?.tools)}>{capabilityLabel(modelCapabilities?.tools)}</dd>
          </div>
          <div>
            <dt>模型视觉</dt>
            <dd className="deep-note-capability-value" data-state={capabilityState(modelCapabilities?.vision)}>
              {capabilityLabel(modelCapabilities?.vision)}{preflight?.requiresVision ? " · 本次需要" : ""}
            </dd>
          </div>
          <div>
            <dt>推理能力</dt>
            <dd className="deep-note-capability-value" data-state={capabilityState(modelCapabilities?.reasoning)}>{capabilityLabel(modelCapabilities?.reasoning)}</dd>
          </div>
          <div><dt>本地 Reader</dt><dd title={localReaderFormats.join("、") || undefined}>{localReaderFormats.length > 0 ? localReaderFormats.join(" · ") : "未加载"}</dd></div>
          <div><dt>Skill</dt><dd title={skillTitle || undefined}>{skillCount > 0 ? `已冻结 ${skillCount} 项` : "未加载"}</dd></div>
        </dl>
        {skillCount > 0 ? <p className="deep-note-skill-summary" title={skillTitle}>Planner {plannerSkills.length} · Writer {writerSkills.length} · Reviewer {reviewerSkills.length}</p> : null}
        {preflight?.requiresLocalReaders ? <p className="deep-note-pane-note">文档由 Mnemora 本地 Reader 读取，不依赖模型 Tool。</p> : null}
        {preflight?.warnings.map((warning) => <p className="deep-note-inline-warning" key={warning}><AlertTriangle size={13} />{warning}</p>)}

        {failed ? (
          <section className="deep-note-model-recovery" aria-label="切换备用模型">
            <div className="deep-note-pane-heading"><RotateCcw size={15} /><strong>模型请求失败</strong></div>
            <p>可以先重试当前失败步骤，也可以选择备用模型并从最新会话内容重新生成。</p>
            {fallbackModelOptions.length > 0 ? (
              <>
                <label>
                  <span>备用模型</span>
                  <select value={fallbackModelKey} disabled={switchingModel} onChange={(event) => setFallbackModelKey(event.target.value)}>
                    {fallbackModelOptions.map((option) => {
                      const key = `${option.providerId}:${option.modelId}`;
                      return <option key={key} value={key}>{option.providerName} · {option.displayName}</option>;
                    })}
                  </select>
                </label>
                <button className="settings-button settings-button-secondary" type="button" disabled={!fallbackModelKey || switchingModel} onClick={() => { void switchModel(); }}>
                  {switchingModel ? <LoaderCircle size={14} className="deep-note-progress-spinner" /> : <RotateCcw size={14} />}
                  {switchingModel ? "正在重新开始…" : "切换并重新开始"}
                </button>
              </>
            ) : <p className="deep-note-model-recovery-empty">没有可用的备用模型。请先在设置中配置其他服务商和 API Key。</p>}
          </section>
        ) : null}

        <div className="deep-note-pane-heading"><ShieldCheck size={15} /><strong>规划输入覆盖</strong></div>
        <p className="deep-note-pane-note">覆盖完成只表示消息已纳入规划输入；提纲仍需等待模型生成。</p>
        <dl className="deep-note-stat-list">
          <div><dt>消息</dt><dd>{contextBudget?.processedMessageCount ?? 0}/{contextBudget?.totalMessageCount ?? 0}</dd></div>
          <div><dt>来源分块</dt><dd>{contextBudget?.processedChunkCount ?? 0}/{contextBudget?.chunkCount ?? 0}</dd></div>
          <div><dt>预计输入</dt><dd>{formatTokenCount(contextBudget?.estimatedInputTokens)}</dd></div>
          <div><dt>处理结果</dt><dd>{contextBudget?.coverageComplete ? "完整" : "处理中"}</dd></div>
        </dl>
        {(contextBudget?.omittedMessageIds.length ?? 0) > 0 ? <p className="deep-note-inline-warning"><AlertTriangle size={13} />仍有 {contextBudget?.omittedMessageIds.length} 条消息未处理</p> : null}

        <div className="deep-note-pane-heading"><Gauge size={15} /><strong>运行预算</strong></div>
        <div className="deep-note-budget-meter"><span style={{ width: `${budgetProgress}%` }} /></div>
        <p>{budget?.semanticCallsUsed ?? 0} / {budget?.semanticCallLimit ?? 0} 次语义调用</p>
        <dl className="deep-note-stat-list">
          <div><dt>请求重试上限</dt><dd>{runtime.detail?.run.retryAttempts ?? 5} 次（最多 {(runtime.detail?.run.retryAttempts ?? 5) + 1} 次请求）</dd></div>
          <div><dt>节点尝试上限</dt><dd>{budget?.nodeAttemptLimit ?? 5}</dd></div>
          <div><dt>章节修订上限</dt><dd>{budget?.sectionRevisionLimit ?? 5}</dd></div>
          <div><dt>提纲调整</dt><dd>{budget?.replansUsed ?? 0}/{budget?.replanLimit ?? 4}</dd></div>
          <div><dt>章节执行</dt><dd>按章节计划</dd></div>
        </dl>

        {totalSections > 0 ? (
          <>
            <div className="deep-note-pane-heading"><Clock3 size={15} /><strong>章节进度</strong></div>
            <div className="deep-note-budget-meter is-task"><span style={{ width: `${taskProgress}%` }} /></div>
            <p>{processedSections} / {totalSections} 个章节已处理</p>
            {sections.length > 0 ? (
              <div className="deep-note-section-progress-list">
                {sections.map((section) => <SectionProgressItem key={section.sectionId} section={section} heading={sectionHeadings.get(section.sectionId) ?? `章节 ${section.position + 1}`} />)}
              </div>
            ) : null}
          </>
        ) : null}

        {nodes.length > 0 ? (
          <>
            <div className="deep-note-pane-heading"><Network size={15} /><strong>执行图（计划依赖）</strong></div>
            <p>{completedNodes}/{nodes.length} 个节点完成</p>
            <div className="deep-note-node-list">
              {nodes.map((node) => (
                <div key={node.nodeId} data-status={node.status} title={node.dependsOn?.length ? `依赖 ${node.dependsOn.join("、")}` : "无前置依赖"}>
                  <span />
                  <small>{node.sectionId ? `${sectionHeadings.get(node.sectionId) ?? node.sectionId} · ` : ""}{node.nodeType}</small>
                </div>
              ))}
            </div>
          </>
        ) : null}
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
