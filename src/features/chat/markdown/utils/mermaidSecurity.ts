const DANGEROUS_SVG_TAGS = new Set(["script", "foreignobject", "iframe", "object", "embed", "image"]);
const MERMAID_NODE_SHAPES = "rect, polygon, circle, ellipse, path";
const MERMAID_PALETTE_SIZE = 6;

// Kept as a compatibility export for callers that previously imported the
// large-diagram predicate from the security module. Layout policy now lives in
// mermaidLayout so rendering and viewer sizing share one contract.
export { isLargeMermaidDiagram } from "./mermaidLayout";

export type MermaidSvgMetrics = {
  width: number;
  height: number;
  aspectRatio: number;
};

export type SanitizedMermaidSvg = {
  svg: string;
  metrics: MermaidSvgMetrics;
};

/** Mermaid 输出由受信任的渲染器生成，但仍移除脚本、外链和事件属性。 */
export function sanitizeMermaidSvg(svg: string): SanitizedMermaidSvg {
  if (typeof DOMParser === "undefined" || typeof document === "undefined") {
    return { svg, metrics: extractMermaidSvgMetrics(svg) };
  }
  const parsed = new DOMParser().parseFromString(svg, "image/svg+xml");
  const root = parsed.documentElement;
  if (!root || root.tagName.toLowerCase() !== "svg") throw new Error("Mermaid 未生成有效 SVG");

  for (const element of Array.from(root.querySelectorAll("*"))) {
    if (DANGEROUS_SVG_TAGS.has(element.tagName.toLowerCase())) {
      element.remove();
      continue;
    }
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (name.startsWith("on") || name === "srcdoc") {
        element.removeAttribute(attribute.name);
      } else if ((name === "href" || name === "xlink:href") && !value.startsWith("#")) {
        element.removeAttribute(attribute.name);
      }
    }
  }

  markDefaultFlowchartNodes(root);
  markFlowchartClusters(root);
  const metrics = extractMermaidSvgMetrics(root.outerHTML);
  root.setAttribute("role", "img");
  root.setAttribute("width", String(metrics.width));
  root.setAttribute("height", String(metrics.height));
  root.setAttribute("preserveAspectRatio", "xMidYMid meet");
  root.style.removeProperty("max-width");
  root.style.removeProperty("width");
  root.style.removeProperty("height");
  root.style.removeProperty("background");
  root.style.removeProperty("background-color");
  root.setAttribute("data-mnemora-mermaid", "true");
  root.removeAttribute("aria-roledescription");
  return { svg: new XMLSerializer().serializeToString(root), metrics };
}

export function extractMermaidSvgMetrics(svg: string): MermaidSvgMetrics {
  const viewBox = svg.match(/\bviewBox\s*=\s*["']\s*([-+\d.eE]+)[\s,]+([-+\d.eE]+)[\s,]+([-+\d.eE]+)[\s,]+([-+\d.eE]+)\s*["']/i);
  let width = viewBox ? Number(viewBox[3]) : parseSvgDimension(svg, "width");
  let height = viewBox ? Number(viewBox[4]) : parseSvgDimension(svg, "height");
  if (!Number.isFinite(width) || width <= 0) width = 640;
  if (!Number.isFinite(height) || height <= 0) height = 360;
  return { width, height, aspectRatio: width / height };
}

function parseSvgDimension(svg: string, attribute: "width" | "height") {
  const match = svg.match(new RegExp(`\\b${attribute}\\s*=\\s*["']\\s*([-+\\d.eE]+)`, "i"));
  return match ? Number(match[1]) : Number.NaN;
}

/**
 * Mermaid 11 把默认主题色主要放在 SVG 内嵌样式表中，而 classDef/style
 * 通常会写成 shape 的 inline style。仅依赖 `[fill="#..."]` 无法稳定命中
 * 默认节点；使用全局 `!important` 又会抹掉用户定义的语义颜色。
 *
 * 因此这里只给“没有额外 class、没有 inline fill/stroke”的默认流程图节点
 * 标记色板序号。实际颜色仍由应用 CSS 根据明暗主题决定。用户 classDef、
 * `style A ...`、透明辅助路径和标签背景都不会被标记。
 */
function markDefaultFlowchartNodes(root: Element) {
  let paletteIndex = 0;
  for (const group of Array.from(root.querySelectorAll("g.node"))) {
    const classes = Array.from(group.classList);
    if (!classes.includes("default") || classes.some((name) => name !== "node" && name !== "default")) {
      continue;
    }

    const shapes = Array.from(group.querySelectorAll(MERMAID_NODE_SHAPES)).filter((shape) => {
      if (shape.closest(".label, marker, defs, clipPath")) return false;
      return shape.getAttribute("fill")?.trim().toLowerCase() !== "none";
    });
    if (shapes.length === 0 || shapes.some(hasAuthoredNodePaint)) continue;

    for (const shape of shapes) {
      shape.setAttribute("data-mnemora-node-tone", String(paletteIndex));
    }
    paletteIndex = (paletteIndex + 1) % MERMAID_PALETTE_SIZE;
  }
}

function hasAuthoredNodePaint(element: Element) {
  const style = element.getAttribute("style")?.toLowerCase() ?? "";
  return /(?:^|;)\s*(?:fill|stroke)\s*:/.test(style);
}

function hasClusterPaint(element: Element) {
  return hasAuthoredNodePaint(element) || element.hasAttribute("fill") || element.hasAttribute("stroke");
}

function markFlowchartClusters(root: Element) {
  for (const group of Array.from(root.querySelectorAll("g.cluster"))) {
    const shape = Array.from(group.children).find((element) => element.tagName.toLowerCase() === "rect");
    if (!shape) continue;

    if (!hasClusterPaint(shape)) {
      group.setAttribute("data-mnemora-cluster-default", "true");
      continue;
    }

    const fill = readInlinePaint(shape, "fill");
    const tone = fill ? getColorTone(fill) : null;
    if (tone) group.setAttribute("data-mnemora-cluster-tone", tone);
  }
}

function readInlinePaint(element: Element, property: "fill" | "stroke") {
  const attribute = element.getAttribute(property)?.trim();
  if (attribute) return attribute;
  const style = element.getAttribute("style") ?? "";
  return style.match(new RegExp(`(?:^|;)\\s*${property}\\s*:\\s*([^;!]+)`, "i"))?.[1]?.trim() ?? "";
}

function getColorTone(color: string): "light" | "dark" | null {
  const rgb = parseRgbColor(color);
  if (!rgb) return null;
  const luminance = rgb
    .map((channel) => {
      const value = channel / 255;
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    })
    .reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
  return luminance > 0.42 ? "light" : "dark";
}

function parseRgbColor(color: string): [number, number, number] | null {
  const normalized = color.trim().toLowerCase();
  const namedColors: Record<string, [number, number, number]> = {
    black: [0, 0, 0],
    white: [255, 255, 255],
    gray: [128, 128, 128],
    grey: [128, 128, 128],
  };
  if (namedColors[normalized]) return namedColors[normalized];
  const shortHex = normalized.match(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i);
  if (shortHex) return shortHex.slice(1).map((value) => Number.parseInt(value + value, 16)) as [number, number, number];
  const hex = normalized.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})(?:[0-9a-f]{2})?$/i);
  if (hex) return hex.slice(1, 4).map((value) => Number.parseInt(value, 16)) as [number, number, number];
  const rgb = normalized.match(/^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/i);
  if (!rgb) return null;
  return rgb.slice(1, 4).map((value) => Math.min(255, Math.max(0, Number(value)))) as [number, number, number];
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
    securityLevel: "strict" as const,
    startOnLoad: false,
    htmlLabels: false,
    wrap: true,
    markdownAutoWrap: true,
    fontSize: 13,
    suppressErrorRendering: true,
    flowchart: {
      useMaxWidth: true,
      diagramPadding: 10,
      nodeSpacing: 28,
      rankSpacing: 38,
      wrappingWidth: 190,
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
    er: { useMaxWidth: true, diagramPadding: 10 },
    mindmap: { useMaxWidth: true, padding: 10, maxNodeWidth: 190 },
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
