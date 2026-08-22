import { useEffect, useMemo, useRef, useState } from "react";
import { BookOpenText, ChevronRight, Files } from "lucide-react";
import type { PDFDocumentProxy, PDFPageProxy, RenderTask } from "pdfjs-dist";
import { Virtualizer } from "virtua";
import { usePdfReaderBridge, type PdfOutlineEntry } from "../context/PdfReaderContext";
import { useI18n } from "../../../i18n/I18nProvider";
import type { PdfCanvasBudget } from "../runtime/pdfCanvasBudget";
import type { PdfCanvasLease } from "../runtime/pdfCanvasBudget";
import { PdfRenderScheduler } from "../runtime/pdfRenderScheduler";
import "../styles/pdf-navigator.css";

type NavigatorMode = "outline" | "thumbnails";

export function PdfNavigatorPanel() {
  const { t } = useI18n();
  const { controller } = usePdfReaderBridge();
  const [mode, setMode] = useState<NavigatorMode>("outline");
  const scrollRef = useRef<HTMLDivElement>(null);
  const thumbnailScheduler = useMemo(
    () => new PdfRenderScheduler(),
    [controller?.itemId],
  );
  const thumbnailRows = useMemo(() => (
    Array.from({ length: Math.ceil((controller?.pageCount ?? 0) / 2) }, (_, rowIndex) => (
      [rowIndex * 2, rowIndex * 2 + 1].filter((pageIndex) => pageIndex < (controller?.pageCount ?? 0))
    ))
  ), [controller?.pageCount]);

  useEffect(() => {
    if (mode === "thumbnails" && controller) return undefined;
    thumbnailScheduler.cancelByPrefix("pdf-thumbnail:");
    controller?.canvasBudget.releaseByPrefix("pdf-thumbnail:");
    return undefined;
  }, [controller, mode, thumbnailScheduler]);

  useEffect(() => () => thumbnailScheduler.dispose(), [thumbnailScheduler]);

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
            <Virtualizer data={thumbnailRows} scrollRef={scrollRef} bufferSize={220} itemSize={154}>
              {(row) => (
                <div className="pdf-thumbnail-row">
                  {row.map((pageIndex) => (
                    <PdfThumbnail
                      key={pageIndex}
                      pdf={controller.pdf}
                      pageIndex={pageIndex}
                      active={controller.currentPage === pageIndex + 1}
                      canvasBudget={controller.canvasBudget}
                      renderScheduler={thumbnailScheduler}
                      onOpen={() => controller.goToPage(pageIndex)}
                    />
                  ))}
                </div>
              )}
            </Virtualizer>
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
  canvasBudget,
  renderScheduler,
  onOpen,
}: {
  pdf: PDFDocumentProxy;
  pageIndex: number;
  active: boolean;
  canvasBudget: PdfCanvasBudget;
  renderScheduler: PdfRenderScheduler;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const wrapperRef = useRef<HTMLButtonElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    let cancelled = false;
    let page: PDFPageProxy | null = null;
    let renderTask: RenderTask | null = null;
    let canvasLease: PdfCanvasLease | null = null;
    const canvas = canvasRef.current;

    const clearCanvas = () => {
      if (!canvas) return;
      canvas.width = 0;
      canvas.height = 0;
      canvas.style.width = "0";
      canvas.style.height = "0";
    };
    const cleanupPage = () => {
      page?.cleanup();
      page = null;
    };

    const render = async (signal: AbortSignal) => {
      if (!canvas) return;
      try {
        page = await pdf.getPage(pageIndex + 1);
        if (cancelled || signal.aborted) {
          cleanupPage();
          return;
        }
        const baseViewport = page.getViewport({ scale: 1 });
        const viewport = page.getViewport({ scale: 124 / baseViewport.width });
        const ratio = Math.min(window.devicePixelRatio || 1, 1.5);
        canvasLease = canvasBudget.reserve({
          owner: `pdf-thumbnail:${pageIndex}`,
          width: viewport.width,
          height: viewport.height,
          requestedScale: ratio,
          priority: 20,
          onEvict: () => {
            renderTask?.cancel();
            canvasLease = null;
            clearCanvas();
          },
        });
        if (!canvasLease || cancelled || signal.aborted) {
          canvasLease?.release();
          canvasLease = null;
          cleanupPage();
          return;
        }
        const canvasScale = canvasLease.scale;
        canvas.width = Math.max(1, Math.floor(viewport.width * canvasScale));
        canvas.height = Math.max(1, Math.floor(viewport.height * canvasScale));
        canvas.style.width = `${viewport.width}px`;
        canvas.style.height = `${viewport.height}px`;
        const context = canvas.getContext("2d", { alpha: false });
        if (!context) {
          canvasLease.release();
          canvasLease = null;
          clearCanvas();
          cleanupPage();
          return;
        }
        renderTask = page.render({
          canvas,
          canvasContext: context,
          viewport,
          transform: canvasScale === 1 ? undefined : [canvasScale, 0, 0, canvasScale, 0, 0],
        });
        const cancelRender = () => renderTask?.cancel();
        signal.addEventListener("abort", cancelRender, { once: true });
        await renderTask.promise;
        signal.removeEventListener("abort", cancelRender);
        if (cancelled || signal.aborted) {
          canvasLease?.release();
          canvasLease = null;
          clearCanvas();
          cleanupPage();
          return;
        }
        cleanupPage();
      } catch (error) {
        canvasLease?.release();
        canvasLease = null;
        clearCanvas();
        cleanupPage();
        if (cancelled || (error instanceof Error && error.name === "RenderingCancelledException")) return;
      }
    };

    const scheduled = renderScheduler.schedule(
      `pdf-thumbnail:${pageIndex}`,
      20,
      render,
    );
    void scheduled.promise.catch(() => undefined);
    return () => {
      cancelled = true;
      scheduled.cancel();
      renderTask?.cancel();
      canvasLease?.release();
      canvasLease = null;
      clearCanvas();
      cleanupPage();
    };
  }, [canvasBudget, pageIndex, pdf, renderScheduler]);

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
