import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  DEFAULT_SYNC_SETTINGS,
  type SyncResult,
  type SyncSettings,
} from "../../../types/syncSettings";

let browserSettings = structuredClone(DEFAULT_SYNC_SETTINGS);

export function loadSyncSettings() {
  if (!isTauri()) return Promise.resolve(structuredClone(browserSettings));
  return invoke<SyncSettings>("sync_load_settings");
}

export function saveSyncSettings(settings: SyncSettings) {
  if (!isTauri()) {
    browserSettings = structuredClone({ ...settings, autoSync: false });
    return Promise.resolve(structuredClone(browserSettings));
  }
  return invoke<SyncSettings>("sync_save_settings", { settings });
}

export function setNotionToken(token: string) {
  if (!isTauri()) return Promise.resolve(true);
  return invoke<boolean>("sync_set_notion_token", { token });
}

export function deleteNotionToken() {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("sync_delete_notion_token");
}

export async function chooseObsidianVault() {
  if (!isTauri()) return null;
  const path = await open({
    title: "选择 Obsidian Vault",
    multiple: false,
    directory: true,
  });
  return typeof path === "string" ? path : null;
}

export function runNoteSync(noteId?: string) {
  if (!isTauri()) throw new Error("笔记同步需要在 Tauri 应用中运行。");
  return invoke<SyncResult>("sync_run", { request: { noteId } });
}
