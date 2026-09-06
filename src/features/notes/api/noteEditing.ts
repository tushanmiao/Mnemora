import { invoke } from "@tauri-apps/api/core";
import type { LibraryNote } from "../../library/types";

export type NoteEditorMode = "live" | "source" | "read";
export type NoteSaveReason = "typing" | "explicitSave" | "aiApply" | "restore" | "normalize";
export interface NoteDraft {
  noteId: string;
  sessionId: string;
  generation: number;
  baseVersion: string;
  title: string;
  content: string;
  updatedAt: number;
}
export interface NoteEditingSnapshot {
  note: LibraryNote;
  noteVersion: string;
  contentHash: string;
  diskHash: string | null;
  externalContent: string | null;
  sourceMissing: boolean;
  drafts: NoteDraft[];
  stagedImages?: { token: string; relativePath: string; contentHash: string; mimeType: string }[];
}
export interface SaveNoteRequest {
  noteId: string;
  sessionId: string;
  operationId: string;
  draftGeneration: number;
  expectedNoteVersion: string;
  expectedContentHash: string;
  expectedDiskHash: string | null;
  title: string;
  markdown: string;
  acceptExternalChange: boolean;
  reason: NoteSaveReason;
}
export interface SaveNoteReceipt {
  operationId: string;
  draftGeneration: number;
  noteId: string;
  noteVersion: string;
  contentHash: string;
  title: string;
  committedMarkdown: string;
  updatedAt: number;
}
export interface NoteVersionEntry {
  id: string;
  title: string;
  content: string;
  contentHash: string;
  reason: string;
  createdAt: number;
  pinned: boolean;
}
export const noteEditingApi = {
  load: (noteId: string) => invoke<NoteEditingSnapshot>("note_editor_load", { noteId }),
  save: (request: SaveNoteRequest) => invoke<SaveNoteReceipt>("note_editor_save", { request }),
  checkpoint: (draft: NoteDraft) => invoke<void>("note_editor_checkpoint", { draft }),
  discard: (draft: NoteDraft) => invoke<void>("note_editor_discard_draft", {
    noteId: draft.noteId, sessionId: draft.sessionId, generation: draft.generation,
  }),
  versions: (noteId: string) => invoke<NoteVersionEntry[]>("note_editor_versions", { noteId }),
  pin: (noteId: string, versionId: string, pinned: boolean) => invoke<void>("note_editor_pin_version", { noteId, versionId, pinned }),
  copyVersion: (noteId: string, versionId: string) => invoke<LibraryNote>("note_editor_copy_version", { noteId, versionId }),
  stageImage: (noteId: string, sessionId: string, name: string, dataBase64: string) => invoke<{
    token: string; relativePath: string; contentHash: string; mimeType: string;
  }>("note_editor_stage_image", { noteId, sessionId, name, dataBase64 }),
};
