import {
  getDocument,
  PDFDataRangeTransport,
  type PDFDocumentProxy,
  type PDFDocumentLoadingTask,
} from "pdfjs-dist";
import { getLibraryReadingState, isLibraryRuntime, readLibraryPdfRange } from "../../library/api/library";
import type { LibraryItem } from "../../library/types";
import type { PdfOutlineEntry } from "../context/PdfReaderContext";
import type { TranslationKey } from "../../../i18n/translations";

const RANGE_CHUNK_SIZE = 256 * 1024;

export type ReadingPosition = {
  pageIndex: number;
  scrollOffset: number;
  zoom: number;
};

type Translate = (key: TranslationKey, values?: Record<string, string | number>) => string;

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

export type LoadedPdfDocument = {
  pdf: PDFDocumentProxy;
  position: ReadingPosition;
  outline: PdfOutlineEntry[];
};

export type PdfDocumentLoadHandle = {
  promise: Promise<LoadedPdfDocument>;
  dispose: () => void;
};

export function loadPdfDocument(item: LibraryItem, t: Translate): PdfDocumentLoadHandle {
  let disposed = false;
  let loadingTask: PDFDocumentLoadingTask | null = null;
  let transport: TauriPdfRangeTransport | null = null;
  let destroyStarted = false;
  let terminalError = "";

  const destroyLoadingTask = async () => {
    if (!loadingTask || destroyStarted) return;
    destroyStarted = true;
    await loadingTask.destroy().catch(() => undefined);
  };

  const promise = (async (): Promise<LoadedPdfDocument> => {
    if (!isLibraryRuntime()) throw new Error(t("pdf.desktopOnly"));
    if (!Number.isSafeInteger(item.file.fileSize) || item.file.fileSize <= 0) {
      throw new Error(t("pdf.invalidSize"));
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
    if (disposed) throw new Error(t("pdf.loadFailed"));
    const position: ReadingPosition = {
      pageIndex: savedState.pageIndex,
      scrollOffset: savedState.scrollOffset,
      zoom: savedState.zoom,
    };
    transport = new TauriPdfRangeTransport(item.id, item.file.fileSize, initialData, () => {
      terminalError = t("pdf.readFailed");
      void destroyLoadingTask();
    });
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
      terminalError = t("pdf.passwordRequired");
      void destroyLoadingTask();
    };
    try {
      const pdf = await loadingTask.promise;
      if (disposed) {
        await destroyLoadingTask();
        throw new Error(t("pdf.loadFailed"));
      }
      const rawOutline = await pdf.getOutline().catch(() => null);
      return { pdf, position, outline: normalizeOutline(rawOutline ?? [], t) };
    } catch (error) {
      if (terminalError) throw new Error(terminalError);
      throw error;
    }
  })();

  return {
    promise,
    dispose: () => {
      disposed = true;
      transport?.dispose();
      void destroyLoadingTask();
    },
  };
}

function normalizeOutline(items: Array<{
  title: string;
  dest: string | unknown[] | null;
  items: Array<unknown>;
}>, t: Translate): PdfOutlineEntry[] {
  const visit = (entries: Array<{
    title: string;
    dest: string | unknown[] | null;
    items: Array<unknown>;
  }>, level: number, prefix: string): PdfOutlineEntry[] => entries.map((entry, index) => ({
    id: `${prefix}-${index}`,
    title: entry.title.trim() || t("pdf.unnamedSection"),
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
