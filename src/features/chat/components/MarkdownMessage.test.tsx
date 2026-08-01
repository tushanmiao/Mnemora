import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MarkdownMessage } from "./MarkdownMessage";
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
        content={'行内公式 $E=mc^2$\n\n$$\n\\int_0^1 x^2 dx\n$$'}
        components={{}}
      />,
    );

    expect(output).toContain('class="katex"');
    expect(output).toContain('class="katex-display"');
    expect(output).toContain('<math');
  });
});
