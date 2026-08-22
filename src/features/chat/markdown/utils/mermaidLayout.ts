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

const PREVIEW_HEIGHT_LIMIT = 680;
const VIEWER_PADDING = 48;
const MIN_VIEWER_SCALE = 0.001;
const MAX_VIEWER_SCALE = 32;

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
  return Math.min(MAX_VIEWER_SCALE, Math.max(MIN_VIEWER_SCALE, scale));
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
  const x = getAxisOrigin(metrics.width, width, pan.x, scale, false);
  // Fit-width is intentionally top-anchored: a long process diagram starts at
  // its title instead of opening on an arbitrary middle segment.
  const y = getAxisOrigin(metrics.height, height, pan.y, scale, mode === "width");
  return { x, y, width, height, scale };
}

function getAxisOrigin(diagramSize: number, viewportSize: number, panPixels: number, scale: number, anchorStart: boolean) {
  if (viewportSize >= diagramSize) return (diagramSize - viewportSize) / 2;
  const max = diagramSize - viewportSize;
  const origin = anchorStart ? 0 : max / 2;
  return clamp(origin - panPixels / scale, 0, max);
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}
