const SVG_NAMESPACE = "http://www.w3.org/2000/svg";
const OVERFLOW_TOLERANCE_PX = 1;

/**
 * 允许把图表压缩到的最小比例。
 *
 * Mermaid 的 `useMaxWidth` 会把整张图等比塞进容器宽度，字号跟着一起缩。一张
 * 2400px 宽的状态机落到 868px 的正文栏里意味着缩到 0.36 —— 13px 的字实际渲染
 * 成 4.7px，笔画糊成一团，还会被子像素抗锯齿染上彩色伪影。那正是「同样的
 * mermaid 在 VS Code 里好看、在这里很丑」的全部原因：VS Code 的图在宽面板里
 * 不需要这么压。
 *
 * 0.85 是「13px 字仍有 11px」的下限。低于这个比例就不再压缩，改为保留尺寸并
 * 让容器横向滚动 —— 宁可让用户横向拖，也不要给一张读不了的图。
 */
const MIN_LEGIBLE_SCALE = 0.85;

export type MermaidIntrinsicSize = {
  x: number;
  y: number;
  width: number;
  height: number;
};

/**
 * Mount the already-sanitized Mermaid output as a real SVG element. Keeping
 * the original viewBox and avoiding a Shadow DOM/viewBox rewrite makes the
 * browser and Mermaid share one geometry model.
 */
export function mountMermaidSvg(host: HTMLElement, source: string) {
  const template = host.ownerDocument.createElement("template");
  template.innerHTML = source;
  const svg = template.content.querySelector<SVGSVGElement>("svg");
  if (!svg) throw new Error("未找到可显示的 Mermaid SVG。");

  const size = readMermaidIntrinsicSize(svg);
  svg.style.height = "auto";
  svg.style.maxHeight = "none";

  if (!size) {
    svg.style.width = "auto";
    svg.style.maxWidth = "100%";
  } else {
    const contentWidth = readContentWidth(host);
    const fitScale = contentWidth > 0 ? contentWidth / size.width : 1;
    if (fitScale >= 1) {
      // 图比栏窄：按原尺寸显示，不放大（放大只会让线条变粗、文字发虚）。
      svg.style.width = `${size.width}px`;
      svg.style.maxWidth = "100%";
    } else if (fitScale >= MIN_LEGIBLE_SCALE) {
      // 压一点还能读：交给 maxWidth 贴合栏宽。
      svg.style.width = `${size.width}px`;
      svg.style.maxWidth = "100%";
    } else {
      // 压下去就读不了了：钉在可读下限，超出的部分由 overflow-x 滚动承载。
      svg.style.width = `${Math.round(size.width * MIN_LEGIBLE_SCALE)}px`;
      svg.style.maxWidth = "none";
    }
  }

  svg.setAttribute("aria-hidden", "true");
  host.replaceChildren(svg);
  return svg;
}

/** Detect whether Mermaid's natural width is being reduced by the preview. */
export function syncMermaidOverflow(block: HTMLElement, host: HTMLElement, svg: SVGSVGElement) {
  const naturalWidth = readMermaidIntrinsicSize(svg)?.width ?? 0;
  const contentWidth = readContentWidth(host);
  const overflowed = naturalWidth > contentWidth + OVERFLOW_TOLERANCE_PX;
  block.toggleAttribute("data-mermaid-overflow", overflowed);
  return overflowed;
}

/**
 * Clone the current SVG for the generic image viewer. The clone receives an
 * explicit background and intrinsic dimensions so it remains self-contained
 * after CSS variables from the note/chat surface are no longer inherited.
 */
export function createMermaidPreviewSource(svg: SVGSVGElement, host: HTMLElement) {
  const clone = svg.cloneNode(true) as SVGSVGElement;
  const size = readMermaidIntrinsicSize(svg);
  const styles = host.ownerDocument.defaultView?.getComputedStyle(host);
  const background = styles?.getPropertyValue("--color-surface-raised").trim()
    || styles?.backgroundColor.trim()
    || "#ffffff";
  const radius = styles?.getPropertyValue("--radius-md").trim();

  clone.setAttribute("xmlns", SVG_NAMESPACE);
  clone.removeAttribute("aria-hidden");
  clone.style.setProperty("--mermaid-surface-background", background);
  if (radius) clone.style.setProperty("--radius-md", radius);

  if (size) {
    clone.setAttribute("width", String(size.width));
    clone.setAttribute("height", String(size.height));
    clone.style.width = `${size.width}px`;
    clone.style.height = `${size.height}px`;
    clone.style.maxWidth = "none";
    clone.style.maxHeight = "none";

    const backgroundRect = host.ownerDocument.createElementNS(SVG_NAMESPACE, "rect");
    backgroundRect.setAttribute("x", String(size.x));
    backgroundRect.setAttribute("y", String(size.y));
    backgroundRect.setAttribute("width", String(size.width));
    backgroundRect.setAttribute("height", String(size.height));
    backgroundRect.setAttribute("fill", background);
    backgroundRect.setAttribute("data-mnemora-mermaid-background", "true");
    clone.prepend(backgroundRect);
  }

  const Serializer = host.ownerDocument.defaultView?.XMLSerializer ?? XMLSerializer;
  const serialized = new Serializer().serializeToString(clone);
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(serialized)}`;
}

export function readMermaidIntrinsicSize(svg: SVGSVGElement): MermaidIntrinsicSize | null {
  const viewBox = svg.getAttribute("viewBox")?.trim().split(/[\s,]+/).map(Number);
  if (viewBox?.length === 4 && viewBox.every(Number.isFinite) && viewBox[2] > 0 && viewBox[3] > 0) {
    return { x: viewBox[0], y: viewBox[1], width: viewBox[2], height: viewBox[3] };
  }

  const width = parseSvgLength(svg.getAttribute("width"));
  const height = parseSvgLength(svg.getAttribute("height"));
  return width > 0 && height > 0 ? { x: 0, y: 0, width, height } : null;
}

function readContentWidth(host: HTMLElement) {
  // 宿主可能还没进文档（挂载期），或在测试里是个精简替身；测不出宽度时返回 0，
  // 调用方据此退回「按原尺寸 + maxWidth 贴合」的保守路径。
  if (typeof host.getBoundingClientRect !== "function") return host.clientWidth ?? 0;
  const styles = host.ownerDocument.defaultView?.getComputedStyle(host);
  const horizontalInsets = styles
    ? (Number.parseFloat(styles.borderLeftWidth) || 0)
      + (Number.parseFloat(styles.borderRightWidth) || 0)
      + (Number.parseFloat(styles.paddingLeft) || 0)
      + (Number.parseFloat(styles.paddingRight) || 0)
    : 0;
  const measuredWidth = host.getBoundingClientRect().width || host.clientWidth;
  return Math.max(0, measuredWidth - horizontalInsets);
}

function parseSvgLength(value: string | null) {
  const match = value?.trim().match(/^([-+\d.eE]+)/);
  return match ? Number(match[1]) : Number.NaN;
}
