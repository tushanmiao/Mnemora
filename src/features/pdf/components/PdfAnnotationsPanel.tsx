import { useEffect, useState } from "react";
import {
  Highlighter,
  LoaderCircle,
  MapPin,
  NotebookPen,
  Save,
  ScanLine,
  Trash2,
  Underline,
} from "lucide-react";
import type {
  LibraryAnnotation,
  LibraryAnnotationColor,
} from "../../library/types";
import { PDF_ANNOTATION_COLORS } from "../types";
import { usePdfReaderBridge } from "../context/PdfReaderContext";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/pdf-annotations.css";

export function PdfAnnotationsPanel() {
  const { t } = useI18n();
  const { controller } = usePdfReaderBridge();
  if (!controller) {
    return (
      <section className="work-context-empty" aria-label={t("pdf.noAnnotationLabel")}>
        <Highlighter size={28} aria-hidden="true" />
        <h2>{t("pdf.noAnnotations")}</h2>
        <p>{t("pdf.noOpen")}</p>
      </section>
    );
  }

  return (
    <section className="pdf-annotations-panel" aria-label={t("pdf.annotationPanel")}>
      <header>
        <div>
          <Highlighter size={17} />
          <strong>{t("pdf.annotations")}</strong>
          <span>{controller.annotations.length}</span>
        </div>
        <button
          className={controller.annotationMode === "area" ? "is-active" : ""}
          type="button"
          title={controller.annotationMode === "area" ? t("pdf.exitArea") : t("pdf.areaAnnotation")}
          aria-label={controller.annotationMode === "area" ? t("pdf.exitArea") : t("pdf.areaAnnotation")}
          aria-pressed={controller.annotationMode === "area"}
          onClick={() => controller.setAnnotationMode(
            controller.annotationMode === "area" ? "text" : "area",
          )}
        >
          <ScanLine size={16} />
        </button>
      </header>

      <div className="pdf-annotation-panel-colors" aria-label={t("pdf.newAnnotationColor")}>
        {PDF_ANNOTATION_COLORS.map((color) => (
          <button
            className={`mnemora-pdf-color-swatch mnemora-pdf-color-${color.id}${controller.annotationColor === color.id ? " is-active" : ""}`}
            type="button"
            title={color.label}
            aria-label={t("pdf.annotationColorLabel", { color: color.label })}
            aria-pressed={controller.annotationColor === color.id}
            key={color.id}
            onClick={() => controller.setAnnotationColor(color.id)}
          />
        ))}
      </div>

      {controller.annotationError ? (
        <p className="pdf-panel-error" role="alert">{controller.annotationError}</p>
      ) : null}

      <div className="pdf-annotations-list">
        {controller.annotationsLoading ? (
          <div className="pdf-panel-loading" role="status">
            <LoaderCircle size={18} />
            <span>{t("pdf.loadingAnnotations")}</span>
          </div>
        ) : controller.annotations.length === 0 ? (
          <div className="pdf-panel-empty" role="status">
            <Highlighter size={24} />
            <span>{t("pdf.noAnnotations")}</span>
          </div>
        ) : controller.annotations.map((annotation) => (
          <AnnotationItem annotation={annotation} key={annotation.id} />
        ))}
      </div>
    </section>
  );
}

function AnnotationItem({ annotation }: { annotation: LibraryAnnotation }) {
  const { t } = useI18n();
  const { controller } = usePdfReaderBridge();
  const [comment, setComment] = useState(annotation.comment);
  const [color, setColor] = useState<LibraryAnnotationColor>(annotation.color);
  const [saving, setSaving] = useState(false);
  const [creatingNote, setCreatingNote] = useState(false);

  useEffect(() => {
    setComment(annotation.comment);
    setColor(annotation.color);
  }, [annotation.color, annotation.comment]);

  if (!controller) return null;
  const Icon = annotation.kind === "highlight" ? Highlighter : annotation.kind === "underline" ? Underline : ScanLine;
  const label = t(annotation.kind === "highlight" ? "pdf.highlight" : annotation.kind === "underline" ? "pdf.underline" : "pdf.area");
  const dirty = comment !== annotation.comment || color !== annotation.color;

  const save = async () => {
    if (!dirty || saving) return;
    setSaving(true);
    try {
      await controller.updateAnnotation(annotation.id, color, comment);
    } catch {
      // 错误由 Reader Bridge 统一展示。
    } finally {
      setSaving(false);
    }
  };

  const createNote = async () => {
    if (creatingNote) return;
    setCreatingNote(true);
    const quote = annotation.text
      ? `> ${annotation.text.replace(/\n/g, "\n> ")}\n\n`
      : t("pdf.areaQuote", { page: annotation.pageIndex + 1 });
    try {
      const note = await controller.createNote(
        t("pdf.annotationNoteTitle", { page: annotation.pageIndex + 1 }),
        `${quote}${annotation.comment}`.trim(),
      );
      controller.openNote(note);
    } catch {
      // 错误由 Reader Bridge 统一展示。
    } finally {
      setCreatingNote(false);
    }
  };

  return (
    <article className={`pdf-annotation-item mnemora-pdf-color-${annotation.color}`}>
      <header>
        <button
          type="button"
          title={t("pdf.locatePage", { page: annotation.pageIndex + 1 })}
          onClick={() => controller.goToAnnotation(annotation)}
        >
          <Icon size={14} />
          <span>{label}</span>
          <strong>{t("pdf.page", { page: annotation.pageIndex + 1 })}</strong>
          <MapPin size={13} />
        </button>
      </header>

      {annotation.text ? <blockquote>{annotation.text}</blockquote> : null}

      <div className="pdf-annotation-item-colors" aria-label={t("pdf.annotationColor")}>
        {PDF_ANNOTATION_COLORS.map((option) => (
          <button
            className={`mnemora-pdf-color-swatch mnemora-pdf-color-${option.id}${color === option.id ? " is-active" : ""}`}
            type="button"
            title={option.label}
            aria-label={option.label}
            aria-pressed={color === option.id}
            key={option.id}
            onClick={() => setColor(option.id)}
          />
        ))}
      </div>

      <textarea
        value={comment}
        rows={3}
        maxLength={20_000}
        placeholder={t("pdf.commentPlaceholder")}
        aria-label={t("pdf.annotationComment")}
        onChange={(event) => setComment(event.target.value)}
      />

      <footer>
        <button type="button" title={t("pdf.saveComment")} disabled={!dirty || saving} onClick={() => void save()}>
          {saving ? <LoaderCircle size={14} className="is-spinning" /> : <Save size={14} />}
          <span>{t("common.save")}</span>
        </button>
        <button type="button" title={t("pdf.toNote")} disabled={creatingNote} onClick={() => void createNote()}>
          <NotebookPen size={14} />
          <span>{t("pdf.note")}</span>
        </button>
        <button
          className="is-danger"
          type="button"
          title={t("pdf.deleteAnnotation")}
          onClick={() => {
            if (!window.confirm(t("pdf.deleteAnnotationConfirm"))) return;
            void controller.deleteAnnotation(annotation.id).catch(() => undefined);
          }}
        >
          <Trash2 size={14} />
          <span>{t("common.delete")}</span>
        </button>
      </footer>
    </article>
  );
}

export default PdfAnnotationsPanel;
