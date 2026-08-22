import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MarkdownMessage, normalizeMathFenceLanguage } from "./MarkdownMessage";
import { MathMarkdownContent } from "./MathMarkdownContent";

describe("MarkdownMessage safe HTML", () => {
  it("renders allowed layout tags and strips active or app-spoofing HTML", () => {
    const output = renderToStaticMarkup(
      <MarkdownMessage
        content={'<div class="fake" style="color:red" onclick="bad()"><span>safe</span><script>bad()</script><iframe src="https://example.com">frame</iframe><a href="javascript:bad()">blocked</a><a href="https://example.com">allowed</a></div>'}
      />,
    );

    expect(output).toContain("<div><span>safe</span>");
    expect(output).toContain('href="https://example.com"');
    expect(output).not.toContain("class=\"fake\"");
    expect(output).not.toContain("style=");
    expect(output).not.toContain("onclick");
    expect(output).not.toContain("<script");
    expect(output).not.toContain("bad()");
    expect(output).not.toContain("<iframe");
    expect(output).not.toContain('href="javascript:');
  });

  it("keeps the generated language class needed by the HTML preview button", () => {
    const output = renderToStaticMarkup(
      <MarkdownMessage content={'```html\n<div>preview</div>\n```'} />,
    );

    expect(output).toContain('class="language-html"');
    expect(output).toContain('aria-label="预览 HTML"');
  });

  it("renders inline and block LaTeX with KaTeX", () => {
    const output = renderToStaticMarkup(
      <MathMarkdownContent
        content={'行内公式 $E=mc^2$\n\n$$\n\\int_0^1 x^2 dx\n$$\n\n```math\np = \\frac{1}{1 + e^{-x}}\n```'}
        components={{}}
      />,
    );

    expect(output).toContain('class="katex"');
    expect(output).toContain('class="katex-display"');
    expect(output).toContain('<math');
  });

  it("renders fenced math blocks with KaTeX instead of a code block", () => {
    const output = renderToStaticMarkup(
      <MarkdownMessage
        messageId="math-fenced"
        content={'#### 记忆保持率\n\n```math\np = \\frac{1}{1 + e^{-x}}\n```'}
      />,
    );

    expect(output).toContain("markdown-math-loading");
    expect(output).not.toContain('class="language-math"');
    expect(output).not.toContain('markdown-highlighted-code');
  });

  it("normalizes legacy LaTeX and TeX fence aliases", () => {
    expect(normalizeMathFenceLanguage("```latex\nx^2\n```"))
      .toBe("```math\nx^2\n```");
    expect(normalizeMathFenceLanguage("```tex\ny^2\n```"))
      .toBe("```math\ny^2\n```");
  });

  it("renders Mermaid as an enhanced block with a source fallback", () => {
    const output = renderToStaticMarkup(
      <MarkdownMessage
        messageId="assistant-1"
        content={'```mermaid\nflowchart TD\nA[开始] --> B[结束]\n```'}
      />,
    );

    expect(output).toContain("mermaid");
    expect(output).toContain("flowchart TD");
    expect(output).toContain("显示 Mermaid 图表");
  });

  it("safely previews Mermaid nested in a Markdown source fence and keeps a source toggle", () => {
    const output = renderToStaticMarkup(
      <MarkdownMessage
        messageId="nested-mermaid-source"
        content={'````markdown\n### 示例\n\n```mermaid\nflowchart TD\nA[开始] --> B[结束]\n```\n````'}
      />,
    );

    expect(output).toContain("已按安全预览渲染");
    expect(output).toContain('aria-label="查看 Markdown 源码"');
    expect(output).toContain('class="markdown-mermaid-block"');
  });

  it("limits Mermaid rendering without leaking the count across renders", () => {
    const content = Array.from({ length: 11 }, (_, index) => (
      `\`\`\`mermaid\nflowchart TD\nA${index}-->B${index}\n\`\`\``
    )).join("\n\n");

    const first = renderToStaticMarkup(<MarkdownMessage content={content} />);
    const second = renderToStaticMarkup(<MarkdownMessage content={content} />);

    expect(first.match(/class="markdown-mermaid-block"/g)).toHaveLength(10);
    expect(second.match(/class="markdown-mermaid-block"/g)).toHaveLength(10);
  });

  it("renders callouts, footnotes, scoped headings, and safe images", () => {
    const output = renderToStaticMarkup(
      <MarkdownMessage
        messageId="assistant-2"
        content={'# 结论\n\n> [!NOTE]\n> 重要补充[^1]\n\n![图表](https://example.com/figure.png)\n\n[^1]: 补充说明'}
      />,
    );

    expect(output).toContain('data-callout="note"');
    expect(output).toContain('src="https://example.com/figure.png"');
    expect(output).toContain("data-footnotes");
    expect(output).toContain("mnemora-doc-assistant-2-footnote-label");
    expect(output).toContain("mnemora-heading-assistant-2-0");
  });

  it("links a verified literature citation to its PDF callback", () => {
    const output = renderToStaticMarkup(
      <MarkdownMessage
        messageId="assistant-3"
        literatureReferences={[{
          id: "ref-1",
          libraryItemId: "paper-1",
          title: "Paper",
          pageIndex: 2,
          kind: "page",
          text: "excerpt",
        }]}
        content="结论见【Paper，第 3 页】。"
      />,
    );

    expect(output).toContain('href="mnemora-citation:ref-1"');
    expect(output).toContain("markdown-literature-citation");
  });
});
