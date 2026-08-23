import { MARKDOWN_RENDER_LIMITS } from "./renderLimits";

const DANGEROUS_SVG_TAGS = new Set(["script", "iframe", "object", "embed", "image"]);
const XML_VOID_TAGS = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]);

// Kept as a compatibility export for callers that previously imported the
// large-diagram predicate from the security module. Layout policy now lives in
// mermaidLayout so rendering and viewer sizing share one contract.
export { isLargeMermaidDiagram } from "./mermaidLayout";

export type MermaidSvgMetrics = {
  width: number;
  height: number;
  aspectRatio: number;
  svgChars?: number;
  elementCount?: number;
  foreignObjectCount?: number;
  viewerSafe?: boolean;
};

export type SanitizedMermaidSvg = {
  svg: string;
  metrics: MermaidSvgMetrics;
};

/**
 * Mermaid's htmlLabels renderer emits HTML void elements (most notably
 * `<br>`) inside SVG foreignObject nodes. That is valid HTML, but the SVG is
 * subsequently parsed as XML by the sanitizer and Shadow DOM renderer. Make
 * the contract explicit at that boundary instead of disabling htmlLabels.
 */
export function normalizeMermaidSvgForXml(svg: string) {
  return svg.replace(/<([a-z][\w:.-]*)(\s[^<>]*?)?\s*\/?\s*>/gi, (tag, name: string, attributes = "") => {
    if (!XML_VOID_TAGS.has(name.toLowerCase()) || /\/\s*>$/.test(tag)) return tag;
    return `<${name}${attributes.trimEnd()} />`;
  });
}

/** Mermaid 输出由受信任的渲染器生成，但仍移除脚本、外链和事件属性。 */
export function sanitizeMermaidSvg(svg: string): SanitizedMermaidSvg {
  const normalizedSvg = normalizeMermaidSvgForXml(svg);
  if (typeof DOMParser === "undefined" || typeof document === "undefined") {
    return { svg: normalizedSvg, metrics: measureMermaidViewerBudget(normalizedSvg) };
  }
  const parsed = new DOMParser().parseFromString(normalizedSvg, "image/svg+xml");
  const root = parsed.documentElement;
  const parserError = parsed.querySelector("parsererror");
  if (parserError || !root || root.tagName.toLowerCase() !== "svg") {
    throw new Error("Mermaid 未生成有效 SVG");
  }

  for (const element of [root, ...Array.from(root.querySelectorAll("*"))]) {
    if (DANGEROUS_SVG_TAGS.has(element.tagName.toLowerCase())) {
      element.remove();
      continue;
    }
    for (const attribute of Array.from(element.attributes ?? [])) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (name.startsWith("on") || name === "srcdoc") {
        element.removeAttribute(attribute.name);
      } else if ((name === "href" || name === "xlink:href" || name === "src") && !value.startsWith("#")) {
        element.removeAttribute(attribute.name);
      } else if (name === "style" && containsExternalCssUrl(value)) {
        element.removeAttribute(attribute.name);
      }
    }
    if (element.tagName.toLowerCase() === "style") {
      element.textContent = sanitizeMermaidCss(element.textContent ?? "");
    }
  }

  ensureMermaidViewBox(root);
  const dimensions = extractMermaidSvgMetrics(root.outerHTML);
  root.setAttribute("role", "img");
  root.setAttribute("width", "100%");
  root.removeAttribute("height");
  root.setAttribute("preserveAspectRatio", "xMidYMin meet");
  // Mermaid lays the diagram out at its intrinsic size and caps it with
  // `max-width` (its `useMaxWidth` contract). Overriding that cap with 100%
  // let a 132px-wide chart stretch across a 1300px column, magnifying 13px
  // labels roughly tenfold. Keep `width: 100%` so narrow containers still
  // shrink the diagram, but never grow it past what Mermaid measured.
  root.style.setProperty("max-width", `${dimensions.width}px`);
  root.style.setProperty("width", "100%");
  root.style.removeProperty("height");
  root.style.removeProperty("background");
  root.style.removeProperty("background-color");
  root.setAttribute("data-mnemora-mermaid", "shadow");
  root.removeAttribute("aria-roledescription");
  const serialized = new XMLSerializer().serializeToString(root);
  return { svg: serialized, metrics: withMermaidViewerBudget(serialized, dimensions) };
}

function ensureMermaidViewBox(root: Element) {
  const current = root.getAttribute("viewBox")?.trim();
  if (current && /^[-+\d.eE]+[\s,]+[-+\d.eE]+[\s,]+[-+\d.eE]+[\s,]+[-+\d.eE]+$/.test(current)) {
    return;
  }
  const width = parseSvgLength(root.getAttribute("width"));
  const height = parseSvgLength(root.getAttribute("height"));
  if (width > 0 && height > 0) root.setAttribute("viewBox", `0 0 ${width} ${height}`);
}

export function extractMermaidSvgMetrics(svg: string): MermaidSvgMetrics {
  const viewBox = svg.match(/\bviewBox\s*=\s*["']\s*([-+\d.eE]+)[\s,]+([-+\d.eE]+)[\s,]+([-+\d.eE]+)[\s,]+([-+\d.eE]+)\s*["']/i);
  let width = viewBox ? Number(viewBox[3]) : parseSvgDimension(svg, "width");
  let height = viewBox ? Number(viewBox[4]) : parseSvgDimension(svg, "height");
  if (!Number.isFinite(width) || width <= 0) width = 640;
  if (!Number.isFinite(height) || height <= 0) height = 360;
  return { width, height, aspectRatio: width / height };
}

export function measureMermaidViewerBudget(svg: string) {
  return withMermaidViewerBudget(svg, extractMermaidSvgMetrics(svg));
}

function withMermaidViewerBudget(svg: string, dimensions: Pick<MermaidSvgMetrics, "width" | "height" | "aspectRatio">): MermaidSvgMetrics {
  const elementCount = (svg.match(/<([a-z][\w:.-]*)(?:\s|\/?>)/gi) ?? []).length;
  const foreignObjectCount = (svg.match(/<foreignObject(?:\s|\/?>)/gi) ?? []).length;
  const svgChars = svg.length;
  const viewerSafe = svgChars <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerSvgChars
    && elementCount <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerElements
    && foreignObjectCount <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerForeignObjects
    && dimensions.width <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerIntrinsicDimension
    && dimensions.height <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerIntrinsicDimension;
  return { ...dimensions, svgChars, elementCount, foreignObjectCount, viewerSafe };
}

function parseSvgDimension(svg: string, attribute: "width" | "height") {
  const match = svg.match(new RegExp(`\\b${attribute}\\s*=\\s*["']\\s*([-+\\d.eE]+)`, "i"));
  return match ? Number(match[1]) : Number.NaN;
}

function parseSvgLength(value: string | null) {
  if (!value) return Number.NaN;
  const match = value.trim().match(/^([-+\d.eE]+)/);
  return match ? Number(match[1]) : Number.NaN;
}

function containsExternalCssUrl(value: string) {
  return /url\s*\(\s*["']?(?!#)[^)]*\)/i.test(value);
}

function sanitizeMermaidCss(value: string) {
  return value
    .replace(/@import[^;]+;?/gi, "")
    .replace(/url\s*\(\s*["']?(?!#)[^)]*\)/gi, "none");
}

export function mermaidThemeConfig(host: HTMLElement) {
  const shell = host.closest<HTMLElement>(".app-shell") ?? host;
  const styles = getComputedStyle(shell);
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const readColor = (name: string, fallback: string) => resolveMermaidColor(shell, name, fallback);
  const dark = shell.getAttribute("data-theme") === "dark";
  return {
    // 始终从 base 主题出发并显式注入明暗色，避免 Mermaid dark 主题中的
    // 黑色内嵌背景与应用表面色叠加后形成无法阅读的色块。
    theme: "base" as const,
    // 保持 Mermaid strict；htmlLabels 只负责渲染器生成的中文换行容器，
    // 下游清洗器仍会再次移除脚本、事件、外部 href 与 CSS 外链。
    securityLevel: "strict" as const,
    startOnLoad: false,
    htmlLabels: true,
    wrap: true,
    markdownAutoWrap: true,
    fontSize: 13,
    suppressErrorRendering: true,
    flowchart: {
      useMaxWidth: true,
      diagramPadding: 10,
      nodeSpacing: 34,
      rankSpacing: 46,
      wrappingWidth: 250,
      curve: "basis" as const,
    },
    sequence: {
      useMaxWidth: true,
      diagramMarginX: 16,
      diagramMarginY: 16,
      actorMargin: 42,
      messageMargin: 28,
      boxMargin: 8,
      boxTextMargin: 5,
      noteMargin: 8,
      actorFontSize: 13,
      noteFontSize: 12,
    },
    class: { useMaxWidth: true, diagramPadding: 10, nodeSpacing: 28, rankSpacing: 38 },
    state: { useMaxWidth: true, nodeSpacing: 28, rankSpacing: 38, fontSize: 13 },
    er: {
      useMaxWidth: true,
      diagramPadding: 18,
      layoutDirection: "LR" as const,
      minEntityWidth: 220,
      minEntityHeight: 72,
      entityPadding: 18,
      nodeSpacing: 54,
      rankSpacing: 72,
      fontSize: 13,
    },
    mindmap: { useMaxWidth: true, padding: 14, maxNodeWidth: 240 },
    themeVariables: {
      background: "transparent",
      fontSize: "13px",
      primaryColor: dark ? "#263f55" : "#dcecff",
      primaryTextColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      textColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      primaryBorderColor: dark ? "#75b5e8" : "#397eb8",
      nodeBkg: dark ? "#263f55" : "#dcecff",
      nodeBorder: dark ? "#75b5e8" : "#397eb8",
      labelBackground: readColor("--color-surface-raised", dark ? "#282d32" : "#ffffff"),
      lineColor: readColor("--color-muted", dark ? "#adb7bc" : "#687276"),
      secondaryColor: dark ? "#3f3150" : "#f1e3ff",
      secondaryBorderColor: dark ? "#c59be3" : "#8553a5",
      tertiaryColor: dark ? "#2f4939" : "#dff3e5",
      tertiaryBorderColor: dark ? "#85c69a" : "#4d8f62",
      clusterBkg: dark ? "#252a31" : "#f7f8fc",
      clusterBorder: readColor("--color-border", dark ? "#626a74" : "#bcc3cc"),
      noteBkgColor: dark ? "#55452b" : "#fff1c7",
      noteBorderColor: dark ? "#e2bd68" : "#a97916",
      noteTextColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      actorBkg: dark ? "#4d303d" : "#fde1eb",
      actorBorder: dark ? "#e18aaf" : "#a53e6b",
      actorTextColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      signalColor: readColor("--color-muted", dark ? "#adb7bc" : "#687276"),
      fontFamily: read("--reading-font-family", "system-ui, sans-serif"),
    },
  };
}

/**
 * Mermaid's color parser (khroma) accepts concrete RGB/HEX/HSL values, but
 * not CSS Color 4 expressions such as `color-mix(...)`. The application theme
 * intentionally uses those expressions for hover/active surfaces, so resolve
 * the custom property through the browser before handing it to Mermaid.
 */
function resolveMermaidColor(host: HTMLElement, property: string, fallback: string) {
  if (typeof document === "undefined" || typeof getComputedStyle === "undefined") return fallback;

  const probe = document.createElement("span");
  probe.setAttribute("aria-hidden", "true");
  probe.style.position = "absolute";
  probe.style.width = "0";
  probe.style.height = "0";
  probe.style.overflow = "hidden";
  // Referencing the custom property keeps nested `var(...)` values inherited
  // from the theme host instead of trying to parse them in JavaScript.
  probe.style.color = `var(${property}, ${fallback})`;
  host.appendChild(probe);
  const resolved = getComputedStyle(probe).color.trim();
  probe.remove();
  return isMermaidColor(resolved) ? resolved : fallback;
}

function isMermaidColor(value: string) {
  const normalized = value.trim().toLowerCase();
  return /^#[0-9a-f]{3,8}$/.test(normalized)
    || /^(?:rgb|rgba|hsl|hsla)\(/.test(normalized)
    || /^(?:transparent|black|white|red|green|blue|yellow|cyan|magenta|gray|grey)$/.test(normalized);
}
