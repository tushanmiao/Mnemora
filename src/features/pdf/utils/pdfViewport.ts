const PDF_PAGE_HORIZONTAL_CHROME = 76;
const MIN_FIT_PAGE_WIDTH = 280;
const MAX_FIT_PAGE_WIDTH = 1100;
const MAX_CANVAS_PIXELS = 12_000_000;

export type PdfPageDisplaySize = {
  width: number;
  height: number;
};

/**
 * 以阅读区域宽度为基准计算页面尺寸，避免高倍缩放后再用页面自身宽度重复放大。
 */
export function resolvePdfPageDisplaySize(
  readerWidth: number,
  pageWidth: number,
  pageHeight: number,
  zoom: number,
): PdfPageDisplaySize {
  const safeReaderWidth = Number.isFinite(readerWidth) ? readerWidth : 0;
  const safePageWidth = Number.isFinite(pageWidth) && pageWidth > 0 ? pageWidth : 595;
  const safePageHeight = Number.isFinite(pageHeight) && pageHeight > 0 ? pageHeight : 842;
  const safeZoom = Number.isFinite(zoom) ? Math.max(0.5, Math.min(3, zoom)) : 1;
  const fitWidth = Math.max(
    MIN_FIT_PAGE_WIDTH,
    Math.min(safeReaderWidth - PDF_PAGE_HORIZONTAL_CHROME, MAX_FIT_PAGE_WIDTH),
  );
  const width = fitWidth * safeZoom;
  return {
    width,
    height: width * (safePageHeight / safePageWidth),
  };
}

/**
 * 限制单页 Canvas 的像素总量。高倍缩放仍保持正确的 CSS 尺寸，但不会无限放大
 * 后备位图，避免少数超大页面迅速占满内存。
 */
export function resolvePdfCanvasScale(
  width: number,
  height: number,
  devicePixelRatio: number,
): number {
  const safeWidth = Math.max(1, width);
  const safeHeight = Math.max(1, height);
  const requestedScale = Math.max(0.1, Math.min(devicePixelRatio || 1, 2));
  const pixelLimitedScale = Math.sqrt(MAX_CANVAS_PIXELS / (safeWidth * safeHeight));
  return Math.max(0.1, Math.min(requestedScale, pixelLimitedScale));
}
