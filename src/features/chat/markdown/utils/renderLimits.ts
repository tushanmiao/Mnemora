/** 学习内容渲染的统一资源上限，避免模型输出异常内容拖慢 WebView。 */
export const MARKDOWN_RENDER_LIMITS = {
  maxMermaidChars: 24_000,
  maxMermaidBlocksPerMessage: 10,
  maxHighlightedCodeChars: 32_000,
  maxDecodedImagePixels: 25_000_000,
  maxRemoteImageBytes: 8 * 1024 * 1024,
  maxOutlineItems: 32,
  maxLongCodeLines: 48,
} as const;

export type MarkdownRenderLimits = typeof MARKDOWN_RENDER_LIMITS;

