import { invoke, isTauri } from "@tauri-apps/api/core";
import type { ModelSettings, ProviderApiKeyUpdate } from "../../../types/modelSettings";

export function isTauriRuntime() {
  return isTauri();
}

/** 从 Rust 内存快照读取非敏感配置；不会返回完整 API Key。 */
export function loadModelSettings() {
  return invoke<ModelSettings>("load_model_settings");
}

/** 把非敏感配置写入 Rust 管理的版本化 JSON 文件。 */
export function saveModelSettings(settings: ModelSettings) {
  return invoke<ModelSettings>("save_model_settings", { settings });
}

export function setProviderApiKey(providerId: string, apiKey: string) {
  return invoke<boolean>("set_provider_api_key", { providerId, apiKey });
}

export function deleteProviderApiKey(providerId: string) {
  return invoke<boolean>("delete_provider_api_key", { providerId });
}

/**
 * 先保存供应商结构，再单向写入或删除系统凭据，最后重新读取脱敏后的设置快照。
 * 新供应商必须先进入 Rust 设置，才能以 providerId 建立凭据项。
 */
export async function persistModelSettings(
  settings: ModelSettings,
  apiKeyUpdates: ProviderApiKeyUpdate[],
) {
  const saved = await saveModelSettings(settings);
  const savedProviderIds = new Set(saved.providers.map((provider) => provider.id));

  for (const update of apiKeyUpdates) {
    if (!savedProviderIds.has(update.providerId)) continue;
    if (update.action === "set") {
      await setProviderApiKey(update.providerId, update.apiKey);
    } else {
      await deleteProviderApiKey(update.providerId);
    }
  }

  const refreshed = await loadModelSettings();
  return apiKeyUpdates.length > 0 ? saveModelSettings(refreshed) : refreshed;
}
