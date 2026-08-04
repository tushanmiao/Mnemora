/** Work 文献库的前端命令契约，与 Rust `library/types.rs` 保持 camelCase 对齐。 */

export type LibraryView = "all" | "recent" | "favorites" | "unfiled" | "trash";
export type LibrarySort = "updated" | "title" | "year" | "imported";

export interface LibraryFileSummary {
  id: string;
  originalName: string;
  fileSize: number;
  fileHash: string;
  mimeType: string;
  createdAt: number;
  available: boolean;
}

export interface LibraryItem {
  id: string;
  title: string;
  authors: string[];
  publicationYear: number | null;
  publicationTitle: string;
  doi: string;
  abstractText: string;
  favorite: boolean;
  tags: string[];
  collectionIds: string[];
  collectionNames: string[];
  file: LibraryFileSummary;
  createdAt: number;
  updatedAt: number;
  lastOpenedAt: number | null;
  deletedAt: number | null;
}

export interface LibraryListRequest {
  view: LibraryView;
  searchQuery: string;
  collectionId: string | null;
  sort: LibrarySort;
  offset?: number;
  limit?: number;
}

export interface LibraryListPage {
  items: LibraryItem[];
  offset: number;
  total: number;
  hasMore: boolean;
}

export interface LibraryReadingState {
  itemId: string;
  pageIndex: number;
  scrollOffset: number;
  zoom: number;
  updatedAt: number;
}

export interface LibraryReadingStateUpdate {
  itemId: string;
  pageIndex: number;
  scrollOffset: number;
  zoom: number;
}

export type LibraryAnnotationKind = "highlight" | "underline" | "area";
export type LibraryAnnotationColor = "yellow" | "green" | "blue" | "pink" | "purple";

export interface LibraryAnnotationRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LibraryAnnotation {
  id: string;
  itemId: string;
  kind: LibraryAnnotationKind;
  pageIndex: number;
  color: LibraryAnnotationColor;
  text: string;
  comment: string;
  rects: LibraryAnnotationRect[];
  createdAt: number;
  updatedAt: number;
}

export interface LibraryAnnotationCreate {
  itemId: string;
  kind: LibraryAnnotationKind;
  pageIndex: number;
  color: LibraryAnnotationColor;
  text: string;
  comment?: string;
  rects: LibraryAnnotationRect[];
}

export interface LibraryAnnotationUpdate {
  annotationId: string;
  color: LibraryAnnotationColor;
  comment: string;
}

export interface LibraryNote {
  id: string;
  itemId: string | null;
  itemTitle: string | null;
  title: string;
  content: string;
  /** 所属分组名；null 表示未分类。分组只作用于独立笔记。 */
  groupName: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface LibraryNoteSummary {
  id: string;
  itemId: string | null;
  itemTitle: string | null;
  title: string;
  contentPreview: string;
  contentChars: number;
  groupName: string | null;
  createdAt: number;
  updatedAt: number;
}

/** 笔记分组（SQLite 持久化；空分组也会保留）。 */
export interface LibraryNoteGroup {
  name: string;
  noteCount: number;
  createdAt: number;
}

export interface LibraryNoteCreate {
  itemId?: string | null;
  title: string;
  content: string;
  groupName?: string | null;
}

export type NoteSourceOrigin = "conversation" | "aiSupplement";

export interface NoteSource {
  id: string;
  noteId: string;
  sectionId: string;
  origin: NoteSourceOrigin;
  conversationId: string | null;
  messageId: string | null;
  summarizedUntilMessageId: string | null;
  createdAt: number;
}

export interface NoteSourceCreate {
  sectionId: string;
  origin: NoteSourceOrigin;
  conversationId?: string | null;
  messageId?: string | null;
  summarizedUntilMessageId?: string | null;
}

export interface LibraryNoteUpdate {
  noteId: string;
  title: string;
  content: string;
}

export interface LibraryNoteImportFailure {
  path: string;
  fileName: string;
  error: string;
}

export interface LibraryNoteImportResult {
  imported: LibraryNote[];
  failed: LibraryNoteImportFailure[];
}

export interface LibraryCollection {
  id: string;
  name: string;
  itemCount: number;
  createdAt: number;
  updatedAt: number;
}

export interface LibraryItemUpdate {
  itemId: string;
  title: string;
  authors: string[];
  publicationYear: number | null;
  publicationTitle: string;
  doi: string;
  abstractText: string;
  favorite: boolean;
  tags: string[];
  collectionIds: string[];
}

export interface LibraryImportFailure {
  path: string;
  fileName: string;
  error: string;
}

export interface LibraryImportResult {
  imported: LibraryItem[];
  duplicates: LibraryItem[];
  failed: LibraryImportFailure[];
}
