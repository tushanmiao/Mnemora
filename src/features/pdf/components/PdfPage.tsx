import {
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { TextLayer, type PDFDocumentProxy, type PDFPageProxy, type RenderTask } from "pdfjs-dist";
import type { LibraryAnnotation, LibraryAnnotationRect } from "../../library/types";
import type { PdfTextSelection } from "../types";
import { resolvePdfCanvasScale, resolvePdfPageDisplaySize } from "../utils/pdfViewport";
import { useI18n } from "../../../i18n/I18nProvider";
import type { WorkspaceLifecycleState } from "../../../runtime/resources/WorkspaceLifecycle";
import type { PdfCanvasBudget, PdfCanvasLease } from "../runtime/pdfCanvasBudget";
import type { PdfRenderScheduler } from "../runtime/pdfRenderScheduler";

type PdfPageProps = {
  pdf: PDFDocumentProxy;
  pageIndex: number;
  zoom: number;
  readerWidth: number;
  annotations: LibraryAnnotation[];
  annotationMode: "text" | "area";
  focusedAnnotationId: string | null;
  isCurrent: boolean;
  lifecycleState: WorkspaceLifecycleState;
  canvasBudget: PdfCanvasBudget;
  renderScheduler: PdfRenderScheduler;
  onTextSelection: (selection: PdfTextSelection) => void;
  onAreaSelection: (pageIndex: number, rect: LibraryAnnotationRect) => void;
};

const DEFAULT_PAGE_WIDTH = 595;
const DEFAULT_PAGE_HEIGHT = 842;

export function PdfPage({
  pdf,
  pageIndex,
  zoom,
  readerWidth,
  annotations,
  annotationMode,
  focusedAnnotationId,
  isCurrent,
  lifecycleState,
  canvasBudget,
  renderScheduler,
  onTextSelection,
  onAreaSelection,
}: PdfPageProps) {
  const { t } = useI18n();
  const shellRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textLayerRef = useRef<HTMLDivElement>(null);
  const [pageSize, setPageSize] = useState({
    width: DEFAULT_PAGE_WIDTH,
    height: DEFAULT_PAGE_HEIGHT,
  });
  const [renderedSize, setRenderedSize] = useState<{ width: number; height: number } | null>(null);
  const [status, setStatus] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [areaDraft, setAreaDraft] = useState<LibraryAnnotationRect | null>(null);
  const areaStartRef = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (lifecycleState !== "active" || readerWidth <= 0) {
      setStatus("idle");
      setRenderedSize(null);
      return;
    }
    let cancelled = false;
    let page: PDFPageProxy | null = null;
    let renderTask: RenderTask | null = null;
    let textLayer: TextLayer | null = null;
    let canvasLease: PdfCanvasLease | null = null;
    const canvas = canvasRef.current;
    const textLayerContainer = textLayerRef.current;

    const clearLayers = () => {
      if (canvas) {
        canvas.width = 0;
        canvas.height = 0;
        canvas.style.width = "0";
        canvas.style.height = "0";
      }
      if (textLayerContainer) textLayerContainer.replaceChildren();
    };
    const cleanupPage = () => {
      page?.cleanup();
      page = null;
    };

    const renderPage = async (signal: AbortSignal) => {
      if (!canvas || !textLayerContainer) return;
      setStatus("loading");
      try {
        page = await pdf.getPage(pageIndex + 1);
        if (cancelled || signal.aborted) {
          cleanupPage();
          return;
        }
        const baseViewport = page.getViewport({ scale: 1 });
        setPageSize({ width: baseViewport.width, height: baseViewport.height });
        const displaySize = resolvePdfPageDisplaySize(
          readerWidth,
          baseViewport.width,
          baseViewport.height,
          zoom,
        );
        const scale = displaySize.width / baseViewport.width;
        const viewport = page.getViewport({ scale });
        setRenderedSize({ width: viewport.width, height: viewport.height });
        const requestedScale = resolvePdfCanvasScale(
          viewport.width,
          viewport.height,
          isCurrent ? window.devicePixelRatio || 1 : Math.min(window.devicePixelRatio || 1, 1),
        );
        canvasLease = canvasBudget.reserve({
          owner: `pdf-page:${pageIndex}`,
          width: viewport.width,
          height: viewport.height,
          requestedScale,
          priority: isCurrent ? 0 : 10,
          onEvict: () => {
            renderTask?.cancel();
            textLayer?.cancel();
            clearLayers();
            if (!cancelled) setStatus("idle");
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
        textLayerContainer.style.width = `${viewport.width}px`;
        textLayerContainer.style.height = `${viewport.height}px`;
        textLayerContainer.style.setProperty("--total-scale-factor", String(viewport.scale));
        const context = canvas.getContext("2d", { alpha: false });
        if (!context) throw new Error("无法创建 PDF 画布。");
        renderTask = page.render({
          canvas,
          canvasContext: context,
          viewport,
          transform: canvasScale === 1
            ? undefined
            : [canvasScale, 0, 0, canvasScale, 0, 0],
        });
        const cancelRender = () => {
          renderTask?.cancel();
          textLayer?.cancel();
        };
        signal.addEventListener("abort", cancelRender, { once: true });
        await renderTask.promise;
        if (cancelled || signal.aborted) {
          cleanupPage();
          return;
        }
        if (!isCurrent) {
          setStatus("ready");
          cleanupPage();
          return;
        }
        const textContent = await page.getTextContent();
        if (cancelled || signal.aborted) {
          cleanupPage();
          return;
        }
        textLayer = new TextLayer({
          textContentSource: textContent,
          container: textLayerContainer,
          viewport,
        });
        await textLayer.render();
        if (!cancelled) setStatus("ready");
        cleanupPage();
      } catch (error) {
        cleanupPage();
        if (cancelled || (error instanceof Error && error.name === "RenderingCancelledException")) {
          canvasLease?.release();
          canvasLease = null;
          clearLayers();
          return;
        }
        clearLayers();
        setRenderedSize(null);
        setStatus("error");
      }
    };

    const scheduled = renderScheduler.schedule(
      `pdf-page:${pageIndex}`,
      isCurrent ? 0 : 10,
      renderPage,
    );
    void scheduled.promise.catch(() => {
      if (!cancelled) setStatus("error");
    });
    return () => {
      cancelled = true;
      scheduled.cancel();
      renderTask?.cancel();
      textLayer?.cancel();
      canvasLease?.release();
      clearLayers();
      cleanupPage();
    };
  }, [canvasBudget, isCurrent, lifecycleState, pdf, pageIndex, readerWidth, renderScheduler, zoom]);

  useEffect(() => {
    areaStartRef.current = null;
    setAreaDraft(null);
  }, [annotationMode]);

  const handleTextSelection = () => {
    if (annotationMode !== "text") return;
    const selection = window.getSelection();
    const textLayer = textLayerRef.current;
    const surface = textLayer?.parentElement;
    if (!selection || selection.isCollapsed || selection.rangeCount === 0 || !textLayer || !surface) {
      return;
    }
    if (!selection.anchorNode || !selection.focusNode) return;
    if (!textLayer.contains(selection.anchorNode) || !textLayer.contains(selection.focusNode)) return;
    const text = selection.toString().trim().slice(0, 20_000);
    if (!text) return;
    const surfaceRect = surface.getBoundingClientRect();
    if (surfaceRect.width <= 0 || surfaceRect.height <= 0) return;
    const clientRects = Array.from(selection.getRangeAt(0).getClientRects());
    const rects = clientRects
      .map((rect): LibraryAnnotationRect | null => {
        const left = Math.max(surfaceRect.left, rect.left);
        const top = Math.max(surfaceRect.top, rect.top);
        const right = Math.min(surfaceRect.right, rect.right);
        const bottom = Math.min(surfaceRect.bottom, rect.bottom);
        if (right - left < 0.5 || bottom - top < 0.5) return null;
        return {
          x: (left - surfaceRect.left) / surfaceRect.width,
          y: (top - surfaceRect.top) / surfaceRect.height,
          width: (right - left) / surfaceRect.width,
          height: (bottom - top) / surfaceRect.height,
        };
      })
      .filter((rect): rect is LibraryAnnotationRect => rect !== null)
      .slice(0, 256);
    if (rects.length === 0) return;
    const anchor = clientRects[clientRects.length - 1] ?? surfaceRect;
    onTextSelection({
      pageIndex,
      text,
      rects,
      clientX: Math.min(window.innerWidth - 76, Math.max(76, anchor.left + anchor.width / 2)),
      clientY: Math.min(window.innerHeight - 44, Math.max(8, anchor.bottom + 8)),
    });
  };

  const relativePointer = (event: ReactPointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(1, (event.clientX - bounds.left) / Math.max(1, bounds.width))),
      y: Math.max(0, Math.min(1, (event.clientY - bounds.top) / Math.max(1, bounds.height))),
    };
  };

  const handleAreaPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (annotationMode !== "area" || event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    const point = relativePointer(event);
    areaStartRef.current = point;
    setAreaDraft({ x: point.x, y: point.y, width: 0.000_001, height: 0.000_001 });
  };

  const handleAreaPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const start = areaStartRef.current;
    if (!start) return;
    const point = relativePointer(event);
    setAreaDraft({
      x: Math.min(start.x, point.x),
      y: Math.min(start.y, point.y),
      width: Math.abs(point.x - start.x),
      height: Math.abs(point.y - start.y),
    });
  };

  const finishAreaSelection = (event: ReactPointerEvent<HTMLDivElement>) => {
    const start = areaStartRef.current;
    if (!start) return;
    const point = relativePointer(event);
    const rect = {
      x: Math.min(start.x, point.x),
      y: Math.min(start.y, point.y),
      width: Math.abs(point.x - start.x),
      height: Math.abs(point.y - start.y),
    };
    areaStartRef.current = null;
    setAreaDraft(null);
    if (rect.width < 0.005 || rect.height < 0.005) return;
    onAreaSelection(pageIndex, rect);
  };

  const displaySize = renderedSize ?? resolvePdfPageDisplaySize(
    readerWidth,
    pageSize.width,
    pageSize.height,
    zoom,
  );
  return (
    <div
      className="mnemora-pdf-page-shell"
      ref={shellRef}
      data-page-index={pageIndex}
      style={{
        width: displaySize.width + 32,
        minWidth: displaySize.width + 32,
        minHeight: displaySize.height + 32,
      }}
      aria-label={t("pdf.page", { page: pageIndex + 1 })}
    >
      <div
        className="mnemora-pdf-page-surface"
        style={{ width: displaySize.width, height: displaySize.height }}
      >
        <canvas className="mnemora-pdf-page-canvas" ref={canvasRef} />
        {lifecycleState === "active" ? (
          <div className="mnemora-pdf-annotation-layer" aria-hidden="true">
            {annotations.flatMap((annotation) => annotation.rects.map((rect, rectIndex) => (
              <span
                className={`mnemora-pdf-annotation mnemora-pdf-annotation-${annotation.kind} mnemora-pdf-annotation-${annotation.color}${focusedAnnotationId === annotation.id ? " is-focused" : ""}`}
                key={`${annotation.id}:${rectIndex}`}
                style={{
                  left: `${rect.x * 100}%`,
                  top: `${rect.y * 100}%`,
                  width: `${rect.width * 100}%`,
                  height: `${rect.height * 100}%`,
                }}
              />
            )))}
          </div>
        ) : null}
        <div
          className="textLayer mnemora-pdf-text-layer"
          ref={textLayerRef}
          onMouseUp={handleTextSelection}
        />
        {lifecycleState === "active" && annotationMode === "area" ? (
          <div
            className="mnemora-pdf-area-selector"
            onPointerDown={handleAreaPointerDown}
            onPointerMove={handleAreaPointerMove}
            onPointerUp={finishAreaSelection}
            onPointerCancel={() => {
              areaStartRef.current = null;
              setAreaDraft(null);
            }}
          >
            {areaDraft ? (
              <span
                style={{
                  left: `${areaDraft.x * 100}%`,
                  top: `${areaDraft.y * 100}%`,
                  width: `${areaDraft.width * 100}%`,
                  height: `${areaDraft.height * 100}%`,
                }}
              />
            ) : null}
          </div>
        ) : null}
        {status === "loading" ? <span className="mnemora-pdf-page-status">{t("pdf.rendering")}</span> : null}
        {status === "error" ? <span className="mnemora-pdf-page-status mnemora-pdf-page-error">{t("pdf.renderFailed")}</span> : null}
      </div>
    </div>
  );
}
