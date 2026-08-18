import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { LibraryNote, LibraryNoteSummary } from "../../library/types";

export type NotePipelinePhase =
  | "preflight"
  | "analyzing"
  | "awaitingOutline"
  | "compiling"
  | "queued"
  | "drafting"
  | "validating"
  | "replanning"
  | "assembling"
  | "persisting"
  | "paused"
  | "blocked"
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
  inputSnapshotHash: string;
  currentPlanVersion: number;
  executionVersion: number;
  budgetJson: string;
  preflightJson: string;
  sidecarJson: string;
  idempotencyKey: string;
  completedSectionIds: string[];
  failedSectionIds: string[];
  warnings: string[];
  errorMessage: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface DeepNoteCapabilities {
  tools: boolean;
  vision: boolean | null;
  reasoning: boolean | null;
  structuredOutputs: boolean;
}

export interface DeepNotePreflight {
  ready: boolean;
  model: {
    providerId: string;
    modelId: string;
    apiModel: string;
    contextWindowTokens: number | null;
    capabilities: DeepNoteCapabilities;
  };
  requiresTools: boolean;
  requiresVision: boolean;
  missingCapabilities: string[];
  warnings: string[];
  attachmentIds: string[];
}

export interface DeepNoteBudget {
  semanticCallLimit: number;
  semanticCallsUsed: number;
  nodeAttemptLimit: number;
  sectionRevisionLimit: number;
  replanLimit: number;
  replansUsed: number;
  maxParallelNodes: number;
}

export interface DeepNoteContextBudget {
  contextWindowTokens: number | null;
  estimatedInputTokens: number;
  plannerOutputReserveTokens: number;
  promptOverheadTokens: number;
  safetyMarginTokens: number;
  usableInputTokens: number;
  directInputLimitTokens: number;
  chunkTargetTokens: number;
  chunkCount: number;
  processedChunkCount: number;
  totalMessageCount: number;
  processedMessageCount: number;
  coverageComplete: boolean;
  omittedMessageIds: string[];
}

export interface NotePipelineActivity {
  kind: string;
  attempt: number;
  maxRetries: number;
  startedAt: number;
  delayMs: number | null;
  lastError: string | null;
}

export interface DeepNoteDagNode {
  nodeId: string;
  nodeType: string;
  sectionId: string | null;
  dependsOn: string[];
  status: string;
  attemptCount: number;
  evidenceIds: string[];
  validationJson: string;
  errorMessage: string | null;
}

export type DeepNoteSectionStatus =
  | "pending"
  | "ready"
  | "inProgress"
  | "completed"
  | "needsReview"
  | "needsRevision"
  | "failed"
  | "blocked"
  | "skipped"
  | "interrupted";

export interface DeepNoteSectionProgress {
  sectionId: string;
  position: number;
  status: DeepNoteSectionStatus;
  attemptCount: number;
  revisionCount: number;
  errorMessage: string | null;
  markdownChars: number;
  updatedAt: number;
}

export interface DeepNoteRunDetail {
  run: NotePipelineRun;
  preflight: DeepNotePreflight | null;
  inputSnapshot: { messageIds: string[]; attachmentIds: string[]; createdAt: number } | null;
  planVersion: {
    planId: string;
    version: number;
    plan: import("../notePipeline/outlineSchema").DeepNoteOutline;
    compiledDag: DeepNoteDagNode[];
    planHash: string;
    revisionReason: string;
    confirmedAt: number | null;
  } | null;
  budget: DeepNoteBudget;
  contextBudget: DeepNoteContextBudget;
  sourceChunkCount: number;
  nodes: DeepNoteDagNode[];
  sections: DeepNoteSectionProgress[];
  sourceChunks: unknown[];
  evidence: unknown[];
  ledger: Record<string, unknown>;
  events: Array<{
    sequence: number;
    eventType: string;
    nodeId: string | null;
    payloadJson: string;
    createdAt: number;
  }>;
  markdownPreview: string;
  sidecarJson: string;
}

export type NotePipelineEvent =
  | {
      type: "progress";
      runId: string;
      phase: NotePipelinePhase;
      current: number | null;
      total: number | null;
      message: string;
      activity: NotePipelineActivity | null;
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

export function pauseNotePipeline(runId: string) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_pause", { runId });
}

export function listResumableNotePipelines() {
  if (!isTauri()) return Promise.resolve<NotePipelineRun[]>([]);
  return invoke<NotePipelineRun[]>("note_pipeline_list_resumable");
}

export function getNotePipeline(runId: string) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_get", { runId });
}

export function getNotePipelineDetail(runId: string) {
  requireTauri();
  return invoke<DeepNoteRunDetail>("note_pipeline_get_detail", { runId });
}

export function prepareNoteEdit(request: NoteEditPrepareRequest) {
  requireTauri();
  return invoke<NoteEditPrepareResult>("note_edit_prepare", { request });
}

export function resolveNoteEdit(proposalId: string, accepted: boolean) {
  requireTauri();
  return invoke<LibraryNote | null>("note_edit_resolve", { proposalId, accepted });
}
