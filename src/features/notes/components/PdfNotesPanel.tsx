import { useEffect, useState } from "react";
import {
  ExternalLink,
  LoaderCircle,
  NotebookPen,
  Plus,
  Trash2,
} from "lucide-react";
import { usePdfReaderBridge } from "../../pdf/context/PdfReaderContext";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/notes.css";

export function PdfNotesPanel() {
  const { language, t } = useI18n();
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
      <section className="work-context-empty" aria-label={t("notes.noLinked")}>
        <NotebookPen size={28} aria-hidden="true" />
        <h2>{t("notes.noLinked")}</h2>
        <p>{t("notes.noOpenPdf")}</p>
      </section>
    );
  }

  const create = async () => {
    const normalizedTitle = title.trim() || t("notes.defaultTitle");
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
    <section className="pdf-notes-panel" aria-label={t("notes.currentLiterature")}>
      <header>
        <NotebookPen size={17} />
        <strong>{t("work.notes")}</strong>
        <span>{controller.notes.length}</span>
      </header>

      <form onSubmit={(event) => {
        event.preventDefault();
        void create();
      }}>
        <input
          value={title}
          maxLength={500}
          placeholder={t("notes.title")}
          aria-label={t("notes.title")}
          disabled={!controller.notesLoaded || controller.notesLoading}
          onChange={(event) => setTitle(event.target.value)}
        />
        <textarea
          value={content}
          maxLength={500_000}
          rows={5}
          placeholder={t("notes.contentPlaceholder")}
          aria-label={t("notes.content")}
          disabled={!controller.notesLoaded || controller.notesLoading}
          onChange={(event) => setContent(event.target.value)}
        />
        <button
          type="submit"
          disabled={creating || !controller.notesLoaded || (!title.trim() && !content.trim())}
        >
          {creating ? <LoaderCircle className="is-spinning" size={15} /> : <Plus size={15} />}
          <span>{creating ? t("notes.creating") : t("notes.create")}</span>
        </button>
      </form>

      {controller.noteError ? (
        <div className="pdf-panel-error pdf-notes-error" role="alert">
          <span>{controller.noteError}</span>
          <button type="button" onClick={() => void controller.loadNotes()}>{t("common.retry")}</button>
        </div>
      ) : null}

      <div className="pdf-notes-list">
        {controller.notesLoading || !controller.notesLoaded ? (
          <div className="pdf-panel-loading" role="status">
            <LoaderCircle size={18} />
            <span>{t("notes.loading")}</span>
          </div>
        ) : controller.notes.length === 0 ? (
          <div className="pdf-panel-empty" role="status">
            <NotebookPen size={24} />
            <span>{t("notes.noLinked")}</span>
          </div>
        ) : controller.notes.map((note) => (
          <article className="pdf-note-list-item" key={note.id}>
            <button className="pdf-note-list-main" type="button" onClick={() => controller.openNote(note)}>
              <strong>{note.title}</strong>
              <span>{note.contentPreview || t("notes.empty")}</span>
              <small>{formatTime(note.updatedAt, language)}</small>
            </button>
            <div>
              <button type="button" title={t("notes.openWorkspace")} aria-label={t("notes.openNamed", { title: note.title })} onClick={() => controller.openNote(note)}>
                <ExternalLink size={14} />
              </button>
              <button
                type="button"
                title={t("notes.delete")}
                aria-label={t("notes.deleteNamed", { title: note.title })}
                onClick={() => {
                  if (!window.confirm(t("notes.deleteConfirm", { title: note.title }))) return;
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

function formatTime(timestamp: number, language: "zh" | "en") {
  return timestamp > 0
    ? new Intl.DateTimeFormat(language === "en" ? "en-US" : "zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }).format(timestamp)
    : "";
}

export default PdfNotesPanel;
