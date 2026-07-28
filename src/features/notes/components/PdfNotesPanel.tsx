import { useEffect, useState } from "react";
import {
  ExternalLink,
  LoaderCircle,
  NotebookPen,
  Plus,
  Trash2,
} from "lucide-react";
import { usePdfReaderBridge } from "../../pdf/context/PdfReaderContext";
import "../styles/notes.css";

export function PdfNotesPanel() {
  const { controller } = usePdfReaderBridge();
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    if (!controller || controller.notesLoaded || controller.notesLoading) return;
    void controller.loadNotes();
  }, [controller?.itemId, controller?.loadNotes]);

  if (!controller) {
    return (
      <section className="work-context-empty" aria-label="暂无关联笔记">
        <NotebookPen size={28} aria-hidden="true" />
        <h2>暂无关联笔记</h2>
        <p>当前没有打开 PDF</p>
      </section>
    );
  }

  const create = async () => {
    const normalizedTitle = title.trim() || "学习笔记";
    if (creating) return;
    setCreating(true);
    try {
      await controller.createNote(normalizedTitle, content);
      setTitle("");
      setContent("");
    } catch {
      // 错误由 Reader Bridge 统一展示。
    } finally {
      setCreating(false);
    }
  };

  return (
    <section className="pdf-notes-panel" aria-label="当前文献笔记">
      <header>
        <NotebookPen size={17} />
        <strong>笔记</strong>
        <span>{controller.notes.length}</span>
      </header>

      <form onSubmit={(event) => {
        event.preventDefault();
        void create();
      }}>
        <input
          value={title}
          maxLength={500}
          placeholder="笔记标题"
          aria-label="笔记标题"
          disabled={!controller.notesLoaded || controller.notesLoading}
          onChange={(event) => setTitle(event.target.value)}
        />
        <textarea
          value={content}
          maxLength={500_000}
          rows={5}
          placeholder="记录内容"
          aria-label="笔记内容"
          disabled={!controller.notesLoaded || controller.notesLoading}
          onChange={(event) => setContent(event.target.value)}
        />
        <button
          type="submit"
          disabled={creating || !controller.notesLoaded || (!title.trim() && !content.trim())}
        >
          {creating ? <LoaderCircle className="is-spinning" size={15} /> : <Plus size={15} />}
          <span>{creating ? "正在创建" : "新建笔记"}</span>
        </button>
      </form>

      {controller.noteError ? (
        <div className="pdf-panel-error pdf-notes-error" role="alert">
          <span>{controller.noteError}</span>
          <button type="button" onClick={() => void controller.loadNotes()}>重试</button>
        </div>
      ) : null}

      <div className="pdf-notes-list">
        {controller.notesLoading || !controller.notesLoaded ? (
          <div className="pdf-panel-loading" role="status">
            <LoaderCircle size={18} />
            <span>正在读取笔记</span>
          </div>
        ) : controller.notes.length === 0 ? (
          <div className="pdf-panel-empty" role="status">
            <NotebookPen size={24} />
            <span>暂无关联笔记</span>
          </div>
        ) : controller.notes.map((note) => (
          <article className="pdf-note-list-item" key={note.id}>
            <button className="pdf-note-list-main" type="button" onClick={() => controller.openNote(note)}>
              <strong>{note.title}</strong>
              <span>{note.contentPreview || "空笔记"}</span>
              <small>{formatTime(note.updatedAt)}</small>
            </button>
            <div>
              <button type="button" title="在工作区打开" aria-label={`打开 ${note.title}`} onClick={() => controller.openNote(note)}>
                <ExternalLink size={14} />
              </button>
              <button
                type="button"
                title="删除笔记"
                aria-label={`删除 ${note.title}`}
                onClick={() => {
                  if (!window.confirm(`删除笔记“${note.title}”吗？`)) return;
                  void controller.deleteNote(note.id).catch(() => undefined);
                }}
              >
                <Trash2 size={14} />
              </button>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function formatTime(timestamp: number) {
  return timestamp > 0
    ? new Intl.DateTimeFormat("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }).format(timestamp)
    : "";
}

export default PdfNotesPanel;
