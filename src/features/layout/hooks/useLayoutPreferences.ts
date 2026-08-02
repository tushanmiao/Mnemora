import { useCallback, useState } from "react";

/** 可调整宽度的界面面板。宽度只影响本地界面布局，不进入模型或对话数据。 */
export type LayoutPanel = "chatSidebar" | "workSidebar" | "workContext" | "notesContext";

export interface LayoutPreferences {
  /** Chat 模式左侧会话栏宽度。 */
  chatSidebarWidth: number;
  /** Work 模式左侧文库栏宽度。 */
  workSidebarWidth: number;
  /** Work 模式右侧上下文面板宽度。 */
  workContextWidth: number;
  /** Notes 模式右侧按需 AI 面板宽度。 */
  notesContextWidth: number;
}

export const LAYOUT_PANEL_LIMITS = {
  chatSidebar: { min: 220, default: 276, max: 380 },
  workSidebar: { min: 220, default: 276, max: 380 },
  workContext: { min: 340, default: 440, max: 620 },
  notesContext: { min: 340, default: 440, max: 620 },
} as const;

export const DEFAULT_LAYOUT_PREFERENCES: LayoutPreferences = {
  chatSidebarWidth: LAYOUT_PANEL_LIMITS.chatSidebar.default,
  workSidebarWidth: LAYOUT_PANEL_LIMITS.workSidebar.default,
  workContextWidth: LAYOUT_PANEL_LIMITS.workContext.default,
  notesContextWidth: LAYOUT_PANEL_LIMITS.notesContext.default,
};

const LAYOUT_STORAGE_KEY = "mnemora.layout-preferences.v1";

const PANEL_KEYS: Record<LayoutPanel, keyof LayoutPreferences> = {
  chatSidebar: "chatSidebarWidth",
  workSidebar: "workSidebarWidth",
  workContext: "workContextWidth",
  notesContext: "notesContextWidth",
};

function boundedNumber(value: unknown, fallback: number, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(Math.max(Math.round(value), min), max);
}

/** 对本地存储内容做宽度级别的容错，避免手动修改存储后破坏布局。 */
export function normalizeLayoutPreferences(value: unknown): LayoutPreferences {
  if (!value || typeof value !== "object") return { ...DEFAULT_LAYOUT_PREFERENCES };
  const candidate = value as Partial<Record<keyof LayoutPreferences, unknown>>;
  return {
    chatSidebarWidth: boundedNumber(
      candidate.chatSidebarWidth,
      DEFAULT_LAYOUT_PREFERENCES.chatSidebarWidth,
      LAYOUT_PANEL_LIMITS.chatSidebar.min,
      LAYOUT_PANEL_LIMITS.chatSidebar.max,
    ),
    workSidebarWidth: boundedNumber(
      candidate.workSidebarWidth,
      DEFAULT_LAYOUT_PREFERENCES.workSidebarWidth,
      LAYOUT_PANEL_LIMITS.workSidebar.min,
      LAYOUT_PANEL_LIMITS.workSidebar.max,
    ),
    workContextWidth: boundedNumber(
      candidate.workContextWidth,
      DEFAULT_LAYOUT_PREFERENCES.workContextWidth,
      LAYOUT_PANEL_LIMITS.workContext.min,
      LAYOUT_PANEL_LIMITS.workContext.max,
    ),
    notesContextWidth: boundedNumber(
      candidate.notesContextWidth,
      DEFAULT_LAYOUT_PREFERENCES.notesContextWidth,
      LAYOUT_PANEL_LIMITS.notesContext.min,
      LAYOUT_PANEL_LIMITS.notesContext.max,
    ),
  };
}

function readLayoutPreferences(): LayoutPreferences {
  if (typeof window === "undefined") return { ...DEFAULT_LAYOUT_PREFERENCES };
  try {
    const raw = window.localStorage.getItem(LAYOUT_STORAGE_KEY);
    return raw ? normalizeLayoutPreferences(JSON.parse(raw)) : { ...DEFAULT_LAYOUT_PREFERENCES };
  } catch {
    return { ...DEFAULT_LAYOUT_PREFERENCES };
  }
}

function writeLayoutPreferences(value: LayoutPreferences) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(value));
  } catch {
    // 隐私模式或存储空间不足时，布局仍可在当前会话中正常使用。
  }
}

export function useLayoutPreferences() {
  const [preferences, setPreferences] = useState<LayoutPreferences>(readLayoutPreferences);

  const savePanelWidth = useCallback((panel: LayoutPanel, width: number) => {
    setPreferences((current) => {
      const key = PANEL_KEYS[panel];
      const next = {
        ...current,
        [key]: Math.round(width),
      };
      writeLayoutPreferences(next);
      return next;
    });
  }, []);

  return { preferences, savePanelWidth };
}
