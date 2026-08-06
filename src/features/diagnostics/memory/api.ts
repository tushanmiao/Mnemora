import { invoke } from "@tauri-apps/api/core";
import type { MemoryProcessTreeSample } from "./types";

export function sampleMemoryProcessTree() {
  return invoke<MemoryProcessTreeSample>("plugin:memory-diagnostics|memory_diagnostics_sample");
}

export function exportMemoryDiagnostics(path: string, report: unknown) {
  return invoke<void>("plugin:memory-diagnostics|memory_diagnostics_export", { path, report });
}
