export type UsageStatus = "success" | "error" | "stopped";
export type UsageSource = "providerReported" | "gatewayNormalized" | "estimated" | "missing";

export interface PricingSnapshot {
  inputPerMillion?: number | null;
  outputPerMillion?: number | null;
  cacheReadPerMillion?: number | null;
  cacheWritePerMillion?: number | null;
  currency: string;
  capturedAtMs: number;
  settingsVersion: number;
}

export interface UsageRecord {
  id: string;
  createdAtMs: number;
  durationMs: number;
  timeToFirstTokenMs?: number | null;
  generationDurationMs?: number | null;
  outputTokensPerSecond?: number | null;
  source: string;
  operation: string;
  providerId: string;
  providerName: string;
  modelId: string;
  apiModel: string;
  displayName: string;
  protocol: string;
  status: UsageStatus;
  statusCode?: number | null;
  usageSource: UsageSource;
  inputTokens?: number | null;
  nonCachedInputTokens?: number | null;
  contextInputTokens?: number | null;
  outputTokens?: number | null;
  totalTokens?: number | null;
  reasoningTokens?: number | null;
  cacheReadTokens?: number | null;
  cacheWriteTokens?: number | null;
  costUsd?: number | null;
  costSource?: string | null;
  pricingSnapshot?: PricingSnapshot | null;
  conversationId?: string | null;
  messageId?: string | null;
  runId?: string | null;
  roundIndex?: number | null;
  callIndex?: number | null;
  parentOperation?: string | null;
  activatedSkillIds: string[];
  toolDefinitionCount: number;
  toolCallCount: number;
  errorKind?: string | null;
}

export interface UsageSummary {
  totalRequests: number;
  successfulRequests: number;
  failedRequests: number;
  stoppedRequests: number;
  providerReportedRequests: number;
  gatewayNormalizedRequests: number;
  estimatedUsageRequests: number;
  missingUsageRequests: number;
  knownUsageRequests: number;
  partialCostRequests: number;
  missingCostRequests: number;
  totalTokens: number;
  inputTokens: number;
  nonCachedInputTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  averageDurationMs?: number | null;
  averageTimeToFirstTokenMs?: number | null;
  averageOutputTokensPerSecond?: number | null;
  totalCostUsd?: number | null;
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
  costUsd: number;
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
  costUsd: number;
  averageDurationMs?: number | null;
  lastUsedAtMs?: number | null;
}

export interface UsageFilterOption {
  id: string;
  label: string;
}

export interface UsageModelFilterOption {
  id: string;
  providerId: string;
  providerName: string;
  modelId: string;
  apiModel: string;
  label: string;
}

export interface UsageFilterOptions {
  providers: UsageFilterOption[];
  models: UsageModelFilterOption[];
  operations: UsageFilterOption[];
}

export interface UsageSummaryResponse {
  summary: UsageSummary;
  trend: UsageTrendPoint[];
  providerStats: UsageGroupStats[];
  modelStats: UsageGroupStats[];
  operationStats: UsageGroupStats[];
  filterOptions: UsageFilterOptions;
  totalLogs: number;
  skippedRecords: number;
}

export interface UsageRecordsPage {
  records: UsageRecord[];
  nextCursor?: string | null;
  hasMore: boolean;
  totalMatching: number;
  skippedRecords: number;
}

export interface UsageStatsQuery {
  sinceMs?: number;
  untilMs?: number;
  source?: string;
  operation?: string;
  status?: UsageStatus;
  providerId?: string;
  modelId?: string;
  protocol?: string;
  usageSource?: UsageSource;
  bucketMs?: number;
  bucketCount?: number;
  cursor?: string;
  limit?: number;
}

export function createEmptyUsageSummary(): UsageSummaryResponse {
  return {
    summary: {
      totalRequests: 0,
      successfulRequests: 0,
      failedRequests: 0,
      stoppedRequests: 0,
      providerReportedRequests: 0,
      gatewayNormalizedRequests: 0,
      estimatedUsageRequests: 0,
      missingUsageRequests: 0,
      knownUsageRequests: 0,
      partialCostRequests: 0,
      missingCostRequests: 0,
      totalTokens: 0,
      inputTokens: 0,
      nonCachedInputTokens: 0,
      outputTokens: 0,
      reasoningTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    },
    trend: [],
    providerStats: [],
    modelStats: [],
    operationStats: [],
    filterOptions: { providers: [], models: [], operations: [] },
    totalLogs: 0,
    skippedRecords: 0,
  };
}

export function createEmptyUsageRecords(): UsageRecordsPage {
  return { records: [], hasMore: false, totalMatching: 0, skippedRecords: 0 };
}
