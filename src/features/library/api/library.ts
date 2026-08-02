import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  LibraryAnnotation,
  LibraryAnnotationCreate,
  LibraryAnnotationUpdate,
  LibraryCollection,
  LibraryImportResult,
  LibraryItem,
  LibraryItemUpdate,
  LibraryListPage,
  LibraryListRequest,
  LibraryNote,
  LibraryNoteImportResult,
  LibraryNoteCreate,
  LibraryNoteSummary,
  LibraryNoteUpdate,
  LibraryReadingState,
  LibraryReadingStateUpdate,
} from "../types";

export function isLibraryRuntime() {
  return isTauri();
}

export async function chooseLibraryPdfFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  const selected = await open({
    title: "导入 PDF 文献",
    multiple: true,
    directory: false,
    filters: [{ name: "PDF 文献", extensions: ["pdf"] }],
  });
  if (typeof selected === "string") return [selected];
  return selected ?? [];
}

export async function chooseLibraryMarkdownFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  const selected = await open({
    title: "导入 Markdown 笔记",
    multiple: true,
    directory: false,
    filters: [{ name: "Markdown 笔记", extensions: ["md", "markdown"] }],
  });
  if (typeof selected === "string") return [selected];
  return selected ?? [];
}

export function listLibraryItems(request: LibraryListRequest) {
  if (!isTauri()) {
    return Promise.resolve<LibraryListPage>({ items: [], offset: 0, total: 0, hasMore: false });
  }
  return invoke<LibraryListPage>("library_list_items", { request });
}

export function getLibraryItem(itemId: string) {
  return invoke<LibraryItem>("library_get_item", { itemId });
}

export function importLibraryPdfs(paths: string[], collectionId: string | null) {
  return invoke<LibraryImportResult>("library_import_pdfs", { paths, collectionId });
}

export function updateLibraryItem(update: LibraryItemUpdate) {
  return invoke<LibraryItem>("library_update_item", { update });
}

export function setLibraryItemFavorite(itemId: string, favorite: boolean) {
  return invoke<LibraryItem>("library_set_favorite", { itemId, favorite });
}

export function moveLibraryItemToTrash(itemId: string) {
  return invoke<LibraryItem>("library_move_to_trash", { itemId });
}

export function restoreLibraryItem(itemId: string) {
  return invoke<LibraryItem>("library_restore_item", { itemId });
}

export function deleteLibraryItemPermanently(itemId: string) {
  return invoke<boolean>("library_delete_permanently", { itemId });
}

export function markLibraryItemOpened(itemId: string) {
  return invoke<LibraryItem>("library_mark_opened", { itemId });
}

export function openLibraryItem(itemId: string) {
  return invoke<LibraryItem>("library_open_item", { itemId });
}

export async function readLibraryPdfRange(
  itemId: string,
  start: number,
  end: number,
): Promise<Uint8Array> {
  if (!isTauri()) throw new Error("PDF 阅读器只能在桌面应用中使用。");
  const raw = await invoke<ArrayBuffer | Uint8Array | number[]>("library_read_pdf_range", {
    itemId,
    start,
    end,
  });
  if (raw instanceof Uint8Array) return raw;
  if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
  return Uint8Array.from(raw);
}

export function getLibraryReadingState(itemId: string) {
  return invoke<LibraryReadingState>("library_get_reading_state", { itemId });
}

export function saveLibraryReadingState(update: LibraryReadingStateUpdate) {
  return invoke<LibraryReadingState>("library_save_reading_state", { update });
}

export function listLibraryAnnotations(itemId: string) {
  if (!isTauri()) return Promise.resolve<LibraryAnnotation[]>([]);
  return invoke<LibraryAnnotation[]>("library_list_annotations", { itemId });
}

export function createLibraryAnnotation(create: LibraryAnnotationCreate) {
  return invoke<LibraryAnnotation>("library_create_annotation", { create });
}

export function updateLibraryAnnotation(update: LibraryAnnotationUpdate) {
  return invoke<LibraryAnnotation>("library_update_annotation", { update });
}

export function deleteLibraryAnnotation(annotationId: string) {
  return invoke<boolean>("library_delete_annotation", { annotationId });
}

export function listLibraryNotes(itemId?: string | null) {
  if (!isTauri()) return Promise.resolve<LibraryNoteSummary[]>([]);
  return invoke<LibraryNoteSummary[]>("library_list_notes", { itemId: itemId ?? null });
}

export function getLibraryNote(noteId: string) {
  return invoke<LibraryNote>("library_get_note", { noteId });
}

export function createLibraryNote(create: LibraryNoteCreate) {
  return invoke<LibraryNote>("library_create_note", { create });
}

export function importLibraryMarkdownNotes(paths: string[]) {
  return invoke<LibraryNoteImportResult>("library_import_markdown_notes", { paths });
}

export function updateLibraryNote(update: LibraryNoteUpdate) {
  return invoke<LibraryNote>("library_update_note", { update });
}

export function deleteLibraryNote(noteId: string) {
  return invoke<boolean>("library_delete_note", { noteId });
}

export function listLibraryCollections() {
  if (!isTauri()) return Promise.resolve<LibraryCollection[]>([]);
  return invoke<LibraryCollection[]>("library_list_collections");
}

export function createLibraryCollection(name: string) {
  return invoke<LibraryCollection>("library_create_collection", { name });
}

export function renameLibraryCollection(collectionId: string, name: string) {
  return invoke<void>("library_rename_collection", { collectionId, name });
}

export function deleteLibraryCollection(collectionId: string) {
  return invoke<boolean>("library_delete_collection", { collectionId });
}
