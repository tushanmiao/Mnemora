import { invoke, isTauri } from "@tauri-apps/api/core";

export type MemoryLayer = "l1" | "l2";

const browserMemory = new Map<MemoryLayer, string>();

export function loadMemory(layer: MemoryLayer) {
  if (!isTauri()) return Promise.resolve(browserMemory.get(layer) ?? "");
  return invoke<string>("memory_load", { layer });
}

export function saveMemory(layer: MemoryLayer, content: string) {
  if (!isTauri()) {
    browserMemory.set(layer, content);
    return Promise.resolve();
  }
  return invoke<void>("memory_save", { layer, content });
}

export function clearMemory(layer: MemoryLayer) {
  if (!isTauri()) {
    browserMemory.set(layer, "");
    return Promise.resolve();
  }
  return invoke<void>("memory_clear", { layer });
}

export function getMemoryDirectory() {
  if (!isTauri()) return Promise.resolve("浏览器预览不使用本地记忆目录");
  return invoke<string>("memory_get_directory");
}

export function openMemoryDirectory() {
  if (!isTauri()) return Promise.resolve("浏览器预览不使用本地记忆目录");
  return invoke<string>("memory_open_directory");
}
