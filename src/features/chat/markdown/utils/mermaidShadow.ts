/**
 * 将已经通过安全清洗的 Mermaid SVG 放入 Shadow DOM。
 *
 * Cherry Studio 采用相同的核心策略：离屏测量、生成 SVG、清洗后进入
 * Shadow DOM。这样应用的 Markdown/主题 CSS 不会覆盖 Mermaid 内嵌的
 * classDef、节点文字颜色或 marker 样式。
 */
import { normalizeMermaidSvgForXml } from "./mermaidSecurity";

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
      width: 100%;
      height: 100%;
      max-width: none;
    }
    foreignObject,
    foreignObject > div,
    .nodeLabel,
    .edgeLabel,
    .label,
    p {
      overflow: visible !important;
      overflow-wrap: anywhere;
      word-break: break-word;
    }
  `;
  shadow.append(style, document.importNode(root, true));
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
