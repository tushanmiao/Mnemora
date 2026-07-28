import type {
  LibraryAnnotationColor,
  LibraryAnnotationRect,
} from "../library/types";

/** PDF 文本层抛给阅读器的轻量选区，不保留 DOM Range 引用。 */
export type PdfTextSelection = {
  pageIndex: number;
  text: string;
  rects: LibraryAnnotationRect[];
  clientX: number;
  clientY: number;
};

export const PDF_ANNOTATION_COLORS: Array<{
  id: LibraryAnnotationColor;
  label: string;
}> = [
  { id: "yellow", label: "黄色" },
  { id: "green", label: "绿色" },
  { id: "blue", label: "蓝色" },
  { id: "pink", label: "粉色" },
  { id: "purple", label: "紫色" },
];
