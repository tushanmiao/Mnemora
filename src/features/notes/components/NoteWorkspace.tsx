import { useEffect, useState } from "react";
import { FileText, LoaderCircle, Save, Trash2 } from "lucide-react";
import {
  deleteLibraryNote,
  getLibraryNote,
  updateLibraryNote,
} from "../../library/api/library";
import type { LibraryNote } from "../../library/types";
import "../styles/notes.css";

type NoteWorkspaceProps = {
  noteId: string;
  onUpdated: (note: LibraryNote) => void;
  onDeleted: () => void;
};

export function NoteWorkspace({ noteId, onUpdated, onDeleted }: NoteWorkspaceProps) {
  const [note, setNote] = useState<LibraryNote | null>(null);
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    setError("");
    void getLibraryNote(noteId)
      .then((next) => {
        if (disposed) return;
        setNote(next);
        setTitle(next.title);
        setContent(next.content);
      })
      .catch((loadError) => {
        if (!disposed) setError(loadError instanceof Error ? loadError.message : String(loadError));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [noteId]);

  const save = async () => {
    if (!title.trim() || saving) return;
    setSaving(true);
    setError("");
    try {
      const updated = await updateLibraryNote({ noteId, title, content });
      setNote(updated);
      setTitle(updated.title);
      setContent(updated.content);
      onUpdated(updated);
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : String(saveError));
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return <div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>正在打开笔记</span></div>;
  }
  if (!note || error && !note) {
    return <div className="work-library-state work-library-error" role="alert"><strong>笔记无法打开</strong><span>{error || "笔记不存在。"}</span></div>;
  }

  const dirty = title !== note.title || content !== note.content;

  return (
    <section className="note-workspace" aria-label={note.title}>
      <header>
        <div>
          <FileText size={15} />
          <span title={note.itemTitle}>{note.itemTitle}</span>
        </div>
        <div>
          <button type="button" disabled={!dirty || saving || !title.trim()} onClick={() => void save()}>
            {saving ? <LoaderCircle className="is-spinning" size={15} /> : <Save size={15} />}
            <span>{saving ? "正在保存" : "保存"}</span>
          </button>
          <button
            className="is-danger"
            type="button"
            onClick={() => {
              if (!window.confirm(`删除笔记“${note.title}”吗？`)) return;
              void deleteLibraryNote(note.id)
                .then((removed) => {
                  if (removed) onDeleted();
                })
                .catch((deleteError) => setError(deleteError instanceof Error ? deleteError.message : String(deleteError)));
            }}
          >
            <Trash2 size={15} />
            <span>删除</span>
          </button>
        </div>
      </header>
      {error ? <p className="note-workspace-error" role="alert">{error}</p> : null}
      <input
        className="note-workspace-title"
        value={title}
        maxLength={500}
        aria-label="笔记标题"
        onChange={(event) => setTitle(event.target.value)}
      />
      <textarea
        className="note-workspace-content"
        value={content}
        maxLength={500_000}
        aria-label="笔记正文"
        onChange={(event) => setContent(event.target.value)}
      />
    </section>
  );
}

export default NoteWorkspace;
