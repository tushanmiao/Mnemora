import type { MermaidSvgMetrics } from "./mermaidSecurity";

export type MermaidViewMode = "fit" | "width" | "actual";

export type MermaidPreviewLayout = {
  minRenderWidth: number;
  maxRenderWidth: number;
  projectedWidth: number;
  projectedHeight: number;
  requiresViewport: boolean;
};

export type MermaidViewerViewport = {
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
};

export type MermaidScrollLayout = {
  contentWidth: number;
  contentHeight: number;
  viewport: MermaidViewerViewport;
};

const PREVIEW_HEIGHT_LIMIT = 680;
const VIEWER_PADDING = 48;
const MIN_VIEWER_SCALE = 0.001;
const MAX_VIEWER_SCALE = 32;
/**
 * Fit modes shrink, they never magnify. A narrow diagram stretched to fill the
 * container turned 13px labels into 40px+ text and cropped the viewBox down to
 * a small slice of the chart. Magnification stays available through the
 * viewer's explicit zoom controls.
 */
const MAX_AUTO_FIT_SCALE = 1;
/**
 * Below this scale a shrunk-to-fit diagram is no longer readable, so the
 * preview switches to fit-width (100% at most) and crops vertically instead,
 * surfacing the interactive viewer for the rest.
 */
const MIN_READABLE_PREVIEW_SCALE = 0.55;

export function getMermaidPreviewLayout(metrics: MermaidSvgMetrics, containerWidth: number): MermaidPreviewLayout {
  const safeContainerWidth = Math.max(280, Number.isFinite(containerWidth) ? containerWidth : 900);
  const projectedWidth = Math.min(metrics.width, safeContainerWidth);
  const projectedHeight = projectedWidth / metrics.aspectRatio;
  const extremeDimensions = metrics.width > 1_800
    || metrics.height > 1_200
    || metrics.aspectRatio > 3.2
    || metrics.aspectRatio < 0.38;

  return {
    minRenderWidth: projectedWidth,
    maxRenderWidth: projectedWidth,
    projectedWidth,
    projectedHeight,
    requiresViewport: extremeDimensions
      || projectedHeight > PREVIEW_HEIGHT_LIMIT,
  };
}

export function isLargeMermaidDiagram(metrics: MermaidSvgMetrics, containerWidth = 900) {
  return getMermaidPreviewLayout(metrics, containerWidth).requiresViewport;
}

export function getDefaultMermaidViewMode(metrics: MermaidSvgMetrics): MermaidViewMode {
  return metrics.aspectRatio < 0.85 ? "width" : "fit";
}

/**
 * The preview is not a viewer: it must show the whole diagram whenever that
 * stays legible. Shrink-to-fit is preferred, and fit-width (capped at 100%,
 * top-anchored) is the fallback for charts too long to shrink.
 */
export function getMermaidPreviewViewMode(
  metrics: MermaidSvgMetrics,
  canvas: { width: number; height: number },
): MermaidViewMode {
  return getMermaidViewerScale(metrics, canvas, "fit") >= MIN_READABLE_PREVIEW_SCALE ? "fit" : "width";
}

export function getMermaidViewerScale(
  metrics: MermaidSvgMetrics,
  canvas: { width: number; height: number },
  mode: MermaidViewMode,
) {
  if (mode === "actual") return 1;

  const availableWidth = Math.max(1, canvas.width - VIEWER_PADDING);
  const availableHeight = Math.max(1, canvas.height - VIEWER_PADDING);
  const widthScale = availableWidth / metrics.width;
  const heightScale = availableHeight / metrics.height;
  const scale = mode === "width" ? widthScale : Math.min(widthScale, heightScale);
  return Math.min(MAX_VIEWER_SCALE, Math.max(MIN_VIEWER_SCALE, Math.min(scale, MAX_AUTO_FIT_SCALE)));
}

/**
 * Returns a bounded SVG viewBox for the interactive viewer. The old viewer
 * created a host with the diagram's intrinsic height and then transformed it;
 * a 40,000px chart could therefore allocate a giant compositing/layout layer.
 * Cropping the viewBox keeps the DOM and canvas dimensions constant while
 * retaining pan/zoom navigation for the complete diagram.
 */
export function getMermaidViewerViewport(
  metrics: MermaidSvgMetrics,
  canvas: { width: number; height: number },
  mode: MermaidViewMode,
  zoom = 1,
  pan = { x: 0, y: 0 },
): MermaidViewerViewport {
  const safeCanvasWidth = Math.max(1, Number.isFinite(canvas.width) ? canvas.width : 1);
  const safeCanvasHeight = Math.max(1, Number.isFinite(canvas.height) ? canvas.height : 1);
  const baseScale = getMermaidViewerScale(metrics, { width: safeCanvasWidth, height: safeCanvasHeight }, mode);
  const scale = Math.min(MAX_VIEWER_SCALE, Math.max(MIN_VIEWER_SCALE, baseScale * (Number.isFinite(zoom) ? zoom : 1)));
  // Keep the viewBox aspect ratio identical to the canvas. Dimensions may be
  // larger than the diagram, which intentionally produces a small margin
  // without distorting or independently stretching either axis.
  const width = Math.max(1, safeCanvasWidth / scale);
  const height = Math.max(1, safeCanvasHeight / scale);
  const x = getAxisOrigin(metrics.x ?? 0, metrics.width, width, pan.x, scale, false);
  // Fit-width is intentionally top-anchored: a long process diagram starts at
  // its title instead of opening on an arbitrary middle segment.
  const y = getAxisOrigin(metrics.y ?? 0, metrics.height, height, pan.y, scale, mode === "width");
  return { x, y, width, height, scale };
}

/**
 * Maps a native scroll container onto a bounded SVG viewBox. Only the visible
 * slice is painted; the spacer provides scroll range without creating a
 * diagram-sized SVG/compositing layer.
 */
export function getMermaidScrollLayout(
  metrics: MermaidSvgMetrics,
  canvas: { width: number; height: number },
  scroll: { left: number; top: number },
  padding = 24,
): MermaidScrollLayout {
  const canvasWidth = Math.max(1, Number.isFinite(canvas.width) ? canvas.width : 1);
  const canvasHeight = Math.max(1, Number.isFinite(canvas.height) ? canvas.height : 1);
  const safePadding = Math.max(0, Number.isFinite(padding) ? padding : 0);
  const contentWidth = Math.max(canvasWidth, metrics.width + safePadding * 2);
  const contentHeight = Math.max(canvasHeight, metrics.height + safePadding * 2);
  const offsetX = (contentWidth - metrics.width) / 2;
  const offsetY = (contentHeight - metrics.height) / 2;
  const maxLeft = Math.max(0, contentWidth - canvasWidth);
  const maxTop = Math.max(0, contentHeight - canvasHeight);
  const left = clamp(Number.isFinite(scroll.left) ? scroll.left : 0, 0, maxLeft);
  const top = clamp(Number.isFinite(scroll.top) ? scroll.top : 0, 0, maxTop);

  return {
    contentWidth,
    contentHeight,
    viewport: {
      x: (metrics.x ?? 0) - offsetX + left,
      y: (metrics.y ?? 0) - offsetY + top,
      width: canvasWidth,
      height: canvasHeight,
      scale: 1,
    },
  };
}

function getAxisOrigin(diagramStart: number, diagramSize: number, viewportSize: number, panPixels: number, scale: number, anchorStart: boolean) {
  if (viewportSize >= diagramSize) return diagramStart + (diagramSize - viewportSize) / 2;
  const max = diagramStart + diagramSize - viewportSize;
  const origin = anchorStart ? diagramStart : diagramStart + (diagramSize - viewportSize) / 2;
  return clamp(origin - panPixels / scale, diagramStart, max);
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}
