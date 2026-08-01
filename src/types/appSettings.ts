import type { ModelSettings } from "./modelSettings";

export type InterfaceLanguage = "zh" | "en";
export type ThemeMode = "system" | "light" | "dark";
export type FontPreset = "system" | "academic" | "custom";
export type ChineseFontFamily = "system" | "microsoftYaHei" | "simsun" | "notoSansCjk" | "notoSerifCjk";
export type LatinFontFamily = "system" | "segoeUi" | "inter" | "timesNewRoman" | "georgia";
/** 完整主题方案，决定应用表面、文字和边框的整体色调。 */
export type ThemePreset = "mnemora" | "forest" | "ocean" | "rose" | "paper" | "graphite" | "highContrast";
/** 强调色只影响按钮、选中状态和交互反馈，不覆盖主题表面。 */
export type ThemeColor = "neutral" | "warm" | "cool" | "rose" | "amber" | "violet";
export type ResponseLanguage = "followInput" | "zh" | "zhHant" | "en";

/** 受限的背景 CSS 值；只允许颜色和渐变，不允许完整 CSS 样式表。 */
export interface ThemeBackgroundSettings {
  enabled: boolean;
  css: string;
  surfaceOpacity: number;
}

export interface MemorySettings {
  enabled: boolean;
  injectL1: boolean;
  allowModelRead: boolean;
  allowModelWrite: boolean;
}

/** 非敏感的应用基础设置；API Key 不属于该结构。 */
export interface AppSettings {
  version: number;
  interfaceLanguage: InterfaceLanguage;
  theme: ThemeMode;
  themePreset: ThemePreset;
  themeColor: ThemeColor;
  themeBackground: ThemeBackgroundSettings;
  fontSize: number;
  letterSpacing: number;
  fontPreset: FontPreset;
  chineseFontFamily: ChineseFontFamily;
  latinFontFamily: LatinFontFamily;
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
  requestDebugEnabled: boolean;
  memory: MemorySettings;
}

export interface SettingsBundle {
  version: number;
  appSettings: AppSettings;
  modelSettings: ModelSettings;
  memoryImported?: boolean;
}

export interface SettingsBundleInspection {
  version: number;
  containsMemory: boolean;
  memoryBytes: number;
}

export const CURRENT_APP_SETTINGS_VERSION = 7;

export function createInitialAppSettings(): AppSettings {
  return {
    version: CURRENT_APP_SETTINGS_VERSION,
    interfaceLanguage: "zh",
    theme: "system",
    themePreset: "mnemora",
    themeColor: "neutral",
    themeBackground: {
      enabled: false,
      css: "",
      surfaceOpacity: 92,
    },
    fontSize: 14,
    letterSpacing: 0,
    fontPreset: "system",
    chineseFontFamily: "system",
    latinFontFamily: "system",
    launchAtStartup: false,
    retryEnabled: true,
    retryAttempts: 5,
    userDisplayName: "",
    userAvatar: "",
    workingDirectory: "",
    streamEnabled: true,
    thinkingEnabled: false,
    maxOutputTokens: 32_768,
    responseLanguage: "followInput",
    systemPrompt: "",
    requestDebugEnabled: false,
    memory: {
      enabled: false,
      injectL1: true,
      allowModelRead: true,
      allowModelWrite: false,
    },
  };
}
