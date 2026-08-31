import { repairMermaidSource } from "./mermaidRepair";
import { MARKDOWN_RENDER_LIMITS } from "./renderLimits";

const DANGEROUS_SVG_TAGS = new Set(["script", "iframe", "object", "embed", "image"]);
const XML_VOID_TAGS = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]);
const MERMAID_DIRECTIVE = /%%\{[\s\S]*?\}%%/g;
const MERMAID_INIT_DIRECTIVE = /^%%\{\s*(?:init|initialize)\s*:\s*(\{[\s\S]*\})\s*\}%%$/i;
const MERMAID_SECURITY_OVERRIDE = /["']?securityLevel["']?\s*:/i;
const MERMAID_CLICK_LINE = /^\s*click\s+.*$/gim;
const HEX_COLOR = /^#(?:[a-f0-9]{3}|(?:[a-f0-9]{2}){2,4})$/i;

export type MermaidSvgMetrics = {
  x?: number;
  y?: number;
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

export type MermaidSvgPaint = {
  canvas: string;
  subtleCanvas: string;
  alternateCanvas: string;
  foreground: string;
  mutedForeground: string;
  border: string;
  line: string;
  fontFamily: string;
  fontSize: string;
  dark: boolean;
};

export type PreparedMermaidSource = {
  /** 清洗并修复之后、可直接交给解析器的源码。 */
  source: string;
  /** 实际生效的修复规则名。渲染失败时用来告诉用户「修过了仍然失败」。 */
  repairs: string[];
};

/**
 * Keep only the single harmless init option that Codex preserves. Every other
 * directive and all click handlers are removed before Mermaid sees the input;
 * an attempted securityLevel override rejects the diagram entirely.
 */
export function prepareMermaidSourceDetailed(source: string): PreparedMermaidSource {
  let securityOverride = false;
  const prepared = source.replace(MERMAID_DIRECTIVE, (directive) => {
    if (MERMAID_SECURITY_OVERRIDE.test(directive)) securityOverride = true;
    const match = directive.match(MERMAID_INIT_DIRECTIVE);
    if (!match) return "";

    try {
      const parsed = JSON.parse((match[1] ?? "").replace(/'/g, '"')) as {
        themeVariables?: { sequenceNumberColor?: unknown };
      };
      const sequenceNumberColor = parsed.themeVariables?.sequenceNumberColor;
      if (typeof sequenceNumberColor === "string" && HEX_COLOR.test(sequenceNumberColor)) {
        return `%%{init: ${JSON.stringify({
          theme: "base",
          themeVariables: { sequenceNumberColor },
        })}}%%`;
      }
    } catch {
      // Invalid and unsupported init payloads are intentionally discarded.
    }
    return "";
  });

  if (securityOverride) {
    throw new Error("Mermaid 图表尝试覆盖安全级别，已阻止渲染。");
  }
  const sanitized = prepared.replace(MERMAID_CLICK_LINE, "").replace(/\\n/g, "<br/>").trim();
  // 清洗之后、交给解析器之前做一次无损语法修复。放在这里而不是更早，是因为
  // 修复规则要看的是最终形态：\n 已经变成 <br/>，click 行已经删掉。
  return repairMermaidSource(sanitized);
}

/** 只关心最终源码时的薄封装。 */
export function prepareMermaidSource(source: string) {
  return prepareMermaidSourceDetailed(source).source;
}

/**
 * Mermaid's htmlLabels renderer emits HTML void elements (most notably
 * `<br>`) inside SVG foreignObject nodes. That is valid HTML, but the SVG is
 * subsequently parsed as XML by the sanitizer and Shadow DOM renderer. Make
 * the contract explicit at that boundary for legacy diagrams and diagram types
 * that may still contain foreignObject even when flowchart htmlLabels are off.
 */
export function normalizeMermaidSvgForXml(svg: string) {
  return svg.replace(/<([a-z][\w:.-]*)(\s[^<>]*?)?\s*\/?\s*>/gi, (tag, name: string, attributes = "") => {
    if (!XML_VOID_TAGS.has(name.toLowerCase()) || /\/\s*>$/.test(tag)) return tag;
    return `<${name}${attributes.trimEnd()} />`;
  });
}

/** Mermaid 输出由受信任的渲染器生成，但仍移除脚本、外链和事件属性。 */
export function sanitizeMermaidSvg(svg: string, paint?: MermaidSvgPaint): SanitizedMermaidSvg {
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
  stabilizeMermaidSvgPaint(root);
  if (paint) materializeMermaidFallbackPaint(root, paint);
  const dimensions = extractMermaidSvgMetrics(root.outerHTML);
  root.setAttribute("role", "img");
  root.setAttribute("preserveAspectRatio", "xMidYMin meet");
  root.style.removeProperty("background");
  root.style.removeProperty("background-color");
  root.setAttribute("data-mnemora-mermaid", "sanitized");
  root.removeAttribute("aria-roledescription");
  const serialized = new XMLSerializer().serializeToString(root);
  return { svg: serialized, metrics: withMermaidViewerBudget(serialized, dimensions) };
}

/**
 * Keep only the critical stroke-only edge contract in the sanitized SVG. Line
 * width and color stay under Mermaid's ownership so normal, thick, dotted and
 * user-styled edges keep their intended visual hierarchy.
 */
export function stabilizeMermaidSvgPaint(root: Element) {
  let stabilized = 0;
  const edges = root.querySelectorAll<SVGElement>("path.flowchart-link, g.edgePath > path.path");
  for (const edge of Array.from(edges)) {
    if (typeof edge.setAttribute !== "function") continue;
    edge.setAttribute("fill", "none");
    edge.style?.setProperty("fill", "none", "important");
    edge.style?.setProperty("stroke-linecap", "round");
    edge.style?.setProperty("stroke-linejoin", "round");
    stabilized += 1;
  }
  repairNeoFlowchartMarkerGaps(root);
  if (stabilized > 0) root.setAttribute("data-mnemora-edge-contract", "stable");
  return stabilized;
}

/**
 * Mermaid scopes its generated theme rules inside the SVG. Some WebView2
 * production paths can lose that embedded stylesheet while the same SVG is
 * moved through DOMParser/template mounting. SVG then falls back to black
 * fills and start-aligned text. Keep Mermaid's geometry and parser untouched,
 * but duplicate the critical monochrome paint as presentation attributes so
 * the diagram remains readable even when its style element is unavailable.
 */
export function materializeMermaidFallbackPaint(root: Element, paint: MermaidSvgPaint) {
  root.setAttribute("font-family", paint.fontFamily);
  root.setAttribute("font-size", paint.fontSize);
  root.setAttribute("color", paint.foreground);

  setSvgAttributes(root.querySelectorAll("text, tspan"), {
    fill: paint.foreground,
    "font-family": paint.fontFamily,
    "font-size": paint.fontSize,
  });

  setSvgAttributes(root.querySelectorAll([
    "g.node > rect",
    "g.node > circle",
    "g.node > ellipse",
    "g.node > polygon",
    "g.node > path",
    "g.cluster > rect",
    "g.cluster > polygon",
    "g.cluster > path",
    "rect.actor",
    "rect.note",
    "rect.labelBox",
    ".entityBox",
    ".attributeBoxOdd",
    ".attributeBoxEven",
    ".classGroup rect",
    ".statediagram-state rect",
    ".stateGroup rect",
  ].join(", ")), {
    fill: paint.canvas,
    stroke: paint.border,
    "stroke-width": "1",
  });

  setSvgAttributes(root.querySelectorAll([
    ".activation0",
    ".activation1",
    ".activation2",
    ".section0",
    ".section2",
    ".task",
  ].join(", ")), {
    fill: paint.subtleCanvas,
    stroke: paint.border,
  });

  setSvgAttributes(root.querySelectorAll([
    "path.flowchart-link",
    "g.edgePath > path.path",
    ".messageLine0",
    ".messageLine1",
    ".loopLine",
    ".transition",
    ".relation",
  ].join(", ")), {
    fill: "none",
    stroke: paint.line,
  });

  setSvgAttributes(root.querySelectorAll("marker path, marker polygon"), {
    fill: paint.line,
    stroke: paint.line,
  });

  setSvgAttributes(root.querySelectorAll([
    ".node text",
    ".cluster-label text",
    ".edgeLabel text",
  ].join(", ")), {
    fill: paint.foreground,
    "text-anchor": "middle",
  });

  setSvgAttributes(root.querySelectorAll(".labelBkg, .edgeLabel rect, .edgeLabel polygon"), {
    fill: paint.canvas,
    stroke: "none",
  });

  root.setAttribute("data-mnemora-paint-fallback", "materialized");
}

function setSvgAttributes(elements: NodeListOf<Element> | Element[], attributes: Record<string, string>) {
  for (const element of Array.from(elements)) {
    for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, value);
  }
}

/** Port the narrow neo-marker correction used by Codex Desktop. */
export function repairNeoFlowchartMarkerGaps(root: Element) {
  if (!root.classList?.contains("flowchart")) return 0;
  const markers = new Map(Array.from(root.querySelectorAll("marker"), (marker) => [marker.id, marker]));
  const shifted = new Set<Element>();
  let repaired = 0;

  for (const edge of Array.from(root.querySelectorAll<SVGPathElement>('path[data-edge][data-look="neo"].edge-pattern-solid'))) {
    const dash = edge.style.strokeDasharray.trim().split(/[\s,]+/).map(Number);
    if (dash.length !== 4 || dash.some((value) => !Number.isFinite(value))) continue;

    const endpoints = (["start", "end"] as const).map((endpoint) => {
      const reference = edge.getAttribute(`marker-${endpoint}`);
      const markerId = reference?.match(/#([^)'"\s]+)['"]?\)$/)?.[1];
      const marker = markerId ? markers.get(markerId) : undefined;
      const suffix = endpoint === "start" ? "Start" : "End";
      const expectedRefX = endpoint === "start" ? 1 : 11.5;
      if (!marker
        || !new RegExp(`-point${suffix}-margin(?:_.+)?$`).test(marker.id)
        || (!shifted.has(marker) && Number(marker.getAttribute("refX")) !== expectedRefX)) {
        return { gap: 0, marker: undefined, offset: 0 };
      }
      return { gap: 4, marker, offset: endpoint === "start" ? -4 : 4 };
    });

    const markerGap = endpoints[0].gap + endpoints[1].gap;
    if (dash[2] < markerGap) continue;
    for (const endpoint of endpoints) {
      if (!endpoint.marker || shifted.has(endpoint.marker)) continue;
      endpoint.marker.setAttribute("refX", String(Number(endpoint.marker.getAttribute("refX")) + endpoint.offset));
      shifted.add(endpoint.marker);
    }
    edge.style.strokeDasharray = `0 ${dash[1] + endpoints[0].gap} ${dash[2] - markerGap} ${dash[3] + endpoints[1].gap}`;
    repaired += 1;
  }
  return repaired;
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
  let x = viewBox ? Number(viewBox[1]) : 0;
  let y = viewBox ? Number(viewBox[2]) : 0;
  let width = viewBox ? Number(viewBox[3]) : parseSvgDimension(svg, "width");
  let height = viewBox ? Number(viewBox[4]) : parseSvgDimension(svg, "height");
  if (!Number.isFinite(x)) x = 0;
  if (!Number.isFinite(y)) y = 0;
  if (!Number.isFinite(width) || width <= 0) width = 640;
  if (!Number.isFinite(height) || height <= 0) height = 360;
  return { x, y, width, height, aspectRatio: width / height };
}

export function measureMermaidViewerBudget(svg: string) {
  return withMermaidViewerBudget(svg, extractMermaidSvgMetrics(svg));
}

function withMermaidViewerBudget(svg: string, dimensions: Pick<MermaidSvgMetrics, "x" | "y" | "width" | "height" | "aspectRatio">): MermaidSvgMetrics {
  const elementCount = (svg.match(/<([a-z][\w:.-]*)(?:\s|\/?>)/gi) ?? []).length;
  const foreignObjectCount = (svg.match(/<foreignObject(?:\s|\/?>)/gi) ?? []).length;
  const svgChars = svg.length;
  const viewerSafe = svgChars <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerSvgChars
    && elementCount <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerElements
    && foreignObjectCount <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerForeignObjects
    && dimensions.width <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerIntrinsicDimension
    && dimensions.height <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerIntrinsicDimension
    && Math.max(dimensions.aspectRatio, 1 / dimensions.aspectRatio) <= MARKDOWN_RENDER_LIMITS.maxMermaidViewerAspectRatio;
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

export function mermaidSvgPaint(host: HTMLElement): MermaidSvgPaint {
  const shell = host.closest<HTMLElement>(".app-shell") ?? host;
  const styles = getComputedStyle(shell);
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const readColor = (name: string, fallback: string) => resolveMermaidColor(shell, name, fallback);
  const dark = shell.getAttribute("data-theme") === "dark";
  const fontFamily = read("--reading-font-family", '"Segoe UI Variable", "Microsoft YaHei UI", system-ui, sans-serif');
  // Codex-style diagrams deliberately use one neutral ink-and-paper system.
  // Diagram type still controls geometry, but it no longer invents a separate
  // blue/purple/green/yellow palette for nodes, actors, notes or ER rows.
  const canvas = readColor("--color-surface-raised", dark ? "#202428" : "#ffffff");
  const subtleCanvas = dark ? "#282d32" : "#f6f7f8";
  const alternateCanvas = dark ? "#30363c" : "#eceff1";
  const foreground = readColor("--color-text", dark ? "#edf0f2" : "#202427");
  const mutedForeground = readColor("--color-muted", dark ? "#adb7bc" : "#687276");
  const border = readColor("--color-border", dark ? "#626a74" : "#d4d8dc");
  const line = dark ? "#969ea5" : "#62686e";
  return {
    canvas,
    subtleCanvas,
    alternateCanvas,
    foreground,
    mutedForeground,
    border,
    line,
    fontFamily,
    fontSize: "13px",
    dark,
  };
}

export function mermaidThemeConfig(host: HTMLElement, _source = "", paint = mermaidSvgPaint(host)) {
  const {
    canvas,
    subtleCanvas,
    alternateCanvas,
    foreground,
    mutedForeground,
    border,
    line,
    fontFamily,
    dark,
  } = paint;
  const grayScale = dark
    ? ["#596168", "#646c73", "#70787f", "#7c848b", "#899198", "#969da4"]
    : ["#e8eaec", "#dde0e3", "#d2d6d9", "#c7ccd0", "#bcc2c7", "#b1b8bd"];
  return {
    // Use Mermaid's own print-friendly neutral theme, then map its official
    // theme variables onto the active Mnemora light/dark surfaces.
    theme: "neutral" as const,
    // 保持 Mermaid strict；下游清洗器仍会再次移除脚本、事件、外部 href
    // 与 CSS 外链。
    securityLevel: "strict" as const,
    startOnLoad: false,
    suppressErrorRendering: true,
    deterministicIds: false,
    deterministicIDSeed: "mnemora-mermaid",
    // Codex Desktop takes the same conservative route. Pure SVG text avoids
    // foreignObject font/CSS inheritance drift and is materially less likely
    // to clip after a note panel resize or a late font swap.
    htmlLabels: false,
    wrap: true,
    markdownAutoWrap: true,
    fontSize: 13,
    darkMode: dark,
    fontFamily,
    // Codex keeps Mermaid's stable classic geometry as the default. The neo
    // marker-gap repair below is only a compatibility guard for authored neo
    // SVGs; globally forcing neo enables SVG drop-shadow filters which WebView2
    // can composite as opaque black rectangles.
    look: "classic" as const,
    flowchart: {
      htmlLabels: false,
      useMaxWidth: true,
      diagramPadding: 10,
      nodeSpacing: 34,
      rankSpacing: 46,
      wrappingWidth: 250,
      curve: "linear" as const,
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
      // Keep the generated SVG self-contained and WebView2-safe. Mermaid's
      // base theme otherwise enables gradient strokes and CSS drop-shadow
      // filters whenever neo geometry is selected.
      useGradient: false,
      dropShadow: "none",
      fontSize: "13px",
      primaryColor: canvas,
      secondaryColor: canvas,
      tertiaryColor: canvas,
      primaryTextColor: foreground,
      secondaryTextColor: foreground,
      tertiaryTextColor: foreground,
      textColor: foreground,
      primaryBorderColor: border,
      secondaryBorderColor: border,
      tertiaryBorderColor: border,
      nodeBkg: canvas,
      mainBkg: canvas,
      secondBkg: canvas,
      nodeBorder: border,
      labelBackground: canvas,
      edgeLabelBackground: canvas,
      lineColor: line,
      arrowheadColor: line,
      defaultLinkColor: line,
      clusterBkg: canvas,
      clusterBorder: border,
      noteBkgColor: canvas,
      noteBorderColor: border,
      noteTextColor: foreground,
      actorBkg: canvas,
      actorBorder: border,
      actorTextColor: foreground,
      actorLineColor: border,
      signalColor: line,
      signalTextColor: foreground,
      labelBoxBkgColor: canvas,
      labelBoxBorderColor: border,
      labelTextColor: foreground,
      loopTextColor: foreground,
      activationBkgColor: subtleCanvas,
      activationBorderColor: border,
      stateBkg: canvas,
      stateBorder: border,
      stateLabelColor: foreground,
      transitionColor: line,
      transitionLabelColor: foreground,
      labelBackgroundColor: canvas,
      compositeBackground: canvas,
      compositeTitleBackground: subtleCanvas,
      altBackground: subtleCanvas,
      classText: foreground,
      rowOdd: canvas,
      rowEven: canvas,
      attributeBackgroundColorOdd: canvas,
      attributeBackgroundColorEven: canvas,
      sectionBkgColor: subtleCanvas,
      altSectionBkgColor: canvas,
      sectionBkgColor2: alternateCanvas,
      taskBkgColor: alternateCanvas,
      taskBorderColor: border,
      activeTaskBkgColor: subtleCanvas,
      activeTaskBorderColor: line,
      doneTaskBkgColor: subtleCanvas,
      doneTaskBorderColor: border,
      taskTextColor: foreground,
      taskTextDarkColor: foreground,
      taskTextLightColor: foreground,
      taskTextOutsideColor: mutedForeground,
      gridColor: border,
      todayLineColor: line,
      requirementBackground: canvas,
      requirementBorderColor: border,
      requirementTextColor: foreground,
      relationColor: line,
      relationLabelBackground: canvas,
      relationLabelColor: foreground,
      quadrant1Fill: subtleCanvas,
      quadrant2Fill: alternateCanvas,
      quadrant3Fill: subtleCanvas,
      quadrant4Fill: alternateCanvas,
      quadrant1TextFill: foreground,
      quadrant2TextFill: foreground,
      quadrant3TextFill: foreground,
      quadrant4TextFill: foreground,
      cScale0: grayScale[0],
      cScale1: grayScale[1],
      cScale2: grayScale[2],
      cScale3: grayScale[3],
      cScale4: grayScale[4],
      cScale5: grayScale[5],
      cScale6: grayScale[0],
      cScale7: grayScale[1],
      cScale8: grayScale[2],
      cScale9: grayScale[3],
      cScale10: grayScale[4],
      cScale11: grayScale[5],
      fillType0: grayScale[0],
      fillType1: grayScale[1],
      fillType2: grayScale[2],
      fillType3: grayScale[3],
      fillType4: grayScale[4],
      fillType5: grayScale[5],
      fillType6: grayScale[1],
      fillType7: grayScale[3],
      pie1: grayScale[0],
      pie2: grayScale[1],
      pie3: grayScale[2],
      pie4: grayScale[3],
      pie5: grayScale[4],
      pie6: grayScale[5],
      pie7: grayScale[0],
      pie8: grayScale[1],
      pie9: grayScale[2],
      pie10: grayScale[3],
      pie11: grayScale[4],
      pie12: grayScale[5],
      pieTitleTextColor: foreground,
      pieSectionTextColor: foreground,
      pieLegendTextColor: foreground,
      pieStrokeColor: border,
      pieOuterStrokeColor: border,
      git0: grayScale[0],
      git1: grayScale[1],
      git2: grayScale[2],
      git3: grayScale[3],
      git4: grayScale[4],
      git5: grayScale[5],
      git6: grayScale[1],
      git7: grayScale[3],
      branchLabelColor: foreground,
      tagLabelColor: foreground,
      tagLabelBackground: canvas,
      tagLabelBorder: border,
      commitLabelColor: foreground,
      commitLabelBackground: canvas,
      fontFamily,
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
