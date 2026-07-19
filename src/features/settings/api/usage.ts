import { invoke, isTauri } from "@tauri-apps/api/core";
import { createEmptyUsageStats, type UsageStatsQuery, type UsageStatsResponse } from "../../../types/usage";

export function loadUsageStats(query: UsageStatsQuery): Promise<UsageStatsResponse> {
  if (!isTauri()) return Promise.resolve(createEmptyUsageStats());
  return invoke<UsageStatsResponse>("usage_get_stats", { query });
}

export function clearUsageStats(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("usage_clear");
}
