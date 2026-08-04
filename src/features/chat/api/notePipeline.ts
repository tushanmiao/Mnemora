import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { LibraryNote, LibraryNoteSummary } from "../../library/types";

export type NotePipelinePhase =
  | "analyzing"
  | "awaitingOutline"
  | "drafting"
  | "assembling"
  | "persisting"
  | "done"
  | "cancelled"
  | "error";

export interface NotePipelineRun {
  id: string;
  conversationId: string;
  noteId: string | null;
  phase: NotePipelinePhase;
  outlineJson: string;
  selectedSectionIds: string[];
  providerId: string;
  modelId: string;
  maxOutputTokens: number;
  thinkingEnabled: boolean;
  retryAttempts: number;
  completedSectionIds: string[];
  failedSectionIds: string[];
  warnings: string[];
  errorMessage: string | null;
  createdAt: number;
  updatedAt: number;
}

export type NotePipelineEvent =
  | {
      type: "progress";
      runId: string;
      phase: NotePipelinePhase;
      current: number | null;
      total: number | null;
      message: string;
    }
  | { type: "outlineReady"; run: NotePipelineRun }
  | { type: "done"; run: NotePipelineRun; degraded: boolean }
  | { type: "cancelled"; run: NotePipelineRun }
  | { type: "error"; runId: string; message: string };

export interface NoteEditProposal {
  id: string;
  noteId: string;
  conversationId: string;
  sourceMessageId: string | null;
  expectedNoteUpdatedAt: number;
  oldTitle: string;
  newTitle: string;
  oldContent: string;
  newContent: string;
  diff: string;
  createdAt: number;
}

export interface NoteEditPrepareRequest {
  noteId: string;
  conversationId: string;
  selectedText?: string;
  sectionHeading?: string;
  requirement?: string;
}

export interface NoteEditPrepareResult {
  proposal: NoteEditProposal;
  warnings: string[];
}

export interface NoteEditDialogRequest {
  conversationId: string;
  notes: LibraryNoteSummary[];
  noteId: string | null;
  selectedText: string;
  sectionHeading: string;
}

function requireTauri() {
  if (!isTauri()) throw new Error("深度笔记管线只能在桌面应用中运行。");
}

function channel(onEvent: (event: NotePipelineEvent) => void) {
  const value = new Channel<NotePipelineEvent>();
  value.onmessage = onEvent;
  return value;
}

export function startNotePipeline(
  conversationId: string,
  onEvent: (event: NotePipelineEvent) => void,
) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_start", {
    request: { conversationId },
    onEvent: channel(onEvent),
  });
}

export function adjustNotePipeline(
  runId: string,
  requirement: string,
  onEvent: (event: NotePipelineEvent) => void,
) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_adjust", {
    request: { runId, requirement },
    onEvent: channel(onEvent),
  });
}

export function confirmNotePipeline(
  runId: string,
  selectedSectionIds: string[],
  onEvent: (event: NotePipelineEvent) => void,
) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_confirm", {
    request: { runId, selectedSectionIds },
    onEvent: channel(onEvent),
  });
}

export function resumeNotePipeline(
  runId: string,
  onEvent: (event: NotePipelineEvent) => void,
) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_resume", {
    runId,
    onEvent: channel(onEvent),
  });
}

export function cancelNotePipeline(runId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("note_pipeline_cancel", { runId });
}

export function listResumableNotePipelines() {
  if (!isTauri()) return Promise.resolve<NotePipelineRun[]>([]);
  return invoke<NotePipelineRun[]>("note_pipeline_list_resumable");
}

export function getNotePipeline(runId: string) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_get", { runId });
}

export function prepareNoteEdit(request: NoteEditPrepareRequest) {
  requireTauri();
  return invoke<NoteEditPrepareResult>("note_edit_prepare", { request });
}

export function resolveNoteEdit(proposalId: string, accepted: boolean) {
  requireTauri();
  return invoke<LibraryNote | null>("note_edit_resolve", { proposalId, accepted });
}
