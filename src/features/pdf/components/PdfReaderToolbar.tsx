import {
  ArrowDown,
  ArrowUp,
  BookOpen,
  ExternalLink,
  Minus,
  MoveHorizontal,
  Plus,
  ScanLine,
  Search,
} from "lucide-react";
import type { LibraryAnnotationColor } from "../../library/types";
import type { TranslationKey } from "../../../i18n/translations";
import { PDF_ANNOTATION_COLORS } from "../types";

type Translate = (key: TranslationKey, values?: Record<string, string | number>) => string;

type PdfReaderToolbarProps = {
  title: string;
  pdfAvailable: boolean;
  currentPage: number;
  pageCount: number;
  zoom: number;
  annotationMode: "text" | "area";
  annotationColor: LibraryAnnotationColor;
  searchOpen: boolean;
  t: Translate;
  onPageSubmit: (value: string) => void;
  onAnnotationModeChange: (mode: "text" | "area") => void;
  onAnnotationColorChange: (color: LibraryAnnotationColor) => void;
  onZoomChange: (zoom: number) => void;
  onOpenExternal: () => void;
  onSearchToggle: () => void;
};

/** PDF 顶部阅读控制条不持有文档对象，避免阅读器容器同时承担状态与大段展示代码。 */
export function PdfReaderToolbar({
  title,
  pdfAvailable,
  currentPage,
  pageCount,
  zoom,
  annotationMode,
  annotationColor,
  searchOpen,
  t,
  onPageSubmit,
  onAnnotationModeChange,
  onAnnotationColorChange,
  onZoomChange,
  onOpenExternal,
  onSearchToggle,
}: PdfReaderToolbarProps) {
  return (
    <header className="mnemora-pdf-toolbar">
      <div className="mnemora-pdf-toolbar-group">
        <button className="icon-button" type="button" title={t("common.previous")} aria-label={t("common.previous")} disabled={!pdfAvailable || currentPage <= 1} onClick={() => onPageSubmit(String(currentPage - 1))}>
          <ArrowUp size={16} />
        </button>
        <label className="mnemora-pdf-page-input">
          <input
            key={currentPage}
            type="number"
            min={1}
            max={Math.max(1, pageCount)}
            defaultValue={currentPage}
            aria-label={t("pdf.currentPage")}
            disabled={!pdfAvailable}
            onKeyDown={(event) => { if (event.key === "Enter") onPageSubmit(event.currentTarget.value); }}
          />
          <span>/ {pageCount || "-"}</span>
        </label>
        <button className="icon-button" type="button" title={t("common.next")} aria-label={t("common.next")} disabled={!pdfAvailable || currentPage >= pageCount} onClick={() => onPageSubmit(String(currentPage + 1))}>
          <ArrowDown size={16} />
        </button>
      </div>

      <div className="mnemora-pdf-toolbar-title" title={title}><BookOpen size={16} /><span>{title}</span></div>

      <div className="mnemora-pdf-toolbar-group">
        <div className="mnemora-pdf-annotation-colors" aria-label={t("pdf.annotationColor")}>
          {PDF_ANNOTATION_COLORS.map((color) => (
            <button
              className={`mnemora-pdf-color-swatch mnemora-pdf-color-${color.id}${annotationColor === color.id ? " is-active" : ""}`}
              type="button"
              title={color.label}
              aria-label={t("pdf.annotationColorLabel", { color: color.label })}
              aria-pressed={annotationColor === color.id}
              disabled={!pdfAvailable}
              key={color.id}
              onClick={() => onAnnotationColorChange(color.id)}
            />
          ))}
        </div>
        <button className={`icon-button${annotationMode === "area" ? " is-active" : ""}`} type="button" title={annotationMode === "area" ? t("pdf.exitArea") : t("pdf.areaAnnotation")} aria-label={annotationMode === "area" ? t("pdf.exitArea") : t("pdf.areaAnnotation")} aria-pressed={annotationMode === "area"} disabled={!pdfAvailable} onClick={() => onAnnotationModeChange(annotationMode === "area" ? "text" : "area")}>
          <ScanLine size={16} />
        </button>
        <button className="icon-button" type="button" title={t("pdf.zoomOut")} aria-label={t("pdf.zoomOut")} disabled={!pdfAvailable} onClick={() => onZoomChange(zoom - 0.1)}><Minus size={16} /></button>
        <span className="mnemora-pdf-zoom-label">{Math.round(zoom * 100)}%</span>
        <button className="icon-button" type="button" title={t("pdf.zoomIn")} aria-label={t("pdf.zoomIn")} disabled={!pdfAvailable} onClick={() => onZoomChange(zoom + 0.1)}><Plus size={16} /></button>
        <button className="icon-button" type="button" title={t("pdf.fitWidth")} aria-label={t("pdf.fitWidth")} disabled={!pdfAvailable} onClick={() => onZoomChange(1)}><MoveHorizontal size={16} /></button>
        <button className="icon-button" type="button" title={t("work.openSystem")} aria-label={t("work.openSystem")} onClick={onOpenExternal}><ExternalLink size={16} /></button>
        <button className={`icon-button${searchOpen ? " is-active" : ""}`} type="button" title={t("pdf.search")} aria-label={t("pdf.search")} aria-pressed={searchOpen} disabled={!pdfAvailable} onClick={onSearchToggle}><Search size={16} /></button>
      </div>
    </header>
  );
}
