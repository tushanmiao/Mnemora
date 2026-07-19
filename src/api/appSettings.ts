import { invoke, isTauri } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AppSettings, SettingsBundle } from "../types/appSettings";

export function loadApplicationSettings() {
  return invoke<AppSettings>("load_application_settings");
}

export function saveApplicationSettings(settings: AppSettings) {
  return invoke<AppSettings>("save_application_settings", { settings });
}

export async function exportSettingsBundle() {
  if (!isTauri()) throw new Error("设置导出需要在 Tauri 应用中运行。");
  const path = await save({
    title: "导出 Mnemora 设置",
    defaultPath: "mnemora-settings.json",
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!path) return false;
  await invoke("export_settings_bundle", { path });
  return true;
}

export async function importSettingsBundle() {
  if (!isTauri()) throw new Error("设置导入需要在 Tauri 应用中运行。");
  const path = await open({
    title: "导入 Mnemora 设置",
    multiple: false,
    directory: false,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (typeof path !== "string") return null;
  return invoke<SettingsBundle>("import_settings_bundle", { path });
}

export async function chooseWorkingDirectory() {
  if (!isTauri()) return null;
  const path = await open({
    title: "选择普通对话工作目录",
    multiple: false,
    directory: true,
  });
  return typeof path === "string" ? path : null;
}
