import type { MermaidSvgMetrics } from "./mermaidSecurity";

export type MermaidViewMode = "fit" | "width" | "actual";

export type MermaidPreviewLayout = {
  minRenderWidth: number;
  maxRenderWidth: number;
  projectedWidth: number;
  projectedHeight: number;
  requiresViewport: boolean;
};

const MIN_READABLE_SCALE = 0.75;
const MAX_COMFORTABLE_SCALE = 1.25;
const EXTREME_MIN_READABLE_SCALE = 1;
const EXTREME_MAX_COMFORTABLE_SCALE = 1.5;
const PREVIEW_HEIGHT_LIMIT = 680;
const PREVIEW_WIDTH_OVERFLOW_RATIO = 1.12;
const VIEWER_PADDING = 48;

export function getMermaidPreviewLayout(metrics: MermaidSvgMetrics, containerWidth: number): MermaidPreviewLayout {
  const safeContainerWidth = Math.max(280, Number.isFinite(containerWidth) ? containerWidth : 900);
  const preserveIntrinsicScale = metrics.aspectRatio < 0.85 || metrics.aspectRatio > 3.2;
  const minScale = preserveIntrinsicScale ? EXTREME_MIN_READABLE_SCALE : MIN_READABLE_SCALE;
  const maxScale = preserveIntrinsicScale ? EXTREME_MAX_COMFORTABLE_SCALE : MAX_COMFORTABLE_SCALE;
  const minRenderWidth = Math.max(1, Math.round(metrics.width * minScale));
  const maxRenderWidth = Math.max(minRenderWidth, Math.round(metrics.width * maxScale));
  const projectedWidth = Math.min(Math.max(safeContainerWidth, minRenderWidth), maxRenderWidth);
  const projectedHeight = projectedWidth / metrics.aspectRatio;
  const extremeDimensions = metrics.width > 1_800
    || metrics.height > 1_200
    || metrics.aspectRatio > 3.2
    || metrics.aspectRatio < 0.38;

  return {
    minRenderWidth,
    maxRenderWidth,
    projectedWidth,
    projectedHeight,
    requiresViewport: extremeDimensions
      || projectedHeight > PREVIEW_HEIGHT_LIMIT
      || projectedWidth > safeContainerWidth * PREVIEW_WIDTH_OVERFLOW_RATIO,
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
  return Math.min(4, Math.max(0.05, scale));
}
