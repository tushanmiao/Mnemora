import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import type {
  LibraryAnnotation,
  LibraryAnnotationColor,
  LibraryNote,
  LibraryNoteSummary,
} from "../../library/types";
import type { WorkNoteSourceContext } from "../../workspace/types";
import type { PdfCanvasBudget } from "../runtime/pdfCanvasBudget";

export type PdfOutlineEntry = {
  id: string;
  title: string;
  level: number;
  dest: string | unknown[] | null;
  children: PdfOutlineEntry[];
};

export type PdfReaderController = {
  itemId: string;
  pdf: PDFDocumentProxy;
  pageCount: number;
  currentPage: number;
  zoom: number;
  outline: PdfOutlineEntry[];
  /** 主阅读区与缩略图共享同一 GPU Canvas 预算。 */
  canvasBudget: PdfCanvasBudget;
  annotations: LibraryAnnotation[];
  notes: LibraryNoteSummary[];
  annotationsLoading: boolean;
  notesLoading: boolean;
  notesLoaded: boolean;
  annotationError: string;
  noteError: string;
  annotationMode: "text" | "area";
  annotationColor: LibraryAnnotationColor;
  goToPage: (pageIndex: number) => void;
  /** 按用户操作读取单页文本，不缓存全文。 */
  readPageText: (pageIndex: number) => Promise<string>;
  goToAnnotation: (annotation: LibraryAnnotation) => void;
  setAnnotationMode: (mode: "text" | "area") => void;
  setAnnotationColor: (color: LibraryAnnotationColor) => void;
  updateAnnotation: (
    annotationId: string,
    color: LibraryAnnotationColor,
    comment: string,
  ) => Promise<LibraryAnnotation>;
  deleteAnnotation: (annotationId: string) => Promise<boolean>;
  loadNotes: () => Promise<void>;
  createNote: (title: string, content: string) => Promise<LibraryNote>;
  updateNote: (noteId: string, title: string, content: string) => Promise<LibraryNote>;
  deleteNote: (noteId: string) => Promise<boolean>;
  openNote: (
    note: Pick<LibraryNote, "id" | "title">,
    source?: WorkNoteSourceContext,
  ) => void;
};

type PdfReaderContextValue = {
  controller: PdfReaderController | null;
  register: (controller: PdfReaderController) => void;
  unregister: (itemId: string) => void;
};

const PdfReaderContext = createContext<PdfReaderContextValue | null>(null);

export function PdfReaderBridgeProvider({ children }: { children: ReactNode }) {
  const [controller, setController] = useState<PdfReaderController | null>(null);
  const register = useCallback((nextController: PdfReaderController) => {
    setController(nextController);
  }, []);
  const unregister = useCallback((itemId: string) => {
    setController((current) => current?.itemId === itemId ? null : current);
  }, []);
  const value = useMemo(
    () => ({ controller, register, unregister }),
    [controller, register, unregister],
  );

  return <PdfReaderContext.Provider value={value}>{children}</PdfReaderContext.Provider>;
}

export function usePdfReaderBridge() {
  const value = useContext(PdfReaderContext);
  if (!value) throw new Error("usePdfReaderBridge 必须在 PdfReaderBridgeProvider 内使用。");
  return value;
}
