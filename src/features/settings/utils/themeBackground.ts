import type { ThemeBackgroundSettings } from "../../../types/appSettings";

export const DEFAULT_SURFACE_OPACITY = 92;
export const MIN_SURFACE_OPACITY = 72;
export const MAX_SURFACE_OPACITY = 100;
export const MAX_THEME_BACKGROUND_CSS_LENGTH = 1_500_000;
const SAFE_IMAGE_SCHEMES = new Set(["https:", "http:", "data:", "asset:", "blob:"]);

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
  if (/\b(?:paint|element|expression)\s*\(/i.test(css)) {
    return "背景样式不允许执行脚本化或浏览器扩展函数。";
  }
  if (/\bjavascript\s*:|@import|\/\*/i.test(css)) {
    return "背景样式包含不安全的脚本、导入或注释。";
  }
  if (/[{};]/.test(css)) {
    return "请输入单个 CSS background 值，而不是完整样式表。";
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
    const name = match[1].toLowerCase();
    if (!ALLOWED_FUNCTIONS.has(name) && !["url", "image-set", "cross-fade"].includes(name)) {
      return `不支持 ${match[1]}() 背景函数。`;
    }
  }

  for (const rawUrl of extractCssUrls(css)) {
    const urlError = validateBackgroundImageUrl(rawUrl);
    if (urlError) return urlError;
  }

  if (!/^[a-zA-Z0-9#(),.%+\-/:_'"?&=\s]+$/.test(css)) {
    return "背景样式包含不支持的字符。";
  }

  // CSS.supports 只在浏览器中可用；单元测试和服务端渲染时跳过这一步。
  if (typeof CSS !== "undefined" && !CSS.supports("background", css)) {
    return "背景样式不是有效的 CSS background 值。";
  }
  return null;
}

export function validateBackgroundImageUrl(value: string): string | null {
  const candidate = value.trim().replace(/^['"]|['"]$/g, "");
  if (!candidate) return "背景图片 URL 不能为空。";
  if (/^data:/i.test(candidate)) {
    if (!/^data:image\/(?:png|jpeg|webp|gif|avif);base64,[a-z0-9+/=\s]+$/i.test(candidate)) {
      return "Data URL 只允许 base64 编码的 PNG、JPEG、WebP、GIF 或 AVIF 图片。";
    }
    if (candidate.length > 1_500_000) return "内嵌背景图片不能超过约 1 MB。";
    return null;
  }
  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return "背景图片必须使用完整的 HTTPS/HTTP URL，或应用生成的 asset/blob URL。";
  }
  if (!SAFE_IMAGE_SCHEMES.has(url.protocol)) return "背景图片只允许 HTTPS、HTTP、data、asset 或 blob URL。";
  if (url.username || url.password) return "背景图片 URL 不能包含用户名或密码。";
  if (url.protocol === "file:") return "背景图片不能直接读取 file:// 本地路径。";
  return null;
}

function extractCssUrls(value: string) {
  return Array.from(value.matchAll(/url\(\s*([^)]*?)\s*\)/gi), (match) => match[1]);
}

export function resolveThemeBackgroundCss(settings: ThemeBackgroundSettings): string | null {
  if (!settings.enabled) return null;
  const css = settings.css.trim();
  return css && !validateThemeBackgroundCss(css) ? css : null;
}
