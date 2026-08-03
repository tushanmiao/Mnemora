import type { LibraryNote } from "../../library/types";

const NOTES_LAYOUT_STORAGE_KEY = "mnemora.notes.layout.v1";
export const OUTLINE_DEFAULT_WIDTH = 232;
export const OUTLINE_MIN_WIDTH = 168;
export const OUTLINE_MAX_WIDTH = 440;

export type NotesLayout = { outlineWidth: number; outlineOpen: boolean };

export function loadNotesLayout(): NotesLayout {
  const fallback: NotesLayout = { outlineWidth: OUTLINE_DEFAULT_WIDTH, outlineOpen: true };
  try {
    const parsed: unknown = JSON.parse(
      window.localStorage.getItem(NOTES_LAYOUT_STORAGE_KEY) ?? "{}",
    );
    if (!parsed || typeof parsed !== "object") return fallback;
    const candidate = parsed as Partial<NotesLayout>;
    const width = typeof candidate.outlineWidth === "number" && Number.isFinite(candidate.outlineWidth)
      ? Math.min(Math.max(candidate.outlineWidth, OUTLINE_MIN_WIDTH), OUTLINE_MAX_WIDTH)
      : OUTLINE_DEFAULT_WIDTH;
    return { outlineWidth: width, outlineOpen: candidate.outlineOpen !== false };
  } catch {
    return fallback;
  }
}

export function persistNotesLayout(layout: NotesLayout) {
  try {
    window.localStorage.setItem(NOTES_LAYOUT_STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // 本地存储不可用时布局仅在当前会话内生效。
  }
}

export function revisionHash(note: LibraryNote) {
  return `${note.updatedAt.toString(36)}-${note.content.length.toString(36)}`;
}

export function lineAtOffset(content: string, offset: number) {
  return content.slice(0, offset).split("\n").length;
}

export function noteStats(content: string) {
  const characters = Array.from(content).length;
  const words = content.trim() ? content.trim().split(/\s+/).filter(Boolean).length : 0;
  const readingMinutes = characters === 0 ? 0 : Math.max(1, Math.ceil(characters / 400));
  return { characters, words, readingMinutes };
}

export function formatNoteSize(characters: number) {
  if (characters >= 10_000) return `${(characters / 10_000).toFixed(1)} 万字`;
  return `${characters} 字`;
}

export const NOTE_TIME_FORMATTER = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
});
