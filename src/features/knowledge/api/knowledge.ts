import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  KnowledgeChunkView,
  KnowledgeConsentStatus,
  KnowledgeDocumentStatus,
  KnowledgeEmbeddingRebuildResult,
  KnowledgeGlobalConsentStatus,
  KnowledgeJobView,
  KnowledgeMineruTokenStatus,
  KnowledgeOverview,
  KnowledgeRebuildResult,
  KnowledgeSearchRequest,
  KnowledgeSearchResponse,
} from "../types";

export function isKnowledgeRuntime() {
  return isTauri();
}

export function getKnowledgeOverview() {
  if (!isTauri()) return Promise.resolve<KnowledgeOverview>(emptyOverview());
  return invoke<KnowledgeOverview>("knowledge_overview");
}

export function listKnowledgeDocuments(limit = 200) {
  if (!isTauri()) return Promise.resolve<KnowledgeDocumentStatus[]>([]);
  return invoke<KnowledgeDocumentStatus[]>("knowledge_list_documents", { limit });
}

export function listKnowledgeJobs(limit = 100) {
  if (!isTauri()) return Promise.resolve<KnowledgeJobView[]>([]);
  return invoke<KnowledgeJobView[]>("knowledge_list_jobs", { limit });
}

export function rebuildKnowledgeAll() {
  if (!isTauri()) return Promise.reject(new Error("知识库重建需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeRebuildResult>("knowledge_rebuild_all");
}

export function rebuildKnowledgeEmbeddings(documentId?: string, force = true) {
  if (!isTauri()) return Promise.reject(new Error("向量索引重建需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeEmbeddingRebuildResult>("knowledge_rebuild_embeddings", {
    documentId: documentId ?? null,
    force,
  });
}

export function rebuildKnowledgeNote(noteId: string) {
  if (!isTauri()) return Promise.reject(new Error("知识库重建需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeDocumentStatus>("knowledge_rebuild_note", { noteId });
}

export function enqueueKnowledgeLiterature(itemId: string) {
  if (!isTauri()) return Promise.reject(new Error("知识库操作需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeDocumentStatus>("knowledge_enqueue_literature", { itemId });
}

export function searchKnowledge(request: KnowledgeSearchRequest) {
  if (!isTauri()) {
    return Promise.resolve<KnowledgeSearchResponse>({
      query: request.query,
      scope: request.scope ?? "library",
      hits: [],
      lexicalDegraded: true,
      insufficientEvidence: true,
      requestedMode: "lexical",
      actualMode: "lexical",
      fallbackReason: null,
      vectorDimensions: null,
    });
  }
  return invoke<KnowledgeSearchResponse>("knowledge_search", { request });
}

export function readKnowledgeChunk(chunkId: string) {
  if (!isTauri()) return Promise.reject(new Error("知识库读取需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeChunkView>("knowledge_read_chunk", { chunkId });
}

export function cancelKnowledgeJob(jobId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("knowledge_cancel_job", { jobId });
}

export function getMineruTokenStatus() {
  if (!isTauri()) return Promise.resolve<KnowledgeMineruTokenStatus>({ configured: false });
  return invoke<KnowledgeMineruTokenStatus>("knowledge_get_mineru_token_status");
}

export function setMineruToken(token: string) {
  if (!isTauri()) return Promise.reject(new Error("MinerU Token 管理需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeMineruTokenStatus>("knowledge_set_mineru_token", { token });
}

export function deleteMineruToken() {
  if (!isTauri()) return Promise.reject(new Error("MinerU Token 管理需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeMineruTokenStatus>("knowledge_delete_mineru_token");
}

export function getLiteratureConsentStatus(itemId: string) {
  if (!isTauri()) return Promise.resolve<KnowledgeConsentStatus | null>(null);
  return invoke<KnowledgeConsentStatus | null>("knowledge_literature_consent_status", { itemId });
}

export function grantLiteratureConsent(itemId: string, scope: "document" | "global") {
  if (!isTauri()) return Promise.reject(new Error("文献授权需要在 Tauri 桌面应用中运行。"));
  return invoke<KnowledgeDocumentStatus>("knowledge_grant_literature_consent", { itemId, scope });
}

export function grantGlobalLiteratureConsent() {
  if (!isTauri()) return Promise.reject(new Error("文献授权需要在 Tauri 桌面应用中运行。"));
  return invoke<number>("knowledge_grant_global_literature_consent");
}

export function revokeLiteratureConsent(itemId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("knowledge_revoke_literature_consent", { itemId });
}

export function revokeGlobalLiteratureConsent() {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("knowledge_revoke_global_literature_consent");
}

export function getGlobalLiteratureConsentStatus() {
  if (!isTauri()) {
    return Promise.resolve<KnowledgeGlobalConsentStatus>({
      state: "none",
      granted: false,
      grantedAt: null,
      revokedAt: null,
    });
  }
  return invoke<KnowledgeGlobalConsentStatus>("knowledge_global_literature_consent_status");
}

function emptyOverview(): KnowledgeOverview {
  return {
    documentCount: 0,
    literatureCount: 0,
    noteCount: 0,
    readyCount: 0,
    pendingCount: 0,
    failedCount: 0,
    activeJobCount: 0,
    fts5Available: false,
    tokenizer: "none",
    lexicalDegraded: true,
    embeddingReadyCount: 0,
    embeddingPendingCount: 0,
    embeddingFailedCount: 0,
    embeddingDimensions: [],
    lastIndexedAt: null,
  };
}
