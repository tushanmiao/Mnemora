import { useEffect, useRef, useState, type RefObject } from "react";
import { BookOpenText, ChevronRight, Files } from "lucide-react";
import type { PDFDocumentProxy, PDFPageProxy, RenderTask } from "pdfjs-dist";
import { usePdfReaderBridge, type PdfOutlineEntry } from "../context/PdfReaderContext";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/pdf-navigator.css";

type NavigatorMode = "outline" | "thumbnails";

export function PdfNavigatorPanel() {
  const { t } = useI18n();
  const { controller } = usePdfReaderBridge();
  const [mode, setMode] = useState<NavigatorMode>("outline");
  const scrollRef = useRef<HTMLDivElement>(null);

  if (!controller) {
    return (
      <section className="pdf-navigator-empty" role="status">
        <BookOpenText size={26} />
        <h2>{t("pdf.noActive")}</h2>
        <p>{t("pdf.noActiveDescription")}</p>
      </section>
    );
  }

  const openOutlineEntry = async (entry: PdfOutlineEntry) => {
    const pageIndex = await resolveDestinationPage(controller.pdf, entry.dest);
    if (pageIndex !== null) controller.goToPage(pageIndex);
  };

  return (
    <section className="pdf-navigator" aria-label={t("pdf.navigation")}>
      <div className="pdf-navigator-tabs" role="tablist" aria-label={t("pdf.navigationMode")}>
        <button
          className={mode === "outline" ? "is-active" : ""}
          type="button"
          role="tab"
          aria-selected={mode === "outline"}
          onClick={() => setMode("outline")}
        >
          <BookOpenText size={15} />
          <span>{t("pdf.outline")}</span>
        </button>
        <button
          className={mode === "thumbnails" ? "is-active" : ""}
          type="button"
          role="tab"
          aria-selected={mode === "thumbnails"}
          onClick={() => setMode("thumbnails")}
        >
          <Files size={15} />
          <span>{t("pdf.thumbnails")}</span>
        </button>
      </div>

      <div className="pdf-navigator-scroll" ref={scrollRef}>
        {mode === "outline" ? (
          controller.outline.length > 0 ? (
            <div className="pdf-outline-tree">
              {controller.outline.map((entry) => (
                <OutlineEntry key={entry.id} entry={entry} onOpen={openOutlineEntry} />
              ))}
            </div>
          ) : (
            <div className="pdf-navigator-empty" role="status">
              <BookOpenText size={24} />
              <h2>{t("pdf.noOutline")}</h2>
              <p>{t("pdf.noOutlineDescription")}</p>
            </div>
          )
        ) : (
          <div className="pdf-thumbnail-list">
            {Array.from({ length: controller.pageCount }, (_, pageIndex) => (
              <PdfThumbnail
                key={pageIndex}
                pdf={controller.pdf}
                pageIndex={pageIndex}
                active={controller.currentPage === pageIndex + 1}
                scrollRootRef={scrollRef}
                onOpen={() => controller.goToPage(pageIndex)}
              />
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function OutlineEntry({
  entry,
  onOpen,
}: {
  entry: PdfOutlineEntry;
  onOpen: (entry: PdfOutlineEntry) => Promise<void>;
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(entry.level < 1);
  const hasChildren = entry.children.length > 0;
  return (
    <div className="pdf-outline-entry">
      <div className="pdf-outline-entry-row" style={{ paddingInlineStart: `${entry.level * 14}px` }}>
        {hasChildren ? (
          <button
            className={`pdf-outline-expand${expanded ? " is-expanded" : ""}`}
            type="button"
            title={expanded ? t("pdf.collapseSection") : t("pdf.expandSection")}
            aria-label={expanded ? t("pdf.collapseSection") : t("pdf.expandSection")}
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
          >
            <ChevronRight size={13} />
          </button>
        ) : <span className="pdf-outline-spacer" />}
        <button className="pdf-outline-title" type="button" title={entry.title} onClick={() => void onOpen(entry)}>
          {entry.title}
        </button>
      </div>
      {expanded ? entry.children.map((child) => (
        <OutlineEntry key={child.id} entry={child} onOpen={onOpen} />
      )) : null}
    </div>
  );
}

function PdfThumbnail({
  pdf,
  pageIndex,
  active,
  scrollRootRef,
  onOpen,
}: {
  pdf: PDFDocumentProxy;
  pageIndex: number;
  active: boolean;
  scrollRootRef: RefObject<HTMLDivElement | null>;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const wrapperRef = useRef<HTMLButtonElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const wrapper = wrapperRef.current;
    if (!wrapper) return;
    if (typeof IntersectionObserver === "undefined") {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(
      ([entry]) => setVisible(entry.isIntersecting),
      { root: scrollRootRef.current, rootMargin: "500px 0px", threshold: 0 },
    );
    observer.observe(wrapper);
    return () => observer.disconnect();
  }, [pdf, pageIndex, scrollRootRef]);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    let page: PDFPageProxy | null = null;
    let renderTask: RenderTask | null = null;
    const canvas = canvasRef.current;

    const render = async () => {
      if (!canvas) return;
      try {
        page = await pdf.getPage(pageIndex + 1);
        if (cancelled) return;
        const baseViewport = page.getViewport({ scale: 1 });
        const viewport = page.getViewport({ scale: 124 / baseViewport.width });
        const ratio = Math.min(window.devicePixelRatio || 1, 1.5);
        canvas.width = Math.max(1, Math.floor(viewport.width * ratio));
        canvas.height = Math.max(1, Math.floor(viewport.height * ratio));
        canvas.style.width = `${viewport.width}px`;
        canvas.style.height = `${viewport.height}px`;
        const context = canvas.getContext("2d", { alpha: false });
        if (!context) return;
        renderTask = page.render({
          canvas,
          canvasContext: context,
          viewport,
          transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
        });
        await renderTask.promise;
      } catch (error) {
        if (!cancelled && error instanceof Error && error.name !== "RenderingCancelledException") {
          canvas.width = 0;
          canvas.height = 0;
        }
      }
    };

    void render();
    return () => {
      cancelled = true;
      renderTask?.cancel();
      if (canvas) {
        canvas.width = 0;
        canvas.height = 0;
      }
      if (page) page.cleanup();
    };
  }, [pdf, pageIndex, visible]);

  return (
    <button
      className={`pdf-thumbnail${active ? " is-active" : ""}`}
      type="button"
      ref={wrapperRef}
      title={t("pdf.goPage", { page: pageIndex + 1 })}
      aria-current={active ? "page" : undefined}
      onClick={onOpen}
    >
      <span className="pdf-thumbnail-canvas-wrap"><canvas ref={canvasRef} /></span>
      <span>{t("pdf.page", { page: pageIndex + 1 })}</span>
    </button>
  );
}

async function resolveDestinationPage(
  pdf: PDFDocumentProxy,
  destination: string | unknown[] | null,
): Promise<number | null> {
  if (!destination) return null;
  const resolved = typeof destination === "string"
    ? await pdf.getDestination(destination)
    : destination;
  if (!resolved || resolved.length === 0) return null;
  const target = resolved[0];
  if (typeof target === "number" && Number.isInteger(target)) return target;
  if (
    target
    && typeof target === "object"
    && "num" in target
    && "gen" in target
    && typeof target.num === "number"
    && typeof target.gen === "number"
  ) {
    return pdf.getPageIndex({ num: target.num, gen: target.gen });
  }
  return null;
}

export default PdfNavigatorPanel;
