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
export type UpdateProxyMode = "system" | "direct" | "manual";

export interface UpdateProxySettings {
  mode: UpdateProxyMode;
  url: string;
}

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

export interface PetSettings {
  enabled: boolean;
  showOnStartup: boolean;
  alwaysOnTop: boolean;
  clickThrough: boolean;
  size: number;
  opacity: number;
  speechBubbles: boolean;
  reducedMotion: boolean;
  taskEvents: boolean;
  selectedPetId: string;
  positionX: number | null;
  positionY: number | null;
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
  noteFontSize: number;
  noteLineHeight: number;
  noteFontPreset: FontPreset;
  noteChineseFontFamily: ChineseFontFamily;
  noteLatinFontFamily: LatinFontFamily;
  launchAtStartup: boolean;
  retryEnabled: boolean;
  retryAttempts: number;
  /** Agent 可执行的业务轮数；达到上限后仍保留一次无工具最终汇总调用。 */
  agentMaxRounds: 5 | 10 | 20 | 50 | 100;
  userDisplayName: string;
  userAvatar: string;
  workingDirectory: string;
  streamEnabled: boolean;
  thinkingEnabled: boolean;
  maxOutputTokens: number;
  responseLanguage: ResponseLanguage;
  systemPrompt: string;
  requestDebugEnabled: boolean;
  /** 是否在普通 Chat 中显示可拖动的 Agent 任务中心；深度笔记始终显示。 */
  showChatTaskProgress: boolean;
  pet: PetSettings;
  updateProxy: UpdateProxySettings;
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

/** 新安装时展示在设置中的全局提示词，用户可以直接修改。 */
export const DEFAULT_GLOBAL_SYSTEM_PROMPT = [
  "你是 Mnemora 的学习与研究助手。",
  "优先直接回答问题，并根据复杂度使用清晰的标题、列表、表格或代码块。",
  "严格区分已知事实、用户材料中的证据、合理推断和仍需确认的内容；没有依据时明确说明。",
  "处理 PDF、图片或附件时，只根据实际收到的内容回答，不编造来源、页码、工具结果或已执行操作。",
  "技能只提供工作方法，不扩大应用权限；遵守用户的权限设置和工具结果。",
].join("\n");

export const CURRENT_APP_SETTINGS_VERSION = 15;

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
    noteFontSize: 16,
    noteLineHeight: 1.85,
    noteFontPreset: "system",
    noteChineseFontFamily: "system",
    noteLatinFontFamily: "system",
    launchAtStartup: false,
    retryEnabled: true,
    retryAttempts: 5,
    agentMaxRounds: 20,
    userDisplayName: "",
    userAvatar: "",
    workingDirectory: "",
    streamEnabled: true,
    thinkingEnabled: false,
    maxOutputTokens: 32_768,
    responseLanguage: "followInput",
    systemPrompt: DEFAULT_GLOBAL_SYSTEM_PROMPT,
    requestDebugEnabled: false,
    showChatTaskProgress: true,
    pet: {
      enabled: false,
      showOnStartup: false,
      alwaysOnTop: true,
      clickThrough: false,
      size: 176,
      opacity: 96,
      speechBubbles: true,
      reducedMotion: false,
      taskEvents: true,
      selectedPetId: "mimo",
      positionX: null,
      positionY: null,
    },
    updateProxy: {
      mode: "system",
      url: "",
    },
    memory: {
      enabled: false,
      injectL1: true,
      allowModelRead: true,
      allowModelWrite: false,
    },
  };
}
