import type { ModelSettings } from "./modelSettings";

export type InterfaceLanguage = "zh" | "en";
export type ThemeMode = "system" | "light" | "dark";
export type ThemeColor = "neutral" | "warm" | "cool";
export type ResponseLanguage = "followInput" | "zh" | "zhHant" | "en";

/** 非敏感的应用基础设置；API Key 不属于该结构。 */
export interface AppSettings {
  version: number;
  interfaceLanguage: InterfaceLanguage;
  theme: ThemeMode;
  themeColor: ThemeColor;
  launchAtStartup: boolean;
  retryEnabled: boolean;
  retryAttempts: number;
  userDisplayName: string;
  userAvatar: string;
  workingDirectory: string;
  streamEnabled: boolean;
  thinkingEnabled: boolean;
  maxOutputTokens: number;
  responseLanguage: ResponseLanguage;
  systemPrompt: string;
}

export interface SettingsBundle {
  version: number;
  appSettings: AppSettings;
  modelSettings: ModelSettings;
}

export const CURRENT_APP_SETTINGS_VERSION = 1;

export function createInitialAppSettings(): AppSettings {
  return {
    version: CURRENT_APP_SETTINGS_VERSION,
    interfaceLanguage: "zh",
    theme: "system",
    themeColor: "neutral",
    launchAtStartup: false,
    retryEnabled: true,
    retryAttempts: 1,
    userDisplayName: "",
    userAvatar: "",
    workingDirectory: "",
    streamEnabled: true,
    thinkingEnabled: false,
    maxOutputTokens: 32_768,
    responseLanguage: "followInput",
    systemPrompt: "",
  };
}
