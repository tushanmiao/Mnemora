import { MARKDOWN_RENDER_LIMITS } from "./renderLimits";

export type MarkdownOutlineItem = {
  id: string;
  level: number;
  title: string;
  offset: number;
};

function cleanHeadingText(value: string) {
  return value
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/[`*_~]/g, "")
    .replace(/<[^>]+>/g, "")
    .replace(/\s+#+\s*$/, "")
    .trim();
}

export function headingId(messageId: string, offset: number) {
  return `mnemora-heading-${messageId.replace(/[^a-zA-Z0-9_-]/g, "-")}-${offset}`;
}

/** 只对完成态 Markdown 建立目录，忽略围栏代码中的伪标题。 */
export function extractMarkdownOutline(content: string, messageId: string): MarkdownOutlineItem[] {
  const items: MarkdownOutlineItem[] = [];
  let offset = 0;
  let fence: string | null = null;
  for (const line of content.match(/[^\n]*(?:\n|$)/g)?.filter(Boolean) ?? []) {
    const fenceMatch = line.match(/^[ \t]{0,3}(`{3,}|~{3,})/);
    if (fenceMatch) {
      if (!fence) fence = fenceMatch[1][0];
      else if (fenceMatch[1][0] === fence) fence = null;
    } else if (!fence) {
      const heading = line.match(/^[ \t]{0,3}(#{1,6})[ \t]+(.+?)(?:\n)?$/);
      if (heading) {
        const title = cleanHeadingText(heading[2]);
        if (title) items.push({
          id: headingId(messageId, offset),
          level: heading[1].length,
          title,
          offset,
        });
      }
    }
    offset += line.length;
    if (items.length >= MARKDOWN_RENDER_LIMITS.maxOutlineItems) break;
  }
  return items;
}

export function headingIdFromNode(messageId: string, node: unknown) {
  const position = (node as { position?: { start?: { offset?: number } } }).position;
  const offset = position?.start?.offset;
  return typeof offset === "number" ? headingId(messageId, offset) : undefined;
}

