const REFERENCE_DEFINITION_PATTERN = /(?:^|\n)[ \t]{0,3}\[[^\]\n]+\]:[ \t]*/;
const FENCE_PATTERN = /^[ \t]{0,3}(`{3,}|~{3,})/;
const HTML_TAG_PATTERN = /<(\/?)\s*([a-z][a-z0-9-]*)\b([^<>]*?)(\/?)>/gi;
const INLINE_CODE_PATTERN = /`[^`]*`/g;
const TRACKED_HTML_TAGS = new Set([
  "a", "blockquote", "code", "del", "div", "em", "h1", "h2", "h3", "h4", "h5", "h6",
  "li", "ol", "p", "pre", "span", "strong", "table", "tbody", "td", "th", "thead", "tr", "ul",
]);

export type StreamingMarkdownBlock = {
  content: string;
  htmlComplete: boolean;
};

function updateOpenHtmlTags(line: string, openTags: string[]) {
  const source = line.replace(INLINE_CODE_PATTERN, "");
  HTML_TAG_PATTERN.lastIndex = 0;
  for (const match of source.matchAll(HTML_TAG_PATTERN)) {
    const closing = match[1] === "/";
    const tag = match[2].toLowerCase();
    const selfClosing = match[4] === "/";
    if (!TRACKED_HTML_TAGS.has(tag) || selfClosing) continue;

    if (!closing) {
      openTags.push(tag);
      continue;
    }

    const index = openTags.lastIndexOf(tag);
    if (index >= 0) openTags.splice(index);
  }
}

function escapeIncompleteHtml(content: string) {
  return content.replace(/<\/?[a-z][^<>]*(?:>|$)/gi, (tag) => (
    tag.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
  ));
}

/** 未闭合的流式 HTML 暂时按文本显示，闭合后才交给 rehype-raw 解析。 */
export function renderableStreamingBlock(block: StreamingMarkdownBlock) {
  return block.htmlComplete ? block.content : escapeIncompleteHtml(block.content);
}

/**
 * 流式阶段按空行切分 Markdown，同时保持围栏代码块和跨行 HTML 片段完整。
 * 已完成块可以稳定 memo；只有最后一个未完成块会随 token 更新。
 */
export function splitStreamingMarkdownBlocks(content: string): StreamingMarkdownBlock[] {
  if (!content) return [];

  const blocks: StreamingMarkdownBlock[] = [];
  const lines = content.match(/[^\n]*(?:\n|$)/g)?.filter(Boolean) ?? [];
  let currentBlock = "";
  let fenceMarker: string | null = null;
  let openHtmlTags: string[] = [];
  const keepWholeMessage = REFERENCE_DEFINITION_PATTERN.test(content);

  for (const line of lines) {
    currentBlock += line;
    const wasInsideFence = fenceMarker !== null;
    const fenceMatch = line.match(FENCE_PATTERN);
    let closedFence = false;

    if (fenceMatch) {
      const marker = fenceMatch[1];
      if (!fenceMarker) {
        fenceMarker = marker;
      } else if (
        marker[0] === fenceMarker[0]
        && marker.length >= fenceMarker.length
        && line.slice(fenceMatch[0].length).trim() === ""
      ) {
        fenceMarker = null;
        closedFence = true;
      }
    }

    if (!wasInsideFence && !fenceMatch) updateOpenHtmlTags(line, openHtmlTags);

    if (!keepWholeMessage && (
      closedFence
      || (!fenceMarker && openHtmlTags.length === 0 && line.trim() === "" && currentBlock.trim())
    )) {
      blocks.push({ content: currentBlock, htmlComplete: true });
      currentBlock = "";
      openHtmlTags = [];
    }
  }

  if (currentBlock) {
    blocks.push({ content: currentBlock, htmlComplete: openHtmlTags.length === 0 });
  }
  return blocks;
}
