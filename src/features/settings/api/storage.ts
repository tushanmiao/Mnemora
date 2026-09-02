import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type KnownStorageCategoryId =
  | "conversations"
  | "library"
  | "memory"
  | "prompts"
  | "skills"
  | "usage"
  | "sync"
  | "english"
  | "backgrounds";

export interface StorageCategoryUsage {
  // 后端为了兼容新增数据目录会返回字符串；界面必须为未来分类保留降级显示。
  id: string;
  bytes: number;
}

export interface StorageMigrationResult {
  succeeded: boolean;
  sourcePath: string;
  destinationPath: string;
  completedAt: number;
  error: string | null;
}

export interface StorageStatus {
  currentPath: string;
  defaultPath: string;
  isCustom: boolean;
  available: boolean;
  availabilityError: string | null;
  totalBytes: number;
  categories: StorageCategoryUsage[];
  previousPath: string | null;
  lastMigration: StorageMigrationResult | null;
}

export function getStorageStatus() {
  if (!isTauri()) return Promise.reject(new Error("存储管理仅在桌面应用中可用。"));
  return invoke<StorageStatus>("storage_get_status");
}

export function openStorageDirectory() {
  if (!isTauri()) return Promise.reject(new Error("存储管理仅在桌面应用中可用。"));
  return invoke<void>("storage_open_directory");
}

export async function chooseStorageDirectory(title: string) {
  if (!isTauri()) return null;
  const path = await open({
    title,
    multiple: false,
    directory: true,
  });
  return typeof path === "string" ? path : null;
}

export function migrateStorageData(destination: string) {
  if (!isTauri()) return Promise.reject(new Error("存储管理仅在桌面应用中可用。"));
  return invoke<void>("storage_migrate_data", { destination });
}
