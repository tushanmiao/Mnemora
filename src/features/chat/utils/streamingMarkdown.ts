const REFERENCE_DEFINITION_PATTERN = /(?:^|\n)[ \t]{0,3}\[[^\]\n]+\]:[ \t]*/;
const FENCE_PATTERN = /^[ \t]{0,3}(`{3,}|~{3,})/;

/**
 * 流式阶段按空行切分 Markdown，同时保证围栏代码块不会被拆开。
 * 引用式链接需要跨块解析，因此检测到定义时退回单块模式。
 */
export function splitStreamingMarkdownBlocks(content: string) {
  if (!content) return [];
  if (REFERENCE_DEFINITION_PATTERN.test(content)) return [content];

  const blocks: string[] = [];
  const lines = content.match(/[^\n]*(?:\n|$)/g)?.filter(Boolean) ?? [];
  let currentBlock = "";
  let fenceMarker: string | null = null;

  for (const line of lines) {
    currentBlock += line;
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

    if (closedFence || (!fenceMarker && line.trim() === "" && currentBlock.trim())) {
      blocks.push(currentBlock);
      currentBlock = "";
    }
  }

  if (currentBlock) blocks.push(currentBlock);
  return blocks;
}
