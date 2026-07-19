export type UsageStatus = "success" | "error" | "stopped";
export type UsageSource = "providerReported" | "missing";

export interface UsageRecord {
  id: string;
  createdAtMs: number;
  durationMs: number;
  source: string;
  operation: string;
  providerId: string;
  providerName: string;
  modelId: string;
  apiModel: string;
  displayName: string;
  protocol: string;
  status: UsageStatus;
  statusCode?: number;
  usageSource: UsageSource;
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  reasoningTokens?: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
  costUsd?: number;
  conversationId?: string;
  messageId?: string;
  errorKind?: string;
}

export interface UsageSummary {
  totalRequests: number;
  successfulRequests: number;
  failedRequests: number;
  stoppedRequests: number;
  providerReportedRequests: number;
  missingUsageRequests: number;
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  averageDurationMs?: number;
  totalCostUsd?: number;
}

export interface UsageTrendPoint {
  bucketIndex: number;
  startedAtMs: number;
  requests: number;
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

export interface UsageGroupStats {
  id: string;
  label: string;
  providerId: string;
  providerName: string;
  modelId?: string;
  apiModel?: string;
  requestCount: number;
  successCount: number;
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  averageDurationMs?: number;
  lastUsedAtMs?: number;
}

export interface UsageStatsResponse {
  summary: UsageSummary;
  trend: UsageTrendPoint[];
  logs: UsageRecord[];
  providerStats: UsageGroupStats[];
  modelStats: UsageGroupStats[];
  totalLogs: number;
  skippedRecords: number;
}

export interface UsageStatsQuery {
  sinceMs: number;
  bucketMs: number;
  bucketCount: number;
  limit?: number;
}

export function createEmptyUsageStats(): UsageStatsResponse {
  return {
    summary: {
      totalRequests: 0,
      successfulRequests: 0,
      failedRequests: 0,
      stoppedRequests: 0,
      providerReportedRequests: 0,
      missingUsageRequests: 0,
      totalTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      reasoningTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    },
    trend: [],
    logs: [],
    providerStats: [],
    modelStats: [],
    totalLogs: 0,
    skippedRecords: 0,
  };
}
