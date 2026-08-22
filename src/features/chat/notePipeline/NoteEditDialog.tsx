import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, FilePenLine, GitCompareArrows, Paperclip, ShieldCheck, X } from "lucide-react";
import type {
  NoteEditDialogRequest,
  NoteEditPrepareResult,
} from "../api/notePipeline";
import { createPatch } from "diff";
import { applySelectedNoteHunks, buildNoteDiff, type NoteDiffHunk } from "./noteDiff";
import "./deep-note.css";

export function NoteEditDialog({
  request,
  result,
  busy,
  onClose,
  onPrepare,
  onApply,
}: {
  request: NoteEditDialogRequest | null;
  result: NoteEditPrepareResult | null;
  busy: boolean;
  onClose: () => void;
  onPrepare: (noteId: string, requirement: string) => void;
  onApply: (selection?: { hunkIds: number[]; titleAccepted: boolean; content: string; diff: string }) => void;
}) {
  const [noteId, setNoteId] = useState("");
  const [requirement, setRequirement] = useState("");
  const [selectedHunks, setSelectedHunks] = useState<Set<number>>(new Set());
  const [titleAccepted, setTitleAccepted] = useState(true);

  useEffect(() => {
    setNoteId(request?.noteId ?? request?.notes[0]?.id ?? "");
    setRequirement("");
  }, [request]);

  const diff = useMemo(
    () => result ? buildNoteDiff(result.proposal.oldContent, result.proposal.newContent) : null,
    [result],
  );

  useEffect(() => {
    setSelectedHunks(new Set(diff?.hunks.map((hunk) => hunk.id) ?? []));
    setTitleAccepted(Boolean(result && result.proposal.newTitle !== result.proposal.oldTitle));
  }, [diff, result]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [busy, onClose]);

  const selectedNote = useMemo(
    () => request?.notes.find((note) => note.id === noteId) ?? null,
    [noteId, request],
  );

  if (!request && !result) return null;

  return (
    <div className="deep-note-backdrop" role="presentation">
      <section className="deep-note-dialog note-edit-dialog" role="dialog" aria-modal="true" aria-labelledby="note-edit-title">
        <header className="deep-note-dialog-header">
          <div>
            <span className="deep-note-eyebrow">
              {result ? <GitCompareArrows size={14} /> : <FilePenLine size={14} />}
              {result ? "修改预览" : "更新已有笔记"}
            </span>
            <h2 id="note-edit-title">
              {result ? `${result.proposal.oldTitle} → ${result.proposal.newTitle}` : "选择目标并描述修改"}
            </h2>
          </div>
          <button className="icon-button" type="button" title="关闭" disabled={busy} onClick={onClose}>
            <X size={18} />
          </button>
        </header>

        {result ? (
          <>
            {result.warnings.length > 0 ? (
              <div className="deep-note-weak-points">
                <AlertTriangle size={16} />
                <span>{result.warnings.join("；")}</span>
              </div>
            ) : null}
            {result.attachmentCount > 0 ? (
              <div className="note-edit-source-units">
                <div>
                  <Paperclip size={15} />
                  <strong>{result.attachmentCount} 个新增附件</strong>
                  <span>{result.sourceUnits.length} 个 Source Unit 已完成读取</span>
                </div>
                <div className="note-edit-source-unit-list">
                  {result.sourceUnits.filter((unit) => unit.kind === "attachment").map((unit) => (
                    <span key={unit.unitId} data-status={unit.status}>
                      <ShieldCheck size={13} />
                      {unit.parserId} · {unit.chunkIds.length} chunks
                    </span>
                  ))}
                </div>
                {result.globalReviewPassed ? (
                  <small>全局复核已通过；覆盖快照仍只会在确认后推进。</small>
                ) : result.requiresGlobalReview ? (
                  <small>附件可能影响已有结论；请在应用前检查完整 Diff。覆盖快照只会在确认后推进。</small>
                ) : null}
              </div>
            ) : null}
            <div className="note-edit-diff-toolbar">
              <strong>逐段确认变更</strong>
              <span>{selectedHunks.size}/{diff?.hunks.length ?? 0} 段已选择</span>
              <button type="button" onClick={() => setSelectedHunks(new Set(diff?.hunks.map((hunk) => hunk.id) ?? []))}>全部接受</button>
              <button type="button" onClick={() => setSelectedHunks(new Set())}>全部拒绝</button>
            </div>
            {result.proposal.newTitle !== result.proposal.oldTitle ? (
              <label className={`note-edit-title-hunk${titleAccepted ? " is-selected" : ""}`}>
                <input type="checkbox" checked={titleAccepted} onChange={(event) => setTitleAccepted(event.target.checked)} />
                <span><strong>标题</strong><small>{result.proposal.oldTitle} → {result.proposal.newTitle}</small></span>
              </label>
            ) : null}
            <div className="note-edit-hunk-list" aria-label="逐段 Markdown 差异">
              {(diff?.hunks ?? []).map((hunk) => (
                <DiffHunkRow
                  key={hunk.id}
                  hunk={hunk}
                  selected={selectedHunks.has(hunk.id)}
                  onChange={(selected) => setSelectedHunks((current) => {
                    const next = new Set(current);
                    if (selected) next.add(hunk.id); else next.delete(hunk.id);
                    return next;
                  })}
                />
              ))}
            </div>
            <footer className="deep-note-dialog-footer">
              <span>应用时会先备份当前版本；笔记已被其他编辑修改时会拒绝覆盖。</span>
              <div>
                <button className="settings-button settings-button-secondary" type="button" disabled={busy} onClick={onClose}>
                  放弃
                </button>
                <button
                  className="settings-button settings-button-primary"
                  type="button"
                  disabled={busy || (!titleAccepted && selectedHunks.size === 0)}
                  onClick={() => {
                    const ids = Array.from(selectedHunks).sort((a, b) => a - b);
                    const content = diff
                      ? applySelectedNoteHunks(result.proposal.oldContent, result.proposal.newContent, selectedHunks)
                      : result.proposal.newContent;
                    onApply({
                      hunkIds: ids,
                      titleAccepted,
                      content,
                      diff: createPatch(
                        result.proposal.oldTitle || "note.md",
                        result.proposal.oldContent,
                        content,
                      ),
                    });
                  }}
                >
                  应用选中修改
                </button>
              </div>
            </footer>
          </>
        ) : request ? (
          <>
            {request.noteId ? (
              <div className="note-edit-fixed-target">
                <span>目标笔记</span>
                <strong>{selectedNote?.title ?? "当前笔记"}</strong>
              </div>
            ) : (
              <div className="note-edit-target-list" role="radiogroup" aria-label="目标笔记">
                {request.notes.map((note) => (
                  <label className="note-edit-target" key={note.id}>
                    <input
                      type="radio"
                      name="note-edit-target"
                      value={note.id}
                      checked={noteId === note.id}
                      disabled={busy}
                      onChange={() => setNoteId(note.id)}
                    />
                    <span>
                      <strong>{note.title}</strong>
                      <small>{note.contentPreview || "空笔记"}</small>
                    </span>
                  </label>
                ))}
              </div>
            )}

            {request.selectedText ? (
              <blockquote className="note-edit-selection">
                {request.sectionHeading ? <strong>{request.sectionHeading}</strong> : null}
                <span>{request.selectedText}</span>
              </blockquote>
            ) : null}

            <label className="deep-note-requirement note-edit-requirement">
              <span>{request.selectedText ? "修改要求" : "合并要求（可选）"}</span>
              <textarea
                value={requirement}
                disabled={busy}
                autoFocus
                placeholder={request.selectedText
                  ? "例如：改写得更准确，并补充一个最小示例。"
                  : "例如：只合并对话中的新增结论，保留现有章节结构。"}
                onChange={(event) => setRequirement(event.target.value)}
              />
            </label>
            <footer className="deep-note-dialog-footer">
              <span>AI 只生成候选补丁，不会直接覆盖笔记。</span>
              <div>
                <button className="settings-button settings-button-secondary" type="button" disabled={busy} onClick={onClose}>
                  取消
                </button>
                <button
                  className="settings-button settings-button-primary"
                  type="button"
                  disabled={busy || !noteId || (Boolean(request.selectedText) && !requirement.trim())}
                  onClick={() => onPrepare(noteId, requirement)}
                >
                  生成修改预览
                </button>
              </div>
            </footer>
          </>
        ) : null}
      </section>
    </div>
  );
}

function DiffHunkRow({
  hunk,
  selected,
  onChange,
}: {
  hunk: NoteDiffHunk;
  selected: boolean;
  onChange: (selected: boolean) => void;
}) {
  return (
    <label className={`note-edit-hunk${selected ? " is-selected" : ""}`}>
      <div className="note-edit-hunk-heading">
        <input type="checkbox" checked={selected} onChange={(event) => onChange(event.target.checked)} />
        <strong>第 {hunk.id + 1} 段</strong>
        <span>−{hunk.oldLines} / +{hunk.newLines} 行</span>
      </div>
      <div className="note-edit-hunk-columns">
        <pre className="note-edit-hunk-old">{hunk.oldText || "（无原文）"}</pre>
        <pre className="note-edit-hunk-new">{hunk.newText || "（删除该段）"}</pre>
      </div>
    </label>
  );
}
