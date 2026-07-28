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
import "../styles/pdf-annotations.css";

const annotationLabels = {
  highlight: { label: "高亮", icon: Highlighter },
  underline: { label: "下划线", icon: Underline },
  area: { label: "区域", icon: ScanLine },
} as const;

export function PdfAnnotationsPanel() {
  const { controller } = usePdfReaderBridge();
  if (!controller) {
    return (
      <section className="work-context-empty" aria-label="暂无 PDF 批注">
        <Highlighter size={28} aria-hidden="true" />
        <h2>暂无批注</h2>
        <p>当前没有打开 PDF</p>
      </section>
    );
  }

  return (
    <section className="pdf-annotations-panel" aria-label="PDF 批注">
      <header>
        <div>
          <Highlighter size={17} />
          <strong>批注</strong>
          <span>{controller.annotations.length}</span>
        </div>
        <button
          className={controller.annotationMode === "area" ? "is-active" : ""}
          type="button"
          title={controller.annotationMode === "area" ? "退出区域批注" : "区域批注"}
          aria-label={controller.annotationMode === "area" ? "退出区域批注" : "区域批注"}
          aria-pressed={controller.annotationMode === "area"}
          onClick={() => controller.setAnnotationMode(
            controller.annotationMode === "area" ? "text" : "area",
          )}
        >
          <ScanLine size={16} />
        </button>
      </header>

      <div className="pdf-annotation-panel-colors" aria-label="新批注颜色">
        {PDF_ANNOTATION_COLORS.map((color) => (
          <button
            className={`mnemora-pdf-color-swatch mnemora-pdf-color-${color.id}${controller.annotationColor === color.id ? " is-active" : ""}`}
            type="button"
            title={color.label}
            aria-label={`${color.label}批注`}
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
            <span>正在读取批注</span>
          </div>
        ) : controller.annotations.length === 0 ? (
          <div className="pdf-panel-empty" role="status">
            <Highlighter size={24} />
            <span>暂无批注</span>
          </div>
        ) : controller.annotations.map((annotation) => (
          <AnnotationItem annotation={annotation} key={annotation.id} />
        ))}
      </div>
    </section>
  );
}

function AnnotationItem({ annotation }: { annotation: LibraryAnnotation }) {
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
  const details = annotationLabels[annotation.kind];
  const Icon = details.icon;
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
      : `区域批注，第 ${annotation.pageIndex + 1} 页。\n\n`;
    try {
      const note = await controller.createNote(
        `第 ${annotation.pageIndex + 1} 页批注`,
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
          title={`定位到第 ${annotation.pageIndex + 1} 页`}
          onClick={() => controller.goToAnnotation(annotation)}
        >
          <Icon size={14} />
          <span>{details.label}</span>
          <strong>第 {annotation.pageIndex + 1} 页</strong>
          <MapPin size={13} />
        </button>
      </header>

      {annotation.text ? <blockquote>{annotation.text}</blockquote> : null}

      <div className="pdf-annotation-item-colors" aria-label="批注颜色">
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
        placeholder="添加评论"
        aria-label="批注评论"
        onChange={(event) => setComment(event.target.value)}
      />

      <footer>
        <button type="button" title="保存评论" disabled={!dirty || saving} onClick={() => void save()}>
          {saving ? <LoaderCircle size={14} className="is-spinning" /> : <Save size={14} />}
          <span>保存</span>
        </button>
        <button type="button" title="转为笔记" disabled={creatingNote} onClick={() => void createNote()}>
          <NotebookPen size={14} />
          <span>笔记</span>
        </button>
        <button
          className="is-danger"
          type="button"
          title="删除批注"
          onClick={() => {
            if (!window.confirm("删除这条批注吗？")) return;
            void controller.deleteAnnotation(annotation.id).catch(() => undefined);
          }}
        >
          <Trash2 size={14} />
          <span>删除</span>
        </button>
      </footer>
    </article>
  );
}

export default PdfAnnotationsPanel;
