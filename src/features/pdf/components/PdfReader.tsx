import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import {
  ArrowDown,
  ArrowUp,
  BookOpen,
  ExternalLink,
  Highlighter,
  Minus,
  MoveHorizontal,
  MessageCircleQuestion,
  Plus,
  ScanLine,
  Search,
  Underline,
  X,
} from "lucide-react";
import {
  getDocument,
  GlobalWorkerOptions,
  PDFDataRangeTransport,
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
  LibraryNoteSummary,
} from "../../library/types";
import {
  createLibraryAnnotation,
  createLibraryNote,
  deleteLibraryAnnotation,
  deleteLibraryNote,
  getLibraryReadingState,
  isLibraryRuntime,
  listLibraryAnnotations,
  listLibraryNotes,
  readLibraryPdfRange,
  saveLibraryReadingState,
  updateLibraryAnnotation,
  updateLibraryNote,
} from "../../library/api/library";
import type { PdfOutlineEntry } from "../context/PdfReaderContext";
import { usePdfReaderBridge } from "../context/PdfReaderContext";
import { PDF_ANNOTATION_COLORS, type PdfTextSelection } from "../types";
import type { LiteratureReference } from "../../../types/chat";
import {
  createLiteratureReference,
  MAX_LITERATURE_REFERENCE_TEXT_BYTES,
  normalizeLiteratureText,
} from "../../chat/utils/literatureReferences";
import { PdfPage } from "./PdfPage";
import "../styles/pdf-reader.css";

GlobalWorkerOptions.workerSrc = workerUrl;

const RANGE_CHUNK_SIZE = 256 * 1024;
const MAX_SEARCH_RESULTS = 100;

type PdfReaderProps = {
  item: LibraryItem;
  onOpenExternal: (itemId: string) => Promise<LibraryItem>;
  onOpenNote: (note: Pick<LibraryNote, "id" | "title">) => void;
  onAskSelection: (reference: LiteratureReference) => void;
};

type ReadingPosition = {
  pageIndex: number;
  scrollOffset: number;
  zoom: number;
};

type SearchResult = {
  pageIndex: number;
  snippet: string;
};

class TauriPdfRangeTransport extends PDFDataRangeTransport {
  private disposed = false;

  constructor(
    private readonly itemId: string,
    length: number,
    initialData: Uint8Array,
    private readonly onReadError: () => void,
  ) {
    super(length, initialData);
  }

  requestDataRange(begin: number, end: number) {
    if (this.disposed) return;
    void readLibraryPdfRange(this.itemId, begin, end)
      .then((chunk) => {
        if (!this.disposed) this.onDataRange(begin, chunk);
      })
      .catch(() => {
        if (this.disposed) return;
        this.disposed = true;
        this.onReadError();
      });
  }

  dispose() {
    this.disposed = true;
  }

  override abort() {
    this.disposed = true;
  }
}

export function PdfReader({ item, onOpenExternal, onOpenNote, onAskSelection }: PdfReaderProps) {
  const { register, unregister } = usePdfReaderBridge();
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const pageRefs = useRef(new Map<number, HTMLDivElement>());
  const saveTimerRef = useRef<number | null>(null);
  const scrollFrameRef = useRef<number | null>(null);
  const pageNavigationFrameRef = useRef<number | null>(null);
  const searchGenerationRef = useRef(0);
  const resourceGenerationRef = useRef(0);
  const notesLoadRef = useRef({ itemId: "", loading: false, loaded: false });
  const annotationCreateRef = useRef(false);
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
  const [annotations, setAnnotations] = useState<LibraryAnnotation[]>([]);
  const [notes, setNotes] = useState<LibraryNoteSummary[]>([]);
  const [annotationsLoading, setAnnotationsLoading] = useState(false);
  const [notesLoading, setNotesLoading] = useState(false);
  const [notesLoaded, setNotesLoaded] = useState(false);
  const [annotationError, setAnnotationError] = useState("");
  const [noteError, setNoteError] = useState("");
  const [annotationMode, setAnnotationMode] = useState<"text" | "area">("text");
  const [annotationColor, setAnnotationColor] = useState<LibraryAnnotationColor>("yellow");
  const [textSelection, setTextSelection] = useState<PdfTextSelection | null>(null);
  const [selectionColorMenuOpen, setSelectionColorMenuOpen] = useState(false);
  const [focusedAnnotationId, setFocusedAnnotationId] = useState<string | null>(null);

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

  const registerPage = useCallback((pageIndex: number, element: HTMLDivElement | null) => {
    if (element) pageRefs.current.set(pageIndex, element);
    else pageRefs.current.delete(pageIndex);
  }, []);

  const scrollToPage = useCallback((pageIndex: number, behavior: ScrollBehavior = "smooth") => {
    const container = scrollContainerRef.current;
    const page = pageRefs.current.get(Math.max(0, pageIndex));
    if (!container || !page) return false;
    const containerRect = container.getBoundingClientRect();
    const pageRect = page.getBoundingClientRect();
    container.scrollTo({
      top: container.scrollTop + pageRect.top - containerRect.top - 20,
      behavior,
    });
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
    const generation = resourceGenerationRef.current + 1;
    resourceGenerationRef.current = generation;
    setAnnotations([]);
    setNotes([]);
    setAnnotationError("");
    setNoteError("");
    setAnnotationsLoading(true);
    setNotesLoading(false);
    setNotesLoaded(false);
    notesLoadRef.current = { itemId: item.id, loading: false, loaded: false };
    void listLibraryAnnotations(item.id)
      .then((next) => {
        if (resourceGenerationRef.current !== generation) return;
        setAnnotations((current) => {
          const merged = new Map(next.map((annotation) => [annotation.id, annotation]));
          for (const annotation of current) merged.set(annotation.id, annotation);
          return sortAnnotations([...merged.values()]);
        });
      })
      .catch((loadError) => {
        if (resourceGenerationRef.current === generation) {
          setAnnotationError(loadError instanceof Error ? loadError.message : String(loadError));
        }
      })
      .finally(() => {
        if (resourceGenerationRef.current === generation) setAnnotationsLoading(false);
      });
    return () => {
      resourceGenerationRef.current += 1;
      if (notesLoadRef.current.itemId === item.id) {
        notesLoadRef.current = { itemId: "", loading: false, loaded: false };
      }
    };
  }, [item.id]);

  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) return;
    const updateWidth = () => setReaderWidth(Math.round(container.clientWidth));
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let disposed = false;
    let loadingTask: ReturnType<typeof getDocument> | null = null;
    let transport: TauriPdfRangeTransport | null = null;
    let destroyStarted = false;
    let terminalError = "";
    const defaultPosition: ReadingPosition = { pageIndex: 0, scrollOffset: 0, zoom: 1 };

    const destroyLoadingTask = async () => {
      if (!loadingTask || destroyStarted) return;
      destroyStarted = true;
      await loadingTask.destroy().catch(() => undefined);
    };

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

    const load = async () => {
      if (!isLibraryRuntime()) {
        throw new Error("PDF 阅读器只能在桌面应用中使用。");
      }
      if (!Number.isSafeInteger(item.file.fileSize) || item.file.fileSize <= 0) {
        throw new Error("PDF 文件大小无效。");
      }
      const initialEnd = Math.min(item.file.fileSize, RANGE_CHUNK_SIZE);
      const [initialData, savedState] = await Promise.all([
        readLibraryPdfRange(item.id, 0, initialEnd),
        getLibraryReadingState(item.id).catch(() => ({
          itemId: item.id,
          pageIndex: 0,
          scrollOffset: 0,
          zoom: 1,
          updatedAt: 0,
        })),
      ]);
      if (disposed) return;
      const position: ReadingPosition = {
        pageIndex: savedState.pageIndex,
        scrollOffset: savedState.scrollOffset,
        zoom: savedState.zoom,
      };
      readingRef.current = position;
      readingStateReadyRef.current = true;
      pendingRestoreRef.current = position;
      setZoom(position.zoom);
      setCurrentPage(position.pageIndex + 1);
      transport = new TauriPdfRangeTransport(
        item.id,
        item.file.fileSize,
        initialData,
        () => {
          if (disposed) return;
          terminalError = "PDF 数据读取失败，请检查文献文件是否仍然可用。";
          setError(terminalError);
          setLoading(false);
          void destroyLoadingTask();
        },
      );
      loadingTask = getDocument({
        range: transport,
        rangeChunkSize: RANGE_CHUNK_SIZE,
        disableStream: true,
        disableAutoFetch: true,
        cMapUrl: new URL("/pdfjs/cmaps/", window.location.href).toString(),
        cMapPacked: true,
        standardFontDataUrl: new URL("/pdfjs/standard_fonts/", window.location.href).toString(),
        wasmUrl: new URL("/pdfjs/wasm/", window.location.href).toString(),
        useWorkerFetch: true,
        useWasm: true,
        isImageDecoderSupported: false,
        maxImageSize: 25_000_000,
      });
      loadingTask.onPassword = () => {
        terminalError = "当前 PDF 需要密码，请先使用系统阅读器打开。";
        setError(terminalError);
        void destroyLoadingTask();
      };
      const loadedPdf = await loadingTask.promise;
      if (disposed) {
        await destroyLoadingTask();
        return;
      }
      setPdf(loadedPdf);
      setPageCount(loadedPdf.numPages);
      setCurrentPage(Math.min(loadedPdf.numPages, position.pageIndex + 1));
      const rawOutline = await loadedPdf.getOutline().catch(() => null);
      if (!disposed) setOutline(normalizeOutline(rawOutline ?? []));
    };

    void load()
      .catch((loadError) => {
        if (!disposed) {
          setError(terminalError || (loadError instanceof Error ? loadError.message : "PDF 加载失败。"));
        }
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });

    return () => {
      disposed = true;
      searchGenerationRef.current += 1;
      transport?.dispose();
      void destroyLoadingTask();
    };
  }, [item.file.fileSize, item.id]);

  useEffect(() => {
    const position = pendingRestoreRef.current;
    if (!pdf || pageCount <= 0 || readerWidth <= 0 || !position) return;
    let cancelled = false;
    let frame: number | null = null;
    let attempts = 0;

    const restore = () => {
      if (cancelled) return;
      const pageIndex = Math.min(pageCount - 1, Math.max(0, position.pageIndex));
      const container = scrollContainerRef.current;
      const page = pageRefs.current.get(pageIndex);
      if (!container || !page || page.offsetHeight <= 0) {
        attempts += 1;
        if (attempts < 30) {
          frame = requestAnimationFrame(restore);
        } else {
          pendingRestoreRef.current = null;
          restoringRef.current = false;
        }
        return;
      }

      const containerRect = container.getBoundingClientRect();
      const pageTop = page.getBoundingClientRect().top - containerRect.top + container.scrollTop;
      const pageOffset = page.offsetHeight * Math.max(0, Math.min(1, position.scrollOffset));
      container.scrollTo({
        top: Math.max(0, pageTop - 20 + pageOffset),
        behavior: "auto",
      });
      readingRef.current = { ...position, pageIndex };
      setCurrentPage(pageIndex + 1);
      pendingRestoreRef.current = null;
      restoringRef.current = false;
    };

    frame = requestAnimationFrame(restore);
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

  const handleScroll = useCallback(() => {
    if (scrollFrameRef.current !== null) return;
    if (textSelection) window.getSelection()?.removeAllRanges();
    setTextSelection(null);
    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      const container = scrollContainerRef.current;
      if (!container || pageCount === 0) return;
      const containerRect = container.getBoundingClientRect();
      const target = containerRect.top + Math.min(180, container.clientHeight * 0.35);
      let nearestPage = 0;
      let nearestDistance = Number.POSITIVE_INFINITY;
      for (const [pageIndex, element] of pageRefs.current) {
        const rect = element.getBoundingClientRect();
        const distance = target < rect.top
          ? rect.top - target
          : target > rect.bottom
            ? target - rect.bottom
            : 0;
        if (distance < nearestDistance) {
          nearestDistance = distance;
          nearestPage = pageIndex;
        }
      }
      const pageElement = pageRefs.current.get(nearestPage);
      const pageTop = pageElement
        ? pageElement.getBoundingClientRect().top - containerRect.top + container.scrollTop
        : container.scrollTop;
      const pageHeight = pageElement?.offsetHeight ?? container.clientHeight;
      const scrollOffset = Math.max(0, Math.min(1, (container.scrollTop - pageTop) / Math.max(1, pageHeight)));
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
      throw new Error("目标 PDF 页面不可用。");
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
    if (!text) throw new Error("当前页面没有可引用的文字内容。扫描版 PDF 需要后续 OCR 支持。");
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
      setAnnotationError("当前 PDF 选区无法加入文献引用，请重新选择文字后重试。");
      return;
    }
    if (typeof onAskSelection !== "function") {
      setAnnotationError("文献 Chat 尚未准备好，请重新打开 Work 后重试。");
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
    for (const annotation of annotations) {
      const pageAnnotations = grouped.get(annotation.pageIndex);
      if (pageAnnotations) pageAnnotations.push(annotation);
      else grouped.set(annotation.pageIndex, [annotation]);
    }
    return grouped;
  }, [annotations]);

  const createTextAnnotation = useCallback(async (kind: Exclude<LibraryAnnotationKind, "area">) => {
    const selection = textSelection;
    if (!selection || annotationCreateRef.current) return;
    annotationCreateRef.current = true;
    setAnnotationError("");
    try {
      const annotation = await createLibraryAnnotation({
        itemId: item.id,
        kind,
        pageIndex: selection.pageIndex,
        color: annotationColor,
        text: selection.text,
        rects: selection.rects,
      });
      setAnnotations((current) => sortAnnotations([...current, annotation]));
      setTextSelection(null);
      window.getSelection()?.removeAllRanges();
    } catch (createError) {
      setAnnotationError(createError instanceof Error ? createError.message : String(createError));
    } finally {
      annotationCreateRef.current = false;
    }
  }, [annotationColor, item.id, textSelection]);

  const createAreaAnnotation = useCallback(async (
    pageIndex: number,
    rect: LibraryAnnotationRect,
  ) => {
    if (annotationCreateRef.current) return;
    annotationCreateRef.current = true;
    setAnnotationError("");
    try {
      const annotation = await createLibraryAnnotation({
        itemId: item.id,
        kind: "area",
        pageIndex,
        color: annotationColor,
        text: "",
        rects: [rect],
      });
      setAnnotations((current) => sortAnnotations([...current, annotation]));
      setAnnotationMode("text");
    } catch (createError) {
      setAnnotationError(createError instanceof Error ? createError.message : String(createError));
    } finally {
      annotationCreateRef.current = false;
    }
  }, [annotationColor, item.id]);

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

  const updateAnnotationAction = useCallback(async (
    annotationId: string,
    color: LibraryAnnotationColor,
    comment: string,
  ) => {
    setAnnotationError("");
    try {
      const annotation = await updateLibraryAnnotation({ annotationId, color, comment });
      setAnnotations((current) => current.map((candidate) => (
        candidate.id === annotation.id ? annotation : candidate
      )));
      return annotation;
    } catch (updateError) {
      setAnnotationError(updateError instanceof Error ? updateError.message : String(updateError));
      throw updateError;
    }
  }, []);

  const deleteAnnotationAction = useCallback(async (annotationId: string) => {
    setAnnotationError("");
    try {
      const removed = await deleteLibraryAnnotation(annotationId);
      if (removed) setAnnotations((current) => current.filter((item) => item.id !== annotationId));
      return removed;
    } catch (deleteError) {
      setAnnotationError(deleteError instanceof Error ? deleteError.message : String(deleteError));
      throw deleteError;
    }
  }, []);

  const loadNotesAction = useCallback(async () => {
    const current = notesLoadRef.current;
    if (current.itemId === item.id && (current.loading || current.loaded)) return;
    notesLoadRef.current = { itemId: item.id, loading: true, loaded: false };
    setNotesLoading(true);
    setNoteError("");
    try {
      const next = await listLibraryNotes(item.id);
      if (notesLoadRef.current.itemId !== item.id) return;
      notesLoadRef.current = { itemId: item.id, loading: false, loaded: true };
      setNotes(next);
      setNotesLoaded(true);
    } catch (loadError) {
      if (notesLoadRef.current.itemId !== item.id) return;
      notesLoadRef.current = { itemId: item.id, loading: false, loaded: false };
      setNoteError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      if (notesLoadRef.current.itemId === item.id) setNotesLoading(false);
    }
  }, [item.id]);

  const createNoteAction = useCallback(async (title: string, content: string) => {
    setNoteError("");
    try {
      const note = await createLibraryNote({ itemId: item.id, title, content });
      notesLoadRef.current = { itemId: item.id, loading: false, loaded: true };
      setNotesLoaded(true);
      setNotes((current) => [noteSummary(note), ...current]);
      return note;
    } catch (createError) {
      setNoteError(createError instanceof Error ? createError.message : String(createError));
      throw createError;
    }
  }, [item.id]);

  const updateNoteAction = useCallback(async (noteId: string, title: string, content: string) => {
    setNoteError("");
    try {
      const note = await updateLibraryNote({ noteId, title, content });
      setNotes((current) => current.map((candidate) => (
        candidate.id === note.id ? noteSummary(note) : candidate
      )));
      return note;
    } catch (updateError) {
      setNoteError(updateError instanceof Error ? updateError.message : String(updateError));
      throw updateError;
    }
  }, []);

  const deleteNoteAction = useCallback(async (noteId: string) => {
    setNoteError("");
    try {
      const removed = await deleteLibraryNote(noteId);
      if (removed) setNotes((current) => current.filter((note) => note.id !== noteId));
      return removed;
    } catch (deleteError) {
      setNoteError(deleteError instanceof Error ? deleteError.message : String(deleteError));
      throw deleteError;
    }
  }, []);

  useEffect(() => {
    if (!pdf) {
      unregister(item.id);
      return;
    }
    register({
      itemId: item.id,
      pdf,
      pageCount,
      currentPage,
      zoom,
      outline,
      annotations,
      notes,
      annotationsLoading,
      notesLoading,
      notesLoaded,
      annotationError,
      noteError,
      annotationMode,
      annotationColor,
      goToPage,
      readPageText,
      goToAnnotation,
      setAnnotationMode: setReaderAnnotationMode,
      setAnnotationColor,
      updateAnnotation: updateAnnotationAction,
      deleteAnnotation: deleteAnnotationAction,
      loadNotes: loadNotesAction,
      createNote: createNoteAction,
      updateNote: updateNoteAction,
      deleteNote: deleteNoteAction,
      openNote: onOpenNote,
    });
  }, [
    annotationColor,
    annotationError,
    annotationMode,
    annotations,
    annotationsLoading,
    createNoteAction,
    currentPage,
    deleteAnnotationAction,
    deleteNoteAction,
    goToAnnotation,
    goToPage,
    item.id,
    loadNotesAction,
    noteError,
    notes,
    notesLoaded,
    notesLoading,
    onOpenNote,
    outline,
    pageCount,
    pdf,
    register,
    readPageText,
    setReaderAnnotationMode,
    unregister,
    updateAnnotationAction,
    updateNoteAction,
    zoom,
  ]);

  const status = loading
    ? "正在加载 PDF"
    : error
      ? error
      : `${pageCount} 页`;

  return (
    <section
      className={`mnemora-pdf-reader mnemora-pdf-color-${annotationColor}`}
      aria-label={`${item.title} PDF 阅读器`}
    >
      <header className="mnemora-pdf-toolbar">
        <div className="mnemora-pdf-toolbar-group">
          <button
            className="icon-button"
            type="button"
            title="上一页"
            aria-label="上一页"
            disabled={!pdf || currentPage <= 1}
            onClick={() => submitPage(String(currentPage - 1))}
          >
            <ArrowUp size={16} />
          </button>
          <label className="mnemora-pdf-page-input">
            <input
              key={currentPage}
              type="number"
              min={1}
              max={Math.max(1, pageCount)}
              defaultValue={currentPage}
              aria-label="当前页码"
              disabled={!pdf}
              onKeyDown={(event) => {
                if (event.key === "Enter") submitPage(event.currentTarget.value);
              }}
            />
            <span>/ {pageCount || "-"}</span>
          </label>
          <button
            className="icon-button"
            type="button"
            title="下一页"
            aria-label="下一页"
            disabled={!pdf || currentPage >= pageCount}
            onClick={() => submitPage(String(currentPage + 1))}
          >
            <ArrowDown size={16} />
          </button>
        </div>

        <div className="mnemora-pdf-toolbar-title" title={item.title}>
          <BookOpen size={16} />
          <span>{item.title}</span>
        </div>

        <div className="mnemora-pdf-toolbar-group">
          <div className="mnemora-pdf-annotation-colors" aria-label="批注颜色">
            {PDF_ANNOTATION_COLORS.map((color) => (
              <button
                className={`mnemora-pdf-color-swatch mnemora-pdf-color-${color.id}${annotationColor === color.id ? " is-active" : ""}`}
                type="button"
                title={color.label}
                aria-label={`${color.label}批注`}
                aria-pressed={annotationColor === color.id}
                disabled={!pdf}
                key={color.id}
                onClick={() => setAnnotationColor(color.id)}
              />
            ))}
          </div>
          <button
            className={`icon-button${annotationMode === "area" ? " is-active" : ""}`}
            type="button"
            title={annotationMode === "area" ? "退出区域批注" : "区域批注"}
            aria-label={annotationMode === "area" ? "退出区域批注" : "区域批注"}
            aria-pressed={annotationMode === "area"}
            disabled={!pdf}
            onClick={() => setReaderAnnotationMode(annotationMode === "area" ? "text" : "area")}
          >
            <ScanLine size={16} />
          </button>
          <button className="icon-button" type="button" title="缩小" aria-label="缩小" disabled={!pdf} onClick={() => updateZoom(zoom - 0.1)}>
            <Minus size={16} />
          </button>
          <span className="mnemora-pdf-zoom-label">{Math.round(zoom * 100)}%</span>
          <button className="icon-button" type="button" title="放大" aria-label="放大" disabled={!pdf} onClick={() => updateZoom(zoom + 0.1)}>
            <Plus size={16} />
          </button>
          <button className="icon-button" type="button" title="适合宽度" aria-label="适合宽度" disabled={!pdf} onClick={() => updateZoom(1)}>
            <MoveHorizontal size={16} />
          </button>
          <button
            className="icon-button"
            type="button"
            title="在系统阅读器中打开"
            aria-label="在系统阅读器中打开"
            onClick={() => void onOpenExternal(item.id).catch(() => undefined)}
          >
            <ExternalLink size={16} />
          </button>
          <button
            className={`icon-button${searchOpen ? " is-active" : ""}`}
            type="button"
            title="搜索 PDF"
            aria-label="搜索 PDF"
            aria-pressed={searchOpen}
            disabled={!pdf}
            onClick={() => setSearchOpen((open) => !open)}
          >
            <Search size={16} />
          </button>
        </div>
      </header>

      {searchOpen ? (
        <div className="mnemora-pdf-search-bar">
          <Search size={15} aria-hidden="true" />
          <input
            value={searchQuery}
            placeholder="搜索当前 PDF"
            aria-label="搜索当前 PDF"
            onChange={(event) => setSearchQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void runSearch();
            }}
          />
          <button type="button" disabled={searching || !searchQuery.trim()} onClick={() => void runSearch()}>
            {searching ? "搜索中" : "搜索"}
          </button>
          <button className="icon-button" type="button" title="关闭搜索" aria-label="关闭搜索" onClick={() => setSearchOpen(false)}>
            <X size={15} />
          </button>
        </div>
      ) : null}

      {searchOpen && searchResults.length > 0 ? (
        <div className="mnemora-pdf-search-results" role="listbox" aria-label="PDF 搜索结果">
          {searchResults.map((result) => (
            <button
              type="button"
              role="option"
              key={`${result.pageIndex}-${result.snippet}`}
              onClick={() => submitPage(String(result.pageIndex + 1))}
            >
              <strong>第 {result.pageIndex + 1} 页</strong>
              <span>{result.snippet}</span>
            </button>
          ))}
        </div>
      ) : null}

      {annotationError ? (
        <div className="mnemora-pdf-annotation-error" role="alert">
          <span>{annotationError}</span>
          <button type="button" aria-label="关闭批注错误" onClick={() => setAnnotationError("")}>
            <X size={14} />
          </button>
        </div>
      ) : null}

      {textSelection ? (
        <div
          className="mnemora-pdf-selection-toolbar"
          role="toolbar"
          aria-label="PDF 文字批注"
          style={{ left: textSelection.clientX, top: textSelection.clientY }}
          onPointerDown={preserveSelectionToolbarPointer}
        >
          <button type="button" title="高亮" aria-label="高亮" onClick={() => void createTextAnnotation("highlight")}>
            <Highlighter size={15} />
          </button>
          <button type="button" title="下划线" aria-label="下划线" onClick={() => void createTextAnnotation("underline")}>
            <Underline size={15} />
          </button>
          <button type="button" title="问 AI" aria-label="使用此选区询问 AI" onClick={askSelectedText}>
            <MessageCircleQuestion size={15} />
          </button>
          <div className="mnemora-pdf-selection-color-picker" ref={selectionColorMenuRef}>
            <button
              className="mnemora-pdf-selection-color-button"
              type="button"
              title="切换高亮颜色"
              aria-label="切换高亮颜色"
              aria-haspopup="menu"
              aria-expanded={selectionColorMenuOpen}
              onClick={() => setSelectionColorMenuOpen((open) => !open)}
            >
              <span className={`mnemora-pdf-selection-color mnemora-pdf-color-${annotationColor}`} aria-hidden="true" />
            </button>
            {selectionColorMenuOpen ? (
              <div className="mnemora-pdf-selection-color-menu" role="menu" aria-label="选择高亮颜色">
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
          <button type="button" title="取消" aria-label="取消批注" onClick={() => {
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
        onScroll={handleScroll}
        role="document"
        aria-label={status}
      >
        {error ? (
          <div className="mnemora-pdf-reader-state mnemora-pdf-reader-error" role="alert">
            <strong>PDF 无法加载</strong>
            <span>{error}</span>
          </div>
        ) : pdf ? (
          <div className="mnemora-pdf-page-list">
            {Array.from({ length: pageCount }, (_, pageIndex) => (
              <PdfPage
                key={pageIndex}
                pdf={pdf}
                pageIndex={pageIndex}
                zoom={zoom}
                readerWidth={readerWidth}
                annotations={annotationsByPage.get(pageIndex) ?? []}
                annotationMode={annotationMode}
                focusedAnnotationId={focusedAnnotationId}
                scrollContainerRef={scrollContainerRef}
                onRegister={registerPage}
                onTextSelection={setTextSelection}
                onAreaSelection={(targetPageIndex, rect) => {
                  void createAreaAnnotation(targetPageIndex, rect);
                }}
              />
            ))}
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

function normalizeOutline(items: Array<{
  title: string;
  dest: string | unknown[] | null;
  items: Array<unknown>;
}>): PdfOutlineEntry[] {
  const visit = (entries: Array<{
    title: string;
    dest: string | unknown[] | null;
    items: Array<unknown>;
  }>, level: number, prefix: string): PdfOutlineEntry[] => entries.map((entry, index) => ({
    id: `${prefix}-${index}`,
    title: entry.title.trim() || "未命名章节",
    level,
    dest: entry.dest,
    children: visit(
      (entry.items ?? []) as Array<{ title: string; dest: string | unknown[] | null; items: Array<unknown> }>,
      level + 1,
      `${prefix}-${index}`,
    ),
  }));
  return visit(items, 0, "outline");
}

function noteSummary(note: LibraryNote): LibraryNoteSummary {
  return {
    id: note.id,
    itemId: note.itemId,
    itemTitle: note.itemTitle,
    title: note.title,
    contentPreview: note.content.slice(0, 600),
    contentChars: note.content.length,
    createdAt: note.createdAt,
    updatedAt: note.updatedAt,
  };
}

function sortAnnotations(annotations: LibraryAnnotation[]) {
  return [...annotations].sort((left, right) => (
    left.pageIndex - right.pageIndex || left.createdAt - right.createdAt
  ));
}

export default PdfReader;
