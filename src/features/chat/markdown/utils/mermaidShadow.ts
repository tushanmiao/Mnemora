/**
 * 将已经通过安全清洗的 Mermaid SVG 放入 Shadow DOM。
 *
 * Cherry Studio 采用相同的核心策略：离屏测量、生成 SVG、清洗后进入
 * Shadow DOM。这样应用的 Markdown/主题 CSS 不会覆盖 Mermaid 内嵌的
 * classDef、节点文字颜色或 marker 样式。
 */
import { mermaidThemeConfig, normalizeMermaidSvgForXml } from "./mermaidSecurity";

export type MermaidShadowViewport = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export function renderMermaidSvgInShadowHost(svg: string, host: HTMLElement, viewport?: MermaidShadowViewport) {
  const root = parseSvg(svg);
  if (viewport) {
    root.setAttribute("viewBox", `${viewport.x} ${viewport.y} ${viewport.width} ${viewport.height}`);
    root.setAttribute("width", "100%");
    root.setAttribute("height", "100%");
    root.setAttribute("preserveAspectRatio", "xMidYMid meet");
  }

  const shadow = host.shadowRoot ?? host.attachShadow({ mode: "open" });
  const metrics = readMetrics(root, svg);
  host.style.setProperty("--mermaid-intrinsic-width", `${metrics.width}px`);
  host.style.setProperty("--mermaid-intrinsic-height", `${metrics.height}px`);
  host.style.setProperty("--mermaid-aspect-ratio", `${metrics.width} / ${metrics.height}`);
  shadow.replaceChildren();
  const style = document.createElement("style");
  style.textContent = `
    :host {
      display: block;
      width: 100%;
      min-width: 0;
      color: inherit;
      white-space: normal;
    }
    :host([data-mermaid-viewport="true"]) {
      width: 100%;
      height: 100%;
    }
    svg {
      display: block;
      width: 100%;
      max-width: 100%;
      height: auto;
      margin: 0 auto;
      overflow: visible;
    }
    :host([data-mermaid-viewport="true"]) svg {
      width: 100% !important;
      height: 100% !important;
      max-width: none !important;
    }
    foreignObject {
      overflow: visible !important;
    }
    foreignObject > div,
    .nodeLabel,
    .edgeLabel,
    .label {
      overflow: visible !important;
    }
    .nodeLabel p,
    .edgeLabel p,
    .label p {
      margin: 0;
    }
  `;
  const mountedRoot = document.importNode(root, true);
  shadow.append(style, mountedRoot);
  const repaired = repairUnreadableMermaidPaint(mountedRoot, host);
  if (repaired > 0) {
    host.setAttribute("data-mermaid-paint-repaired", String(repaired));
  } else {
    host.removeAttribute("data-mermaid-paint-repaired");
  }
}

/** Update navigation without parsing or cloning the complete SVG tree. */
export function updateMermaidSvgViewport(host: HTMLElement, viewport: MermaidShadowViewport) {
  const root = host.shadowRoot?.querySelector("svg");
  if (!root) return false;
  root.setAttribute("viewBox", `${viewport.x} ${viewport.y} ${viewport.width} ${viewport.height}`);
  root.setAttribute("width", "100%");
  root.setAttribute("height", "100%");
  root.setAttribute("preserveAspectRatio", "xMidYMid meet");
  return true;
}

function parseSvg(svg: string) {
  const normalized = normalizeMermaidSvgForXml(svg);
  const parsed = new DOMParser().parseFromString(normalized, "image/svg+xml");
  const parserError = parsed.querySelector("parsererror");
  const root = parsed.documentElement;
  if (!parserError && root.tagName.toLowerCase() === "svg") return root;

  const fallback = document.createElement("div");
  fallback.innerHTML = normalized;
  const fallbackRoot = fallback.querySelector("svg");
  if (!fallbackRoot) throw new Error("Mermaid SVG 解析失败。");
  fallbackRoot.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  return fallbackRoot;
}

function readMetrics(root: Element, sourceSvg: string) {
  const sourceViewBox = sourceSvg.match(/\bviewBox\s*=\s*["']\s*([-+\d.eE]+)[\s,]+([-+\d.eE]+)[\s,]+([-+\d.eE]+)[\s,]+([-+\d.eE]+)\s*["']/i);
  const viewBox = sourceViewBox
    ? sourceViewBox.slice(1).map(Number)
    : root.getAttribute("viewBox")?.trim().split(/[\s,]+/).map(Number);
  const width = viewBox?.length === 4 && Number.isFinite(viewBox[2]) && viewBox[2] > 0
    ? viewBox[2]
    : Number.parseFloat(root.getAttribute("width") ?? "640") || 640;
  const height = viewBox?.length === 4 && Number.isFinite(viewBox[3]) && viewBox[3] > 0
    ? viewBox[3]
    : Number.parseFloat(root.getAttribute("height") ?? "360") || 360;
  return { width, height };
}

/**
 * An SVG whose scoped Mermaid stylesheet failed to apply falls back to the SVG
 * defaults: black shapes and black text. Repair only groups whose computed
 * foreground/background contrast is actually unreadable, preserving valid
 * classDef/style colors and every normally rendered diagram.
 */
function repairUnreadableMermaidPaint(root: Element, host: HTMLElement) {
  if (typeof getComputedStyle === "undefined") return 0;
  const theme = mermaidThemeConfig(host).themeVariables;
  const nodePalette = { fill: theme.nodeBkg, stroke: theme.nodeBorder, text: theme.primaryTextColor };
  const clusterPalette = { fill: theme.clusterBkg, stroke: theme.clusterBorder, text: theme.textColor };
  let repaired = 0;

  for (const group of Array.from(root.querySelectorAll<SVGGElement>("g.node, g.cluster"))) {
    const shape = findGroupShape(group);
    const label = findGroupLabel(group);
    if (!shape || !label) continue;
    const shapeFill = getComputedStyle(shape).fill;
    const labelStyle = getComputedStyle(label);
    const textPaint = typeof SVGElement !== "undefined" && label instanceof SVGElement
      ? labelStyle.fill
      : labelStyle.color || labelStyle.fill;
    if (!hasUnreadableMermaidContrast(shapeFill, textPaint)) continue;

    const palette = group.classList.contains("cluster") ? clusterPalette : nodePalette;
    shape.style.setProperty("fill", palette.fill, "important");
    shape.style.setProperty("stroke", palette.stroke, "important");
    const labelContainer = label.closest<HTMLElement | SVGElement>(".nodeLabel, .label, .cluster-label") ?? label;
    labelContainer.style.setProperty("color", palette.text, "important");
    for (const element of Array.from(labelContainer.querySelectorAll<HTMLElement>("div, p, span"))) {
      element.style.setProperty("color", palette.text, "important");
    }
    for (const element of Array.from(labelContainer.querySelectorAll<SVGElement>("text, tspan"))) {
      element.style.setProperty("fill", palette.text, "important");
    }
    group.setAttribute("data-mnemora-contrast-repair", "true");
    repaired += 1;
  }
  return repaired;
}

function findGroupShape(group: SVGGElement) {
  return Array.from(group.querySelectorAll<SVGElement>("rect, polygon, circle, ellipse, path")).find((shape) => (
    !shape.closest(".label, .nodeLabel, .cluster-label, marker, defs, clipPath")
    && getComputedStyle(shape).fill !== "none"
  ));
}

function findGroupLabel(group: SVGGElement) {
  const selector = group.classList.contains("cluster")
    ? ".cluster-label p, .cluster-label span, .cluster-label text, .cluster-label"
    : ".nodeLabel p, .nodeLabel span, .nodeLabel, .label p, .label span, .label, text, tspan";
  return group.querySelector<HTMLElement | SVGElement>(selector);
}

export function hasUnreadableMermaidContrast(background: string, foreground: string) {
  const backgroundRgb = parseCssRgb(background);
  const foregroundRgb = parseCssRgb(foreground);
  if (!backgroundRgb || !foregroundRgb) return false;
  return contrastRatio(backgroundRgb, foregroundRgb) < 2.5;
}

function parseCssRgb(value: string): [number, number, number] | null {
  const hex = value.trim().match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})(?:([0-9a-f]{2}))?$/i);
  if (hex) {
    if (hex[4] && Number.parseInt(hex[4], 16) <= 12) return null;
    return hex.slice(1, 4).map((channel) => Number.parseInt(channel, 16)) as [number, number, number];
  }
  const rgb = value.trim().match(/^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+))?/i);
  if (!rgb) return null;
  if (rgb[4] !== undefined && Number(rgb[4]) <= 0.05) return null;
  return rgb.slice(1, 4).map((channel) => Math.min(255, Math.max(0, Number(channel)))) as [number, number, number];
}

function contrastRatio(first: [number, number, number], second: [number, number, number]) {
  const firstLuminance = relativeLuminance(first);
  const secondLuminance = relativeLuminance(second);
  const lighter = Math.max(firstLuminance, secondLuminance);
  const darker = Math.min(firstLuminance, secondLuminance);
  return (lighter + 0.05) / (darker + 0.05);
}

function relativeLuminance(color: [number, number, number]) {
  const [red, green, blue] = color.map((channel) => {
    const normalized = channel / 255;
    return normalized <= 0.04045 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
  });
  return red * 0.2126 + green * 0.7152 + blue * 0.0722;
}
