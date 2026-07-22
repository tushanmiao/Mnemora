import type { ThemeBackgroundSettings } from "../../../types/appSettings";

export const DEFAULT_SURFACE_OPACITY = 92;
export const MIN_SURFACE_OPACITY = 72;
export const MAX_SURFACE_OPACITY = 100;
export const MAX_THEME_BACKGROUND_CSS_LENGTH = 2_048;

const ALLOWED_FUNCTIONS = new Set([
  "linear-gradient",
  "repeating-linear-gradient",
  "radial-gradient",
  "repeating-radial-gradient",
  "conic-gradient",
  "repeating-conic-gradient",
  "rgb",
  "rgba",
  "hsl",
  "hsla",
  "hwb",
  "lab",
  "lch",
  "oklab",
  "oklch",
  "color",
  "color-mix",
]);

/**
 * 校验只作为 background 属性值使用的 CSS 片段。
 * 不接受完整样式表、外部资源或可执行 CSS 函数。
 */
export function validateThemeBackgroundCss(value: string): string | null {
  const css = value.trim();
  if (!css) return null;
  if (css.length > MAX_THEME_BACKGROUND_CSS_LENGTH) {
    return `背景样式不能超过 ${MAX_THEME_BACKGROUND_CSS_LENGTH} 个字符。`;
  }
  if (/\b(url|image-set|cross-fade|paint|element|expression)\s*\(/i.test(css)) {
    return "背景样式不允许加载资源或执行函数。";
  }
  if (/\b(?:javascript|data|file|https?|blob)\s*:/i.test(css)) {
    return "背景样式不允许引用外部或本地 URL。";
  }
  if (/[{};:'"\\]|\/\*/.test(css)) {
    return "背景样式只能包含颜色和渐变值。";
  }

  let depth = 0;
  for (const character of css) {
    if (character === "(") depth += 1;
    if (character === ")") depth -= 1;
    if (depth < 0) return "背景样式的括号不匹配。";
  }
  if (depth !== 0) return "背景样式的括号不匹配。";

  const functionPattern = /([a-z][a-z-]*)\s*\(/gi;
  for (const match of css.matchAll(functionPattern)) {
    if (!ALLOWED_FUNCTIONS.has(match[1].toLowerCase())) {
      return `不支持 ${match[1]}() 背景函数。`;
    }
  }

  if (!/^[a-zA-Z0-9#(),.%+\-/\s]+$/.test(css)) {
    return "背景样式包含不支持的字符。";
  }

  // CSS.supports 只在浏览器中可用；单元测试和服务端渲染时跳过这一步。
  if (typeof CSS !== "undefined" && !CSS.supports("background", css)) {
    return "背景样式不是有效的 CSS background 值。";
  }
  return null;
}

export function resolveThemeBackgroundCss(settings: ThemeBackgroundSettings): string | null {
  if (!settings.enabled) return null;
  const css = settings.css.trim();
  return css && !validateThemeBackgroundCss(css) ? css : null;
}
