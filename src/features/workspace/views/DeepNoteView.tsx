import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  BrainCircuit,
  Check,
  CircleDot,
  FileText,
  Gauge,
  Network,
  RotateCcw,
  ShieldCheck,
  Square,
} from "lucide-react";
import { useDeepNoteViewRuntime } from "../runtime/DeepNoteViewRuntime";
import type { DeepNoteSection } from "../../chat/notePipeline/outlineSchema";
import "../styles/deep-note-workspace.css";

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

export default function DeepNoteView() {
  const runtime = useDeepNoteViewRuntime();
  const outline = runtime.review?.outline ?? runtime.detail?.planVersion?.plan ?? null;
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [requirement, setRequirement] = useState("");

  useEffect(() => {
    setSelected(new Set(outline?.sections.map((section) => section.id) ?? []));
  }, [outline]);

  const budget = runtime.detail?.budget;
  const progress = budget && budget.semanticCallLimit > 0
    ? Math.min(100, Math.round((budget.semanticCallsUsed / budget.semanticCallLimit) * 100))
    : 0;
  const nodes = runtime.detail?.nodes ?? [];
  const completedNodes = useMemo(
    () => nodes.filter((node) => node.status === "completed").length,
    [nodes],
  );

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
        <div className="deep-note-run-status">
          <span className="deep-note-status-dot" />
          {runtime.detail?.run.phase === "awaitingOutline" || runtime.review
            ? "等待计划确认"
            : runtime.detail?.run.phase ?? "准备中"}
        </div>
        <button className="settings-button settings-button-secondary" type="button" onClick={runtime.onCancel}>
          <Square size={14} />停止
        </button>
      </header>

      <aside className="deep-note-source-pane">
        <div className="deep-note-pane-heading"><FileText size={15} /><strong>输入快照</strong></div>
        <dl className="deep-note-stat-list">
          <div><dt>消息</dt><dd>{runtime.detail?.inputSnapshot?.messageIds.length ?? 0}</dd></div>
          <div><dt>附件</dt><dd>{runtime.detail?.inputSnapshot?.attachmentIds.length ?? 0}</dd></div>
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
                <div className="deep-note-section-toolbar"><strong>只读预览</strong></div>
                <pre>{runtime.detail.markdownPreview}</pre>
              </article>
            ) : null}
          </>
        ) : (
          <div className="deep-note-empty"><BrainCircuit size={24} /><span>正在建立输入快照与语义计划</span></div>
        )}
      </main>

      <aside className="deep-note-run-pane">
        <div className="deep-note-pane-heading"><Gauge size={15} /><strong>运行预算</strong></div>
        <div className="deep-note-budget-meter"><span style={{ width: `${progress}%` }} /></div>
        <p>{budget?.semanticCallsUsed ?? 0} / {budget?.semanticCallLimit ?? 0} 次语义调用</p>
        <dl className="deep-note-stat-list">
          <div><dt>节点尝试</dt><dd>{budget?.nodeAttemptLimit ?? 5}</dd></div>
          <div><dt>章节修订</dt><dd>{budget?.sectionRevisionLimit ?? 5}</dd></div>
          <div><dt>局部重规划</dt><dd>{budget?.replanLimit ?? 4}</dd></div>
          <div><dt>安全并发</dt><dd>{budget?.maxParallelNodes ?? 2}</dd></div>
        </dl>
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
