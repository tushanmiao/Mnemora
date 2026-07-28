import { useEffect, useMemo, useState } from "react";
import { FileText, LoaderCircle, NotebookPen, Trash2 } from "lucide-react";
import {
  deleteLibraryNote,
  listLibraryNotes,
} from "../../library/api/library";
import type { LibraryNote, LibraryNoteSummary } from "../../library/types";
import "../styles/notes.css";

type NoteListViewProps = {
  searchQuery: string;
  onOpenNote: (note: Pick<LibraryNote, "id" | "title">) => void;
  onCountChange: (count: number) => void;
};

export function NoteListView({ searchQuery, onOpenNote, onCountChange }: NoteListViewProps) {
  const [notes, setNotes] = useState<LibraryNoteSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    setError("");
    void listLibraryNotes()
      .then((next) => {
        if (!disposed) setNotes(next);
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
  }, []);

  const filtered = useMemo(() => {
    const query = searchQuery.trim().toLocaleLowerCase();
    if (!query) return notes;
    return notes.filter((note) => (
      note.title.toLocaleLowerCase().includes(query)
      || note.contentPreview.toLocaleLowerCase().includes(query)
      || note.itemTitle.toLocaleLowerCase().includes(query)
    ));
  }, [notes, searchQuery]);

  useEffect(() => {
    onCountChange(notes.length);
  }, [notes.length, onCountChange]);

  useEffect(() => () => onCountChange(0), [onCountChange]);

  if (loading) {
    return <div className="work-library-state" role="status"><LoaderCircle className="work-library-spinner" size={24} /><span>正在读取笔记</span></div>;
  }
  if (error) {
    return <div className="work-library-state work-library-error" role="alert"><strong>笔记暂时不可用</strong><span>{error}</span></div>;
  }
  if (filtered.length === 0) {
    return <div className="work-library-empty" role="status"><NotebookPen size={34} /><h2>{searchQuery.trim() ? `没有找到“${searchQuery.trim()}”` : "暂无学习笔记"}</h2><p>0 项</p></div>;
  }

  return (
    <div className="note-library-list" role="list" aria-label="学习笔记">
      {filtered.map((note) => (
        <article className="note-library-row" role="listitem" key={note.id}>
          <button type="button" onDoubleClick={() => onOpenNote(note)} onClick={() => onOpenNote(note)}>
            <NotebookPen size={16} />
            <span>
              <strong>{note.title}</strong>
              <small>{note.contentPreview || "空笔记"}</small>
            </span>
          </button>
          <span title={note.itemTitle}><FileText size={14} />{note.itemTitle}</span>
          <time dateTime={new Date(note.updatedAt).toISOString()}>{formatDate(note.updatedAt)}</time>
          <button
            className="note-library-delete"
            type="button"
            title="删除笔记"
            aria-label={`删除 ${note.title}`}
            onClick={() => {
              if (!window.confirm(`删除笔记“${note.title}”吗？`)) return;
              void deleteLibraryNote(note.id)
                .then((removed) => {
                  if (removed) setNotes((current) => current.filter((item) => item.id !== note.id));
                })
                .catch((deleteError) => setError(deleteError instanceof Error ? deleteError.message : String(deleteError)));
            }}
          >
            <Trash2 size={14} />
          </button>
        </article>
      ))}
    </div>
  );
}

function formatDate(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit" }).format(timestamp);
}

export default NoteListView;
