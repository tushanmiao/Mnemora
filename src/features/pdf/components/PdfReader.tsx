import {
  startTransition,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { Virtualizer, type VirtualizerHandle } from "virtua";
import { BookOpen, Highlighter, MessageCircleQuestion, Search, Underline, X } from "lucide-react";
import {
  GlobalWorkerOptions,
  type PDFDocumentProxy,
} from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import type {
  LibraryAnnotation,
  LibraryAnnotationColor,
  LibraryAnnotationKind,
  LibraryAnnotationRect,
  LibraryItem,
  LibraryNote,
} from "../../library/types";
import {
  isLibraryRuntime,
  saveLibraryReadingState,
} from "../../library/api/library";
import type { PdfOutlineEntry, PdfReaderController } from "../context/PdfReaderContext";
import { usePdfReaderBridge } from "../context/PdfReaderContext";
import { PDF_ANNOTATION_COLORS, type PdfTextSelection } from "../types";
import type { LiteratureReference } from "../../../types/chat";
import type { WorkNoteSourceContext } from "../../workspace/types";
import { useI18n } from "../../../i18n/I18nProvider";
import {
  createLiteratureReference,
  MAX_LITERATURE_REFERENCE_TEXT_BYTES,
  normalizeLiteratureText,
} from "../../chat/utils/literatureReferences";
import { PdfPage } from "./PdfPage";
import { PdfReaderToolbar } from "./PdfReaderToolbar";
import { usePdfResources } from "../hooks/usePdfResources";
import { loadPdfDocument, type ReadingPosition } from "../runtime/pdfDocumentLoader";
import { PdfCanvasBudget } from "../runtime/pdfCanvasBudget";
import { PdfRenderScheduler } from "../runtime/pdfRenderScheduler";
import { useWorkspaceLifecycle } from "../../../runtime/resources/useWorkspaceLifecycle";
import "../styles/pdf-reader.css";

GlobalWorkerOptions.workerSrc = workerUrl;

const MAX_SEARCH_RESULTS = 100;

type PdfReaderProps = {
  item: LibraryItem;
  onOpenExternal: (itemId: string) => Promise<LibraryItem>;
  onOpenNote: (
    note: Pick<LibraryNote, "id" | "title">,
    source?: WorkNoteSourceContext,
  ) => void;
  onAskSelection: (reference: LiteratureReference) => void;
};

type SearchResult = {
  pageIndex: number;
  snippet: string;
};

export function PdfReader({ item, onOpenExternal, onOpenNote, onAskSelection }: PdfReaderProps) {
  const { t } = useI18n();
  const { register, unregister } = usePdfReaderBridge();
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const virtualizerRef = useRef<VirtualizerHandle>(null);
  const saveTimerRef = useRef<number | null>(null);
  const scrollFrameRef = useRef<number | null>(null);
  const readerResizeFrameRef = useRef<number | null>(null);
  const pendingReaderWidthRef = useRef(0);
  const pageNavigationFrameRef = useRef<number | null>(null);
  const searchGenerationRef = useRef(0);
  const focusTimerRef = useRef<number | null>(null);
  const selectionColorMenuRef = useRef<HTMLDivElement>(null);
  const restoringRef = useRef(false);
  const readingStateReadyRef = useRef(false);
  const pendingRestoreRef = useRef<ReadingPosition | null>(null);
  const readingRef = useRef<ReadingPosition>({ pageIndex: 0, scrollOffset: 0, zoom: 1 });
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [pageCount, setPageCount] = useState(0);
  const [currentPage, setCurrentPage] = useState(1);
  const [zoom, setZoom] = useState(1);
  const [outline, setOutline] = useState<PdfOutlineEntry[]>([]);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [readerWidth, setReaderWidth] = useState(0);
  const [annotationMode, setAnnotationMode] = useState<"text" | "area">("text");
  const [annotationColor, setAnnotationColor] = useState<LibraryAnnotationColor>("yellow");
  const [textSelection, setTextSelection] = useState<PdfTextSelection | null>(null);
  const [selectionColorMenuOpen, setSelectionColorMenuOpen] = useState(false);
  const [focusedAnnotationId, setFocusedAnnotationId] = useState<string | null>(null);
  const resources = usePdfResources(item.id);
  const lifecycleState = useWorkspaceLifecycle();
  const canvasBudget = useMemo(() => new PdfCanvasBudget(), [item.id]);
  const renderScheduler = useMemo(() => new PdfRenderScheduler(), [item.id]);
  const pageIndexes = useMemo(
    () => Array.from({ length: pageCount }, (_, pageIndex) => pageIndex),
    [pageCount],
  );
  const estimatedPageSize = useMemo(() => {
    const width = Math.max(280, Math.min(readerWidth - 76, 1100)) * zoom;
    return width * (842 / 595) + 50;
  }, [readerWidth, zoom]);

  const scheduleSave = useCallback((next: ReadingPosition) => {
    readingRef.current = next;
    if (!isLibraryRuntime()) return;
    if (saveTimerRef.current !== null) window.clearTimeout(saveTimerRef.current);
    saveTimerRef.current = window.setTimeout(() => {
      saveTimerRef.current = null;
      void saveLibraryReadingState({ itemId: item.id, ...readingRef.current }).catch(() => undefined);
    }, 650);
  }, [item.id]);

  const flushReadingState = useCallback(() => {
    if (!isLibraryRuntime() || !readingStateReadyRef.current) return;
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    void saveLibraryReadingState({ itemId: item.id, ...readingRef.current }).catch(() => undefined);
  }, [item.id]);

  const scrollToPage = useCallback((pageIndex: number, behavior: ScrollBehavior = "smooth") => {
    const virtualizer = virtualizerRef.current;
    if (!virtualizer) return false;
    virtualizer.scrollToIndex(Math.max(0, pageIndex), { align: "start", smooth: behavior === "smooth" });
    return true;
  }, []);

  useEffect(() => () => unregister(item.id), [item.id, unregister]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (selectionColorMenuOpen) {
        setSelectionColorMenuOpen(false);
        return;
      }
      setTextSelection(null);
      setAnnotationMode("text");
      window.getSelection()?.removeAllRanges();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [selectionColorMenuOpen]);

  useEffect(() => {
    if (!selectionColorMenuOpen) return;
    const closeColorMenu = (event: PointerEvent) => {
      if (!selectionColorMenuRef.current?.contains(event.target as Node)) {
        setSelectionColorMenuOpen(false);
      }
    };
    document.addEventListener("pointerdown", closeColorMenu);
    return () => document.removeEventListener("pointerdown", closeColorMenu);
  }, [selectionColorMenuOpen]);

  useEffect(() => {
    if (!textSelection) setSelectionColorMenuOpen(false);
  }, [textSelection]);

  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) return;
    const updateWidth = () => {
      const nextWidth = Math.round(container.clientWidth);
      if (nextWidth <= 0) return;
      pendingReaderWidthRef.current = nextWidth;
      if (readerResizeFrameRef.current !== null) return;
      readerResizeFrameRef.current = requestAnimationFrame(() => {
        readerResizeFrameRef.current = null;
        const width = pendingReaderWidthRef.current;
        startTransition(() => {
          setReaderWidth((current) => current === width ? current : width);
        });
      });
    };
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(container);
    return () => {
      observer.disconnect();
      if (readerResizeFrameRef.current !== null) {
        cancelAnimationFrame(readerResizeFrameRef.current);
        readerResizeFrameRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    return () => {
      renderScheduler.dispose();
      canvasBudget.releaseAll();
    };
  }, [canvasBudget, renderScheduler]);

  useEffect(() => {
    let cancelled = false;
    const defaultPosition: ReadingPosition = { pageIndex: 0, scrollOffset: 0, zoom: 1 };
    setLoading(true);
    setError("");
    setPdf(null);
    setPageCount(0);
    setOutline([]);
    setSearchResults([]);
    readingRef.current = defaultPosition;
    readingStateReadyRef.current = false;
    pendingRestoreRef.current = null;
    restoringRef.current = true;
    const handle = loadPdfDocument(item, t);
    void handle.promise
      .then(({ pdf: loadedPdf, position, outline: nextOutline }) => {
        if (cancelled) return;
        readingRef.current = position;
        readingStateReadyRef.current = true;
        pendingRestoreRef.current = position;
        setZoom(position.zoom);
        setCurrentPage(position.pageIndex + 1);
        setPdf(loadedPdf);
        setPageCount(loadedPdf.numPages);
        setCurrentPage(Math.min(loadedPdf.numPages, position.pageIndex + 1));
        setOutline(nextOutline);
      })
      .catch((loadError) => {
        if (cancelled) return;
        setError(loadError instanceof Error ? loadError.message : t("pdf.loadFailed"));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
      searchGenerationRef.current += 1;
      // Canvas eviction runs first. The zero-delay task lets cancelled page
      // render promises settle before pdf.js destroys its worker and caches.
      window.setTimeout(() => handle.dispose(), 0);
    };
  }, [item.id, item.file.fileSize, t]);

  useEffect(() => {
    const position = pendingRestoreRef.current;
    if (!pdf || pageCount <= 0 || readerWidth <= 0 || !position) return;
    const virtualizer = virtualizerRef.current;
    if (!virtualizer) return;
    let cancelled = false;
    let frame: number | null = null;
    const pageIndex = Math.min(pageCount - 1, Math.max(0, position.pageIndex));
    virtualizer.scrollToIndex(pageIndex, { align: "start", smooth: false });
    frame = requestAnimationFrame(() => {
      if (cancelled) return;
      const offset = virtualizer.getItemOffset(pageIndex);
      const size = virtualizer.getItemSize(pageIndex);
      virtualizer.scrollTo(offset + size * Math.max(0, Math.min(1, position.scrollOffset)));
      readingRef.current = { ...position, pageIndex };
      setCurrentPage(pageIndex + 1);
      pendingRestoreRef.current = null;
      restoringRef.current = false;
    });
    return () => {
      cancelled = true;
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, [item.id, pageCount, pdf, readerWidth]);

  useEffect(() => () => {
    if (scrollFrameRef.current !== null) cancelAnimationFrame(scrollFrameRef.current);
    if (pageNavigationFrameRef.current !== null) cancelAnimationFrame(pageNavigationFrameRef.current);
    if (focusTimerRef.current !== null) window.clearTimeout(focusTimerRef.current);
  }, []);

  useEffect(() => () => flushReadingState(), [flushReadingState]);

  const handleScroll = useCallback((nextOffset: number) => {
    if (scrollFrameRef.current !== null) return;
    if (textSelection) window.getSelection()?.removeAllRanges();
    setTextSelection(null);
    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      if (pageCount === 0) return;
      const virtualizer = virtualizerRef.current;
      if (!virtualizer) return;
      const nearestPage = Math.max(0, Math.min(pageCount - 1, virtualizer.findItemIndex(nextOffset + virtualizer.viewportSize * 0.35)));
      const pageTop = virtualizer.getItemOffset(nearestPage);
      const pageHeight = virtualizer.getItemSize(nearestPage);
      const scrollOffset = Math.max(0, Math.min(1, (nextOffset - pageTop) / Math.max(1, pageHeight)));
      setCurrentPage(nearestPage + 1);
      if (!restoringRef.current) {
        scheduleSave({ pageIndex: nearestPage, scrollOffset, zoom: readingRef.current.zoom });
      }
    });
  }, [pageCount, scheduleSave, textSelection]);

  const updateZoom = (nextZoom: number) => {
    const normalized = Math.max(0.5, Math.min(3, Math.round(nextZoom * 10) / 10));
    setZoom(normalized);
    scheduleSave({ ...readingRef.current, zoom: normalized });
  };

  const goToPage = useCallback((pageIndex: number) => {
    if (!Number.isFinite(pageIndex) || pageCount <= 0) return;
    const normalized = Math.max(0, Math.min(pageCount - 1, Math.trunc(pageIndex)));
    if (pageNavigationFrameRef.current !== null) {
      cancelAnimationFrame(pageNavigationFrameRef.current);
      pageNavigationFrameRef.current = null;
    }
    let attempts = 0;
    const navigate = () => {
      pageNavigationFrameRef.current = null;
      if (scrollToPage(normalized) || attempts >= 30) return;
      attempts += 1;
      pageNavigationFrameRef.current = requestAnimationFrame(navigate);
    };
    navigate();
    setCurrentPage(normalized + 1);
    scheduleSave({ ...readingRef.current, pageIndex: normalized, scrollOffset: 0 });
  }, [pageCount, scheduleSave, scrollToPage]);

  const submitPage = (value: string) => {
    const page = Number.parseInt(value, 10);
    if (!Number.isFinite(page) || page < 1 || page > pageCount) return;
    goToPage(page - 1);
  };

  const readPageText = useCallback(async (pageIndex: number) => {
    if (!pdf || pageIndex < 0 || pageIndex >= pdf.numPages) {
      throw new Error(t("pdf.pageUnavailable"));
    }
    const page = await pdf.getPage(pageIndex + 1);
    const content = await page.getTextContent();
    const parts: string[] = [];
    let capturedCharacters = 0;
    const captureLimit = MAX_LITERATURE_REFERENCE_TEXT_BYTES * 2;
    for (const entry of content.items) {
      if (!("str" in entry)) continue;
      const remaining = captureLimit - capturedCharacters;
      if (remaining <= 0) break;
      const text = entry.str.slice(0, remaining);
      parts.push(text);
      parts.push("hasEOL" in entry && entry.hasEOL ? "\n" : " ");
      capturedCharacters += text.length + 1;
    }
    if (pageIndex !== currentPage - 1) page.cleanup();
    const text = normalizeLiteratureText(parts.join(""));
    if (!text) throw new Error(t("pdf.noText"));
    return text;
  }, [currentPage, pdf]);

  const askSelectedText = useCallback(() => {
    if (!textSelection) return;
    const reference = createLiteratureReference({
      libraryItemId: item.id,
      title: item.title,
      pageIndex: textSelection.pageIndex,
      kind: "selection",
      text: textSelection.text,
    });
    if (!reference) {
      resources.setAnnotationError(t("pdf.referenceSelectionFailed"));
      return;
    }
    if (typeof onAskSelection !== "function") {
      resources.setAnnotationError(t("pdf.chatUnavailable"));
      return;
    }
    onAskSelection(reference);
    setTextSelection(null);
    window.getSelection()?.removeAllRanges();
  }, [item.id, item.title, onAskSelection, textSelection]);

  const preserveSelectionToolbarPointer = useCallback((
    event: ReactPointerEvent<HTMLDivElement>,
  ) => {
    event.preventDefault();
    event.stopPropagation();
  }, []);

  const runSearch = async () => {
    const query = searchQuery.trim().toLocaleLowerCase();
    if (!pdf || !query) {
      setSearchResults([]);
      return;
    }
    const generation = ++searchGenerationRef.current;
    setSearching(true);
    setSearchResults([]);
    const results: SearchResult[] = [];
    try {
      for (let pageIndex = 0; pageIndex < pdf.numPages && results.length < MAX_SEARCH_RESULTS; pageIndex += 1) {
        const page = await pdf.getPage(pageIndex + 1);
        const content = await page.getTextContent();
        const text = content.items
          .map((entry) => "str" in entry ? entry.str : "")
          .join(" ");
        const matchIndex = text.toLocaleLowerCase().indexOf(query);
        if (matchIndex >= 0) {
          const start = Math.max(0, matchIndex - 48);
          const end = Math.min(text.length, matchIndex + query.length + 80);
          results.push({ pageIndex, snippet: text.slice(start, end).trim() });
        }
        if (pageIndex !== currentPage - 1) page.cleanup();
        if (generation !== searchGenerationRef.current) return;
      }
      if (generation === searchGenerationRef.current) setSearchResults(results);
    } catch {
      if (generation === searchGenerationRef.current) setSearchResults([]);
    } finally {
      if (generation === searchGenerationRef.current) setSearching(false);
    }
  };

  const annotationsByPage = useMemo(() => {
    const grouped = new Map<number, LibraryAnnotation[]>();
    for (const annotation of resources.annotations) {
      const pageAnnotations = grouped.get(annotation.pageIndex);
      if (pageAnnotations) pageAnnotations.push(annotation);
      else grouped.set(annotation.pageIndex, [annotation]);
    }
    return grouped;
  }, [resources.annotations]);

  const createTextAnnotation = useCallback(async (kind: Exclude<LibraryAnnotationKind, "area">) => {
    const selection = textSelection;
    if (!selection) return;
    const annotation = await resources.createAnnotation(
      kind,
      selection.pageIndex,
      annotationColor,
      selection.text,
      selection.rects,
    );
    if (annotation) {
      setTextSelection(null);
      window.getSelection()?.removeAllRanges();
    }
  }, [annotationColor, resources, textSelection]);

  const createAreaAnnotation = useCallback(async (
    pageIndex: number,
    rect: LibraryAnnotationRect,
  ) => {
    const annotation = await resources.createAnnotation("area", pageIndex, annotationColor, "", [rect]);
    if (annotation) setAnnotationMode("text");
  }, [annotationColor, resources]);

  const goToAnnotation = useCallback((annotation: LibraryAnnotation) => {
    goToPage(annotation.pageIndex);
    setFocusedAnnotationId(annotation.id);
    if (focusTimerRef.current !== null) window.clearTimeout(focusTimerRef.current);
    focusTimerRef.current = window.setTimeout(() => {
      focusTimerRef.current = null;
      setFocusedAnnotationId(null);
    }, 1800);
  }, [goToPage]);

  const setReaderAnnotationMode = useCallback((mode: "text" | "area") => {
    setAnnotationMode(mode);
    setTextSelection(null);
    window.getSelection()?.removeAllRanges();
  }, []);

  const readerController = useMemo<PdfReaderController | null>(() => {
    if (!pdf) return null;
    return {
      itemId: item.id,
      pdf,
      pageCount,
      currentPage,
      zoom,
      outline,
      canvasBudget,
      annotations: resources.annotations,
      notes: resources.notes,
      annotationsLoading: resources.annotationsLoading,
      notesLoading: resources.notesLoading,
      notesLoaded: resources.notesLoaded,
      annotationError: resources.annotationError,
      noteError: resources.noteError,
      annotationMode,
      annotationColor,
      goToPage,
      readPageText,
      goToAnnotation,
      setAnnotationMode: setReaderAnnotationMode,
      setAnnotationColor,
      updateAnnotation: resources.updateAnnotation,
      deleteAnnotation: resources.deleteAnnotation,
      loadNotes: resources.loadNotes,
      createNote: resources.createNote,
      updateNote: resources.updateNote,
      deleteNote: resources.deleteNote,
      openNote: (note) => onOpenNote(note, {
        sourcePdfId: item.id,
        sourcePdfTitle: item.title,
        sourcePageIndex: Math.max(0, currentPage - 1),
      }),
    };
  }, [
    annotationColor,
    annotationMode,
    canvasBudget,
    currentPage,
    goToAnnotation,
    goToPage,
    item.id,
    onOpenNote,
    outline,
    pageCount,
    pdf,
    readPageText,
    resources.annotationError,
    resources.annotations,
    resources.annotationsLoading,
    resources.createNote,
    resources.deleteAnnotation,
    resources.deleteNote,
    resources.loadNotes,
    resources.noteError,
    resources.notes,
    resources.notesLoaded,
    resources.notesLoading,
    resources.updateAnnotation,
    resources.updateNote,
    setReaderAnnotationMode,
    zoom,
  ]);

  useEffect(() => {
    if (!readerController) {
      unregister(item.id);
      return;
    }
    register(readerController);
  }, [item.id, readerController, register, unregister]);

  const status = loading
    ? t("pdf.loading")
    : error
      ? error
      : t("pdf.pages", { count: pageCount });

  return (
    <section
      className={`mnemora-pdf-reader mnemora-pdf-color-${annotationColor}`}
      aria-label={t("pdf.reader", { title: item.title })}
    >
      <PdfReaderToolbar
        title={item.title}
        pdfAvailable={Boolean(pdf)}
        currentPage={currentPage}
        pageCount={pageCount}
        zoom={zoom}
        annotationMode={annotationMode}
        annotationColor={annotationColor}
        searchOpen={searchOpen}
        t={t}
        onPageSubmit={submitPage}
        onAnnotationModeChange={setReaderAnnotationMode}
        onAnnotationColorChange={setAnnotationColor}
        onZoomChange={updateZoom}
        onOpenExternal={() => void onOpenExternal(item.id).catch(() => undefined)}
        onSearchToggle={() => setSearchOpen((open) => !open)}
      />

      {searchOpen ? (
        <div className="mnemora-pdf-search-bar">
          <Search size={15} aria-hidden="true" />
          <input
            value={searchQuery}
            placeholder={t("pdf.searchPlaceholder")}
            aria-label={t("pdf.searchPlaceholder")}
            onChange={(event) => setSearchQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void runSearch();
            }}
          />
          <button type="button" disabled={searching || !searchQuery.trim()} onClick={() => void runSearch()}>
            {searching ? t("pdf.searching") : t("common.search")}
          </button>
          <button className="icon-button" type="button" title={t("pdf.closeSearch")} aria-label={t("pdf.closeSearch")} onClick={() => setSearchOpen(false)}>
            <X size={15} />
          </button>
        </div>
      ) : null}

      {searchOpen && searchResults.length > 0 ? (
        <div className="mnemora-pdf-search-results" role="listbox" aria-label={t("pdf.searchResults")}>
          {searchResults.map((result) => (
            <button
              type="button"
              role="option"
              key={`${result.pageIndex}-${result.snippet}`}
              onClick={() => submitPage(String(result.pageIndex + 1))}
            >
              <strong>{t("pdf.page", { page: result.pageIndex + 1 })}</strong>
              <span>{result.snippet}</span>
            </button>
          ))}
        </div>
      ) : null}

      {resources.annotationError ? (
        <div className="mnemora-pdf-annotation-error" role="alert">
          <span>{resources.annotationError}</span>
          <button type="button" aria-label={t("pdf.closeAnnotationError")} onClick={() => resources.setAnnotationError("")}>
            <X size={14} />
          </button>
        </div>
      ) : null}

      {textSelection ? (
        <div
          className="mnemora-pdf-selection-toolbar"
          role="toolbar"
          aria-label={t("pdf.textAnnotation")}
          style={{ left: textSelection.clientX, top: textSelection.clientY }}
          onPointerDown={preserveSelectionToolbarPointer}
        >
          <button type="button" title={t("pdf.highlight")} aria-label={t("pdf.highlight")} onClick={() => void createTextAnnotation("highlight")}>
            <Highlighter size={15} />
          </button>
          <button type="button" title={t("pdf.underline")} aria-label={t("pdf.underline")} onClick={() => void createTextAnnotation("underline")}>
            <Underline size={15} />
          </button>
          <button type="button" title={t("pdf.askAi")} aria-label={t("pdf.askAiSelection")} onClick={askSelectedText}>
            <MessageCircleQuestion size={15} />
          </button>
          <div className="mnemora-pdf-selection-color-picker" ref={selectionColorMenuRef}>
            <button
              className="mnemora-pdf-selection-color-button"
              type="button"
              title={t("pdf.changeHighlightColor")}
              aria-label={t("pdf.changeHighlightColor")}
              aria-haspopup="menu"
              aria-expanded={selectionColorMenuOpen}
              onClick={() => setSelectionColorMenuOpen((open) => !open)}
            >
              <span className={`mnemora-pdf-selection-color mnemora-pdf-color-${annotationColor}`} aria-hidden="true" />
            </button>
            {selectionColorMenuOpen ? (
              <div className="mnemora-pdf-selection-color-menu" role="menu" aria-label={t("pdf.selectHighlightColor")}>
                {PDF_ANNOTATION_COLORS.map((color) => (
                  <button
                    className={`mnemora-pdf-selection-color-option mnemora-pdf-color-${color.id}${annotationColor === color.id ? " is-active" : ""}`}
                    type="button"
                    role="menuitemradio"
                    aria-label={color.label}
                    aria-checked={annotationColor === color.id}
                    title={color.label}
                    key={color.id}
                    onClick={() => {
                      setAnnotationColor(color.id);
                      setSelectionColorMenuOpen(false);
                    }}
                  />
                ))}
              </div>
            ) : null}
          </div>
          <button type="button" title={t("common.cancel")} aria-label={t("pdf.cancelAnnotation")} onClick={() => {
            setTextSelection(null);
            window.getSelection()?.removeAllRanges();
          }}>
            <X size={14} />
          </button>
        </div>
      ) : null}

      <div
        className="mnemora-pdf-reader-scroll"
        ref={scrollContainerRef}
        role="document"
        aria-label={status}
      >
        {error ? (
          <div className="mnemora-pdf-reader-state mnemora-pdf-reader-error" role="alert">
            <strong>{t("pdf.unableToLoad")}</strong>
            <span>{error}</span>
          </div>
        ) : pdf ? (
          <div className="mnemora-pdf-page-list">
            <Virtualizer
              ref={virtualizerRef}
              scrollRef={scrollContainerRef}
              data={pageIndexes}
              bufferSize={180}
              itemSize={estimatedPageSize}
              onScroll={handleScroll}
            >
              {(pageIndex) => (
                <div className="mnemora-pdf-page-slot">
                  <PdfPage
                    pdf={pdf}
                    pageIndex={pageIndex}
                    zoom={zoom}
                    readerWidth={readerWidth}
                    annotations={annotationsByPage.get(pageIndex) ?? []}
                    annotationMode={annotationMode}
                    focusedAnnotationId={focusedAnnotationId}
                    isCurrent={currentPage === pageIndex + 1}
                    lifecycleState={lifecycleState}
                    canvasBudget={canvasBudget}
                    renderScheduler={renderScheduler}
                    onTextSelection={setTextSelection}
                    onAreaSelection={(targetPageIndex, rect) => {
                      void createAreaAnnotation(targetPageIndex, rect);
                    }}
                  />
                </div>
              )}
            </Virtualizer>
          </div>
        ) : (
          <div className="mnemora-pdf-reader-state" role="status">
            <BookOpen size={26} />
            <span>{status}</span>
          </div>
        )}
      </div>
    </section>
  );
}

export default PdfReader;
