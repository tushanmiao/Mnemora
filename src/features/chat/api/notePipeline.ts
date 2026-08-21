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
  abandoned?: boolean;
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
  requiresLocalReaders?: boolean;
  requiresVision: boolean;
  localReaders?: { text: boolean; pdf: boolean; docx: boolean; xlsx: boolean };
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
  callId: string;
  operation: string;
  attempt: number;
  maxRetries: number;
  startedAt: number;
  timeoutMs: number;
  delayMs: number | null;
  lastError: string | null;
}

export interface NotePipelineEventRecord {
  sequence: number;
  eventType: string;
  nodeId: string | null;
  payloadJson: string;
  createdAt: number;
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
  sourceChunks: DeepNoteSourceChunk[];
  evidence: DeepNoteEvidenceArtifact[];
  ledger: Record<string, unknown>;
  skillProfiles?: Record<string, unknown>;
  events: NotePipelineEventRecord[];
  markdownPreview: string;
  sidecarJson: string;
}

export interface DeepNoteSourceChunk {
  chunkId: string;
  sourceKind: string;
  sourceId: string;
  messageId: string | null;
  attachmentId: string | null;
  libraryItemId: string | null;
  location: string;
  excerpt: string;
  contentHash: string;
  ocrConfidence: number | null;
}

export interface DeepNoteEvidenceArtifact {
  evidenceId: string;
  sectionId: string;
  sourceChunkIds: string[];
  claim: string;
  modelSynthesis: string;
  sourceExcerpt: string;
  supportLevel: string;
  status: string;
  contentHash: string;
  createdAt: number;
}

export interface DeepNoteStartInspection {
  status: "new" | "updateAvailable" | "upToDate" | "invalidated";
  noteId: string | null;
  noteTitle: string | null;
  coveredMessageId: string | null;
  coveredMessageCount: number;
  newMessageCount: number;
  message: string;
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
  | { type: "paused"; run: NotePipelineRun }
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
  replaceInvalidated = false,
) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_start", {
    request: { conversationId, replaceInvalidated },
    onEvent: channel(onEvent),
  });
}

export function inspectNotePipelineStart(conversationId: string) {
  requireTauri();
  return invoke<DeepNoteStartInspection>("note_pipeline_inspect_start", { conversationId });
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

export function retryNotePipeline(
  runId: string,
  onEvent: (event: NotePipelineEvent) => void,
) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_retry", {
    runId,
    onEvent: channel(onEvent),
  });
}

export function restartNotePipeline(
  runId: string,
  onEvent: (event: NotePipelineEvent) => void,
) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_restart", {
    runId,
    onEvent: channel(onEvent),
  });
}

export function cancelNotePipeline(runId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("note_pipeline_cancel", { runId });
}

export function abandonNotePipeline(runId: string) {
  requireTauri();
  return invoke<NotePipelineRun>("note_pipeline_abandon", { runId });
}

export function abandonNotePipelinesForConversation(conversationId: string) {
  requireTauri();
  return invoke<number>("note_pipeline_abandon_for_conversation", { conversationId });
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
