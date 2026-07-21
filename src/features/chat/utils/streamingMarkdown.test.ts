import { describe, expect, it } from "vitest";
import {
  renderableStreamingBlock,
  splitStreamingMarkdownBlocks,
} from "./streamingMarkdown";

describe("streaming Markdown HTML blocks", () => {
  it("keeps a multiline HTML fragment together across blank lines", () => {
    const content = "<div>\n\nfirst\n\n<span>second</span>\n\n</div>\n\nnext";
    const blocks = splitStreamingMarkdownBlocks(content);

    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toEqual({
      content: "<div>\n\nfirst\n\n<span>second</span>\n\n</div>\n\n",
      htmlComplete: true,
    });
  });

  it("shows an unfinished HTML tail as escaped text", () => {
    const blocks = splitStreamingMarkdownBlocks("answer\n\n<div>partial");
    const block = blocks[blocks.length - 1];

    expect(block.htmlComplete).toBe(false);
    expect(renderableStreamingBlock(block)).toContain("&lt;div&gt;partial");
  });

  it("does not treat HTML inside a fenced code block as raw HTML", () => {
    const blocks = splitStreamingMarkdownBlocks("```html\n<div>\n```\n\nafter");

    expect(blocks[0].htmlComplete).toBe(true);
    expect(blocks).toHaveLength(2);
  });
});
