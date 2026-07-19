import { invoke, isTauri } from "@tauri-apps/api/core";
import type { RequestDebugRecord } from "../../../types/requestDebug";

export function loadRequestDebugRecords(): Promise<RequestDebugRecord[]> {
  if (!isTauri()) return Promise.resolve([]);
  return invoke<RequestDebugRecord[]>("request_debug_get_records");
}

export function clearRequestDebugRecords(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("request_debug_clear");
}
