import { invoke, isTauri } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  AppSettings,
  SettingsBundle,
  SettingsBundleInspection,
} from "../../../types/appSettings";

export function loadApplicationSettings() {
  return invoke<AppSettings>("load_application_settings");
}

export function saveApplicationSettings(settings: AppSettings) {
  return invoke<AppSettings>("save_application_settings", { settings });
}

export async function exportSettingsBundle(includeMemory = false) {
  if (!isTauri()) throw new Error("设置导出需要在 Tauri 应用中运行。");
  const path = await save({
    title: "导出 Mnemora 设置",
    defaultPath: "mnemora-settings.json",
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!path) return false;
  await invoke("export_settings_bundle", { path, includeMemory });
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
  const inspection = await invoke<SettingsBundleInspection>("inspect_settings_bundle", { path });
  const includeMemory = inspection.containsMemory && window.confirm(
    `该备份包含 ${inspection.memoryBytes.toLocaleString()} bytes 的跨会话记忆。是否一并恢复？\n\n选择“取消”仍会导入基础设置、模型供应商和 API Key，但会保留当前记忆。`,
  );
  return invoke<SettingsBundle>("import_settings_bundle", { path, includeMemory });
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
