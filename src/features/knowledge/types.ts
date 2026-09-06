import type {
  KnowledgeCloudConsentMode,
  KnowledgeScope,
} from "../../types/appSettings";

export type KnowledgeQueryScope = KnowledgeScope;

export interface KnowledgeOverview {
  documentCount: number;
  literatureCount: number;
  noteCount: number;
  readyCount: number;
  pendingCount: number;
  failedCount: number;
  activeJobCount: number;
  fts5Available: boolean;
  tokenizer: string;
  lexicalDegraded: boolean;
  embeddingReadyCount: number;
  embeddingPendingCount: number;
  embeddingFailedCount: number;
  embeddingDimensions: number[];
  lastIndexedAt: number | null;
}

export interface KnowledgeDocumentStatus {
  id: string;
  sourceClass: "literature" | "note" | string;
  sourceKind: "pdf" | "markdown_note" | string;
  sourceId: string;
  title: string;
  state: string;
  cloudConsentState: "not_required" | "awaiting" | "granted" | "revoked" | string;
  extractionQuality: string | null;
  activeRevisionId: string | null;
  sourceHash: string;
  chunkCount: number;
  assetCount: number;
  warningCount: number;
  updatedAt: number;
}

export interface KnowledgeMineruTokenStatus {
  configured: boolean;
}

export interface KnowledgeConsentStatus {
  documentId: string;
  sourceId: string;
  sourceHash: string;
  providerId: string;
  effectiveScope: "document" | "global" | null;
  scope: "document" | "global" | "none" | string;
  granted: boolean;
  documentGranted: boolean;
  globalGranted: boolean;
  documentConsentState: "none" | "granted" | "revoked" | "stale" | string;
  globalConsentState: "none" | "granted" | "revoked" | string;
  documentSourceHashMatches: boolean;
  revoked: boolean;
  documentGrantedAt: number | null;
  globalGrantedAt: number | null;
  documentRevokedAt: number | null;
  globalRevokedAt: number | null;
  grantedAt: number | null;
  revokedAt: number | null;
}

export interface KnowledgeGlobalConsentStatus {
  state: "none" | "granted" | "revoked" | string;
  granted: boolean;
  grantedAt: number | null;
  revokedAt: number | null;
}

export interface KnowledgeJobView {
  id: string;
  jobKind: string;
  documentId: string | null;
  revisionId: string | null;
  state: string;
  stage: string;
  completedUnits: number;
  totalUnits: number;
  errorCode: string | null;
  errorMessage: string | null;
  createdAt: number;
  updatedAt: number;
  finishedAt: number | null;
}

export interface KnowledgeSearchRequest {
  query: string;
  scope?: KnowledgeQueryScope;
  currentLiteratureId?: string | null;
  currentNoteId?: string | null;
  selectedDocumentIds?: string[];
  elementTypes?: string[];
  limit?: number;
}

export interface KnowledgeSearchHit {
  chunkId: string;
  documentId: string;
  sourceClass: string;
  sourceId: string;
  title: string;
  text: string;
  snippet: string;
  headingPath: string[];
  elementTypes: string[];
  pageStart: number | null;
  pageEnd: number | null;
  lineStart: number | null;
  lineEnd: number | null;
  sourceHash: string;
  revisionId: string;
  extractionQuality: string;
  score: number;
  lexicalScore: number | null;
  vectorScore: number | null;
  fusedScore: number | null;
  lexicalRank: number | null;
  vectorRank: number | null;
}

export interface KnowledgeSearchResponse {
  query: string;
  scope: string;
  hits: KnowledgeSearchHit[];
  lexicalDegraded: boolean;
  insufficientEvidence: boolean;
  requestedMode: "lexical" | "vector" | "hybrid" | string;
  actualMode: "lexical" | "vector" | "hybrid" | string;
  fallbackReason: string | null;
  vectorDimensions: number | null;
}

export interface KnowledgeChunkView {
  id: string;
  documentId: string;
  revisionId: string;
  blockKind: string;
  text: string;
  searchText: string;
  headingPath: string[];
  elementIds: string[];
  assetIds: string[];
  pageStart: number | null;
  pageEnd: number | null;
  lineStart: number | null;
  lineEnd: number | null;
  byteStart: number;
  byteEnd: number;
  sourceHash: string;
  extractionQuality: string;
}

export interface KnowledgeRebuildResult {
  queuedPdfCount: number;
  indexedNoteCount: number;
  failedCount: number;
}

export interface KnowledgeEmbeddingRebuildResult {
  queuedJobCount: number;
  cachedChunkCount: number;
  pendingChunkCount: number;
}

export type KnowledgeConsentMode = KnowledgeCloudConsentMode;
