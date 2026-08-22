import type { MermaidSvgMetrics } from "./mermaidSecurity";

export type MermaidViewMode = "fit" | "width" | "actual";

export type MermaidPreviewLayout = {
  minRenderWidth: number;
  maxRenderWidth: number;
  projectedWidth: number;
  projectedHeight: number;
  requiresViewport: boolean;
};

const PREVIEW_HEIGHT_LIMIT = 680;
const VIEWER_PADDING = 48;

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
  return Math.min(4, Math.max(0.05, scale));
}
