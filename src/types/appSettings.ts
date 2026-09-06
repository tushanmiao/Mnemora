import type { ModelSettings } from "./modelSettings";

export type InterfaceLanguage = "zh" | "en";
export type ThemeMode = "system" | "light" | "dark";
export type FontPreset = "system" | "academic" | "custom";
export type ChineseFontFamily = "system" | "microsoftYaHei" | "simsun" | "notoSansCjk" | "notoSerifCjk";
export type LatinFontFamily = "system" | "segoeUi" | "inter" | "timesNewRoman" | "georgia";
/** 完整主题方案，决定应用表面、文字和边框的整体色调。 */
export type ThemePreset =
  | "dawn" | "lamp" | "graphite"
  | "xuan" | "cyanotype" | "paper"
  | "mnemora" | "forest" | "ocean" | "rose"
  | "highContrast";
/** 强调色只影响按钮、选中状态和交互反馈，不覆盖主题表面。 */
export type ThemeColor = "neutral" | "warm" | "cool" | "rose" | "amber" | "violet";
export type ResponseLanguage = "followInput" | "zh" | "zhHant" | "en";
export type UpdateProxyMode = "system" | "direct" | "manual";
export interface NoteEditorSettings {
  defaultMode: "live" | "source" | "read";
  autosaveEnabled: boolean;
  autosaveDelayMs: number;
  lineNumbers: boolean;
  wordWrap: boolean;
  tabSize: 2 | 4 | 8;
  focusMode: boolean;
  typewriterMode: boolean;
  spellcheck: boolean;
  renderPolicy: "auto" | "sourceOnly";
}
export const DEFAULT_NOTE_EDITOR_SETTINGS: NoteEditorSettings = {
  defaultMode: "live", autosaveEnabled: true, autosaveDelayMs: 700, lineNumbers: true,
  wordWrap: true, tabSize: 2, focusMode: false, typewriterMode: false, spellcheck: false, renderPolicy: "auto",
};

export interface UpdateProxySettings {
  mode: UpdateProxyMode;
  url: string;
}

/** 受限的 background 属性值；允许颜色、渐变和安全图片 URL，不允许完整样式表。 */
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

/** 知识库默认访问边界；Chat 内容不会因为任何选项自动成为知识源。 */
export type KnowledgeScope = "library" | "currentLiterature" | "currentNote";
export type KnowledgeRetrievalMode = "lexical" | "vector" | "hybrid";
export type KnowledgeCloudConsentMode = "ask" | "document" | "global";
export type KnowledgeBatchStrategy = "pageBatches" | "manualSplit" | "reject";

/**
 * PDF/Markdown 知识库的非敏感策略。
 *
 * MinerU Token 不属于此结构，必须由桌面端 SecretStore 管理；这里最多保存
 * endpoint、解析偏好和预算。索引本身是可重建派生数据，不会改变 library 的
 * 业务权威性。
 */
export interface KnowledgeSettings {
  enabled: boolean;
  autoRetrieve: boolean;
  defaultScope: KnowledgeScope;
  retrievalMode: KnowledgeRetrievalMode;
  embeddingProvider: string;
  embeddingModel: string;
  chunkTargetChars: number;
  chunkMaxChars: number;
  chunkOverlapChars: number;
  topK: number;
  contextMaxBytes: number;
  includeAnnotations: boolean;
  groundedWork: boolean;
  mineruCloudEnabled: boolean;
  mineruEndpoint: string;
  mineruModel: "vlm" | "pipeline";
  mineruOcrEnabled: boolean;
  mineruFormulaEnabled: boolean;
  mineruTableEnabled: boolean;
  mineruFigureEnabled: boolean;
  mineruLanguage: string;
  mineruConsentMode: KnowledgeCloudConsentMode;
  autoParseImportedPdf: boolean;
  allowLocalTextFallback: boolean;
  remotePageBudgetPerDay: number;
  remoteTaskBudgetPerDay: number;
  batchStrategy: KnowledgeBatchStrategy;
  networkTimeoutSeconds: number;
  indexConcurrency: number;
  markdownAssetsEnabled: boolean;
  embeddingEnabled: boolean;
  hybridEnabled: boolean;
  allowRemoteEmbedding: boolean;
  externalMcpEnabled: boolean;
  debugRetrieval: boolean;
}

export interface PetSettings {
  enabled: boolean;
  showOnStartup: boolean;
  alwaysOnTop: boolean;
  clickThrough: boolean;
  locked: boolean;
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
  noteEditor: NoteEditorSettings;
  launchAtStartup: boolean;
  retryEnabled: boolean;
  retryAttempts: number;
  /** Agent 可执行的业务轮数；达到上限后仍保留一次无工具最终汇总调用。 */
  agentMaxRounds: 5 | 10 | 20 | 50 | 100;
  userDisplayName: string;
  userAvatar: string;
  workingDirectory: string;
  streamEnabled: boolean;
  /**
   * 深度笔记的模型调用是否走流式。与 `streamEnabled` 无关（后者只决定聊天调哪个命令）。
   * 目的是保活：非流式请求生成期间连接静默，容易撞上中转站的 idle 超时。
   * 默认开启；流式失败会自动回落非流式并记一次告警。
   */
  deepNoteStreamKeepalive: boolean;
  thinkingEnabled: boolean;
  maxOutputTokens: number;
  responseLanguage: ResponseLanguage;
  systemPrompt: string;
  requestDebugEnabled: boolean;
  /** 是否在普通 Chat 中显示可拖动的 Agent 任务中心；深度笔记始终显示。 */
  showChatTaskProgress: boolean;
  pet: PetSettings;
  /** 历史字段名保留兼容；该策略同时用于网页工具和应用更新。 */
  updateProxy: UpdateProxySettings;
  memory: MemorySettings;
  knowledge: KnowledgeSettings;
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

export const CURRENT_APP_SETTINGS_VERSION = 18;

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
    noteEditor: { ...DEFAULT_NOTE_EDITOR_SETTINGS },
    launchAtStartup: false,
    retryEnabled: true,
    retryAttempts: 5,
    agentMaxRounds: 20,
    userDisplayName: "",
    userAvatar: "",
    workingDirectory: "",
    streamEnabled: true,
    deepNoteStreamKeepalive: true,
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
      locked: true,
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
    knowledge: {
      enabled: true,
      autoRetrieve: false,
      defaultScope: "library",
      retrievalMode: "lexical",
      embeddingProvider: "",
      embeddingModel: "",
      chunkTargetChars: 1_600,
      chunkMaxChars: 2_400,
      chunkOverlapChars: 200,
      topK: 8,
      contextMaxBytes: 64 * 1024,
      includeAnnotations: false,
      groundedWork: true,
      mineruCloudEnabled: true,
      mineruEndpoint: "https://mineru.net/api/v4",
      mineruModel: "vlm",
      mineruOcrEnabled: true,
      mineruFormulaEnabled: true,
      mineruTableEnabled: true,
      mineruFigureEnabled: true,
      mineruLanguage: "ch",
      mineruConsentMode: "ask",
      autoParseImportedPdf: false,
      allowLocalTextFallback: true,
      remotePageBudgetPerDay: 1_000,
      remoteTaskBudgetPerDay: 20,
      batchStrategy: "pageBatches",
      networkTimeoutSeconds: 120,
      indexConcurrency: 2,
      markdownAssetsEnabled: true,
      embeddingEnabled: false,
      hybridEnabled: false,
      allowRemoteEmbedding: false,
      externalMcpEnabled: false,
      debugRetrieval: false,
    },
  };
}
