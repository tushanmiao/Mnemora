import { invoke, isTauri } from "@tauri-apps/api/core";

export type PluginSummary = {
  id: string;
  name: string;
  version: string;
  description: string;
  publisher: string;
  enabled: boolean;
  signatureStatus: "unsigned" | "unverified";
  skillIds: string[];
  mcpServerIds: string[];
  permissions: { networkDomains: string[]; secrets: string[] };
  installedAt: number;
  rollbackVersion: string | null;
};

export type PluginOverview = { plugins: PluginSummary[]; warnings: string[] };
export type PluginImportKind = "directory" | "zip";

export function listPlugins(): Promise<PluginOverview> {
  if (!isTauri()) return Promise.resolve({ plugins: [], warnings: [] });
  return invoke<PluginOverview>("plugins_list");
}

export function installPlugin(path: string, kind: PluginImportKind, replaceExisting: boolean, allowUnsigned: boolean) {
  return invoke<PluginSummary>("plugins_install", {
    path,
    request: { kind, replaceExisting, allowUnsigned },
  });
}

export function setPluginEnabled(pluginId: string, enabled: boolean) {
  return invoke<PluginSummary>("plugins_set_enabled", { pluginId, enabled });
}

export function rollbackPlugin(pluginId: string) {
  return invoke<PluginSummary>("plugins_rollback", { pluginId });
}

export function uninstallPlugin(pluginId: string) {
  return invoke<boolean>("plugins_uninstall", { pluginId });
}
