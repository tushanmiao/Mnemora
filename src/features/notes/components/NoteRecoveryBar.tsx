import { useNoteText } from "../editor/noteText";
import { useState } from "react";
import type { NoteEditSession, NoteSessionState } from "../runtime/noteEditSession";
import { downloadNoteText } from "./NoteHistoryPanel";

export function NoteRecoveryBar({ session, state, onRestoreMode }: { session: NoteEditSession; state: NoteSessionState; onRestoreMode: () => void }) {
  const nt = useNoteText();
  const [error, setError] = useState("");
  const run = async (action: () => Promise<void>) => { setError(""); try { await action(); } catch (error) { setError(String(error)); } };
  const draft = state.base?.drafts.find((draft) => draft.sessionId !== session.sessionId);
  const conflict = state.conflict;
  const images = [...new Map((state.base?.stagedImages ?? []).filter((image) => !state.content.includes(image.relativePath)).map((image) => [image.relativePath, image])).values()];
  if (!draft && !conflict && !state.error && !error && !images.length) return null;
  return <div className="note-recovery" role="status">
    {state.error || error ? <p>{error || state.error}</p> : null}
    {conflict ? <details open><summary>{nt("笔记版本冲突")}</summary>
      <div className="note-conflict-columns">
        <label>{nt("共同基线")}<textarea readOnly value={state.base?.note.content ?? ""} /></label>
        <label>{nt("本地草稿")}<textarea readOnly value={state.content} /></label>
        <label>{nt("当前文件")}<textarea readOnly value={conflict.externalContent ?? conflict.note.content} /></label>
      </div>
      <button type="button" onClick={() => { onRestoreMode(); void run(() => session.resolve("local", conflict)); }}>{nt("保留本地")}</button>
      <button type="button" onClick={() => { onRestoreMode(); void run(() => session.resolve("disk", conflict)); }}>{nt("采用文件")}</button>
      <button type="button" onClick={() => downloadNoteText(state.title, state.content)}>{nt("导出草稿")}</button>
    </details> : draft ? <>
      <span>{nt("恢复草稿 ·")}{new Date(draft.updatedAt).toLocaleString()}</span>
      <button type="button" onClick={() => { onRestoreMode(); void run(() => session.recover(draft)); }}>{nt("恢复")}</button>
      <button type="button" onClick={() => downloadNoteText(draft.title, draft.content)}>{nt("导出")}</button>
      <button type="button" onClick={() => void run(() => session.discard(draft))}>{nt("丢弃")}</button>
    </> : state.error || error ? <>
      <button type="button" onClick={() => void run(() => session.save())}>{nt("重试保存")}</button>
      <button type="button" onClick={() => downloadNoteText(state.title, state.content)}>{nt("导出草稿")}</button>
    </> : null}
    {images.length ? <details><summary>{nt("未插入的已保留图片")} · {images.length}</summary>{images.map((image) => <button type="button" key={image.token} onClick={() => {
      onRestoreMode(); session.edit({ content: `${state.content}\n\n![](${image.relativePath})\n` }); void run(() => session.checkpoint());
    }}>{nt("恢复图片")} · {image.contentHash.slice(0, 8)}</button>)}</details> : null}
  </div>;
}
