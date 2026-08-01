import type {
  AppSettings,
  ChineseFontFamily,
  FontPreset,
  LatinFontFamily,
} from "../../../types/appSettings";

export const FONT_PRESET_VALUES: Record<Exclude<FontPreset, "custom">, Pick<AppSettings, "chineseFontFamily" | "latinFontFamily">> = {
  system: { chineseFontFamily: "system", latinFontFamily: "system" },
  academic: { chineseFontFamily: "simsun", latinFontFamily: "timesNewRoman" },
};

const CHINESE_FONT_STACKS: Record<ChineseFontFamily, string> = {
  system: '"Microsoft YaHei UI", "Microsoft YaHei", "PingFang SC", sans-serif',
  microsoftYaHei: '"Microsoft YaHei UI", "Microsoft YaHei", sans-serif',
  simsun: 'SimSun, "Songti SC", serif',
  notoSansCjk: '"Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei", sans-serif',
  notoSerifCjk: '"Noto Serif CJK SC", "Source Han Serif SC", SimSun, serif',
};

const LATIN_FONT_STACKS: Record<LatinFontFamily, string> = {
  system: 'Inter, "Segoe UI", system-ui, sans-serif',
  segoeUi: '"Segoe UI", Arial, sans-serif',
  inter: 'Inter, "Segoe UI", Arial, sans-serif',
  timesNewRoman: '"Times New Roman", Times, serif',
  georgia: 'Georgia, "Times New Roman", serif',
};

/** 字体标识映射为固定 CSS 栈，不接受用户输入的任意 CSS。 */
export function resolveReadingFontFamily(settings: Pick<AppSettings, "chineseFontFamily" | "latinFontFamily">) {
  return `${LATIN_FONT_STACKS[settings.latinFontFamily]}, ${CHINESE_FONT_STACKS[settings.chineseFontFamily]}`;
}
