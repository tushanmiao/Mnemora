import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  createEmptyUsageRecords,
  createEmptyUsageSummary,
  type UsageRecordsPage,
  type UsageStatsQuery,
  type UsageSummaryResponse,
} from "../../../types/usage";

export function loadUsageSummary(query: UsageStatsQuery): Promise<UsageSummaryResponse> {
  if (!isTauri()) return Promise.resolve(createEmptyUsageSummary());
  return invoke<UsageSummaryResponse>("usage_get_summary", { query });
}

export function loadUsageRecords(query: UsageStatsQuery): Promise<UsageRecordsPage> {
  if (!isTauri()) return Promise.resolve(createEmptyUsageRecords());
  return invoke<UsageRecordsPage>("usage_get_records", { query });
}

export function clearUsageStats(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("usage_clear");
}
