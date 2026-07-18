import { invoke } from "@tauri-apps/api/core";
import type { ApiProtocol, AuthScheme, ProviderConfig } from "../types/settings";

/** 只在用户点击测试或获取模型时临时发送给 Rust，不进入普通设置 DTO。 */
export type ProviderConnectionInput = {
  providerId: string;
  baseUrl: string;
  apiKey?: string;
  protocol: ApiProtocol;
  authScheme: AuthScheme;
};

export type ConnectionTestResult = {
  success: boolean;
  latencyMs: number;
  statusCode?: number;
  error?: string;
};

function connectionInput(
  provider: ProviderConfig,
  apiKey: string,
): ProviderConnectionInput {
  return {
    providerId: provider.id,
    baseUrl: provider.baseUrl.trim(),
    apiKey: apiKey.trim() || undefined,
    protocol: provider.protocol,
    authScheme: provider.authScheme,
  };
}

/** 用户手动触发后，按当前协议获取远程模型列表。 */
export function fetchProviderModels(provider: ProviderConfig, apiKey: string) {
  return invoke<string[]>("fetch_provider_models", {
    provider: connectionInput(provider, apiKey),
  });
}

/** 用户手动触发后，发送一次不重试的模型列表请求并返回耗时。 */
export function testProviderConnection(provider: ProviderConfig, apiKey: string) {
  return invoke<ConnectionTestResult>("test_provider_connection", {
    provider: connectionInput(provider, apiKey),
  });
}
