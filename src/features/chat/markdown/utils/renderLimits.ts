/** 学习内容渲染的统一资源上限，避免模型输出异常内容拖慢 WebView。 */
export const MARKDOWN_RENDER_LIMITS = {
  maxMermaidChars: 24_000,
  // Inline expansion keeps one SVG tree, but very large foreignObject graphs
  // are still expensive whenever their bounded viewBox changes while scrolling.
  maxMermaidViewerSvgChars: 600_000,
  maxMermaidViewerElements: 12_000,
  maxMermaidViewerForeignObjects: 800,
  maxMermaidViewerIntrinsicDimension: 50_000,
  maxMermaidBlocksPerMessage: 10,
  maxHighlightedCodeChars: 32_000,
  maxDecodedImagePixels: 25_000_000,
  maxRemoteImageBytes: 8 * 1024 * 1024,
  maxOutlineItems: 32,
  maxLongCodeLines: 48,
} as const;

export type MarkdownRenderLimits = typeof MARKDOWN_RENDER_LIMITS;
