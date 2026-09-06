import { useNoteText } from "../editor/noteText";
import { useEffect, useState } from "react";
import { diffLines } from "diff";
import { X, RotateCcw, Pin, Download, FilePlus2 } from "lucide-react";
import { noteEditingApi, type NoteVersionEntry } from "../api/noteEditing";
import type { NoteEditSession } from "../runtime/noteEditSession";

export function downloadNoteText(title: string, text: string, extension = "md", mime = "text/markdown;charset=utf-8") {
  const url = URL.createObjectURL(new Blob([text], { type: mime }));
  const link = document.createElement("a");
  link.href = url; link.download = `${title.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_") || "note"}.${extension}`;
  document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
}
export function NoteHistoryPanel({ session, onClose, onRestoreMode }: { session: NoteEditSession; onClose: () => void; onRestoreMode: () => void }) {
  const nt = useNoteText();
  const [versions, setVersions] = useState<NoteVersionEntry[]>([]);
  const [selected, setSelected] = useState<NoteVersionEntry | null>(null);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    void noteEditingApi.versions(session.noteId).then((items) => { if (active) { setVersions(items); setSelected(items[0] ?? null); } }).catch((error: unknown) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, [session]);
  const run = async (action: () => Promise<unknown>) => { setBusy(true); setError(""); try { await action(); } catch (error) { setError(String(error)); } finally { setBusy(false); } };
  return <section className="note-history" aria-label={nt("笔记版本历史")}>
    <header><strong>{nt("版本历史")}</strong><button type="button" onClick={onClose} aria-label={nt("关闭版本历史")} title={nt("关闭版本历史")}><X size={16} /></button></header>
    {error ? <p role="alert">{error}</p> : null}
    {status ? <p role="status">{status}</p> : null}
    <div className="note-history-body">
      <nav>{versions.length ? versions.map((version) => <button type="button" key={version.id} aria-pressed={selected?.id === version.id} onClick={() => setSelected(version)}>
        <span>{new Date(version.createdAt).toLocaleString()}</span><small>{version.pinned ? nt("固定 · ") : ""}{version.reason}</small>
      </button>) : <p>{nt("暂无历史版本")}</p>}</nav>
      {selected ? <div className="note-history-detail">
        <div className="note-block-tools">
          <button type="button" disabled={busy} title={nt("恢复此版本")} aria-label={nt("恢复此版本")} onClick={() => void run(async () => {
            await session.save(); onRestoreMode(); session.edit({ title: selected.title, content: selected.content }); await session.save("restore"); onClose();
          })}><RotateCcw size={15} /></button>
          <button type="button" disabled={busy} title={nt("固定版本")} aria-label={nt("固定版本")} aria-pressed={selected.pinned} onClick={() => void run(async () => {
            await noteEditingApi.pin(session.noteId, selected.id, !selected.pinned);
            const next = { ...selected, pinned: !selected.pinned }; setSelected(next); setVersions((items) => items.map((item) => item.id === next.id ? next : item));
          })}><Pin size={15} /></button>
          <button type="button" title={nt("导出版本")} aria-label={nt("导出版本")} onClick={() => downloadNoteText(selected.title, selected.content)}><Download size={15} /></button>
          <button type="button" disabled={busy} title={nt("另存为新笔记")} aria-label={nt("另存为新笔记")} onClick={() => void run(async () => {
            const note = await noteEditingApi.copyVersion(session.noteId, selected.id);
            setStatus(`${nt("已另存为")}：${note.title}`);
          })}><FilePlus2 size={15} /></button>
        </div>
        <pre className="note-diff">{diffLines(selected.content, session.snapshot().content).map((part, index) =>
          <span key={index} data-diff={part.added ? "added" : part.removed ? "removed" : undefined}>{part.value}</span>)}</pre>
      </div> : null}
    </div>
  </section>;
}
