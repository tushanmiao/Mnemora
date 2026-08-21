import { useEffect, useState } from "react";
import { AlertCircle, GitBranch, HelpCircle, X } from "lucide-react";
import type { DeepNoteOutline } from "./outlineSchema";
import "./deep-note.css";

export function DeepNoteOutlineDialog({
  outline,
  busy,
  onCancel,
  onAdjust,
  onConfirm,
}: {
  outline: DeepNoteOutline;
  busy: boolean;
  onCancel: () => void;
  onAdjust: (requirement: string) => void;
  onConfirm: (selectedSectionIds: Set<string>) => void;
}) {
  const [selected, setSelected] = useState(() => new Set(outline.sections.map((section) => section.id)));
  const [requirement, setRequirement] = useState("");

  useEffect(() => {
    setSelected(new Set(outline.sections.map((section) => section.id)));
  }, [outline]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="deep-note-backdrop" role="presentation">
      <section className="deep-note-dialog" role="dialog" aria-modal="true" aria-labelledby="deep-note-title">
        <header className="deep-note-dialog-header">
          <div>
            <h2 id="deep-note-title">{outline.title}</h2>
            {outline.summary ? <p>{outline.summary}</p> : null}
          </div>
          <button className="icon-button" type="button" title="取消" onClick={onCancel}>
            <X size={18} />
          </button>
        </header>

        {outline.weakPoints.length > 0 ? (
          <div className="deep-note-weak-points">
            <AlertCircle size={16} />
            <span>对话暴露的薄弱点：{outline.weakPoints.join("；")}</span>
          </div>
        ) : null}

        {(outline.hiddenQuestions?.length ?? 0) > 0
          || (outline.knowledgeGaps?.length ?? 0) > 0
          || (outline.misconceptions?.length ?? 0) > 0 ? (
            <div className="deep-note-weak-points">
              <HelpCircle size={16} />
              <span>
                真正需要解决：{[
                  ...(outline.hiddenQuestions ?? []),
                  ...(outline.knowledgeGaps ?? []),
                  ...(outline.misconceptions ?? []),
                ].join("；")}
              </span>
            </div>
          ) : null}

        {(outline.visualizationOpportunities?.length ?? 0) > 0 ? (
          <div className="deep-note-weak-points">
            <GitBranch size={16} />
            <span>计划图形化表达：{outline.visualizationOpportunities?.join("；")}</span>
          </div>
        ) : null}

        <div className="deep-note-section-list">
          {outline.sections.map((section) => (
            <label className="deep-note-section-item" key={section.id}>
              <input
                type="checkbox"
                checked={selected.has(section.id)}
                disabled={busy}
                onChange={(event) => setSelected((current) => {
                  const next = new Set(current);
                  if (event.target.checked) next.add(section.id);
                  else next.delete(section.id);
                  return next;
                })}
              />
              <span>
                <strong>{section.heading}</strong>
                <small>{section.brief}</small>
                {section.needsSupplement ? <em>包含 AI 补充背景</em> : null}
              </span>
            </label>
          ))}
        </div>

        <label className="deep-note-requirement">
          <span>补充要求（填写后重新调整提纲）</span>
          <textarea
            value={requirement}
            disabled={busy}
            placeholder="例如：增加 PostgreSQL 与 MySQL 的实现差异，不需要基础 SQL 章节。"
            onChange={(event) => setRequirement(event.target.value)}
          />
        </label>

        <footer className="deep-note-dialog-footer">
          <span>调用预算会计入来源分块、视觉分析、章节生成与语义修订；网络重试单独统计。</span>
          <div>
            <button className="settings-button settings-button-secondary" type="button" disabled={busy || !requirement.trim()} onClick={() => onAdjust(requirement)}>
              调整提纲
            </button>
            <button className="settings-button settings-button-primary" type="button" disabled={busy || selected.size === 0} onClick={() => onConfirm(selected)}>
              确认并生成
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
