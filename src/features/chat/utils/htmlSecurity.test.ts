import { describe, expect, it } from "vitest";
import {
  safeMarkdownContentUrlTransform,
  safeMarkdownImageUrlTransform,
  safeMarkdownUrlTransform,
} from "./htmlSecurity";

describe("Markdown URL security", () => {
  it("allows scoped anchors and trusted document links", () => {
    expect(safeMarkdownUrlTransform("#mnemora-doc-message-fn-1")).toBe("#mnemora-doc-message-fn-1");
    expect(safeMarkdownUrlTransform("https://example.com/paper")).toBe("https://example.com/paper");
    expect(safeMarkdownUrlTransform("javascript:alert(1)")).toBe("");
  });

  it("only allows HTTPS or app-controlled image schemes", () => {
    expect(safeMarkdownImageUrlTransform("https://example.com/figure.png")).toContain("https://");
    expect(safeMarkdownImageUrlTransform("http://example.com/figure.png")).toBe("");
    expect(safeMarkdownImageUrlTransform("data:image/svg+xml,bad")).toBe("");
    expect(safeMarkdownContentUrlTransform("mailto:test@example.com", "href")).toContain("mailto:");
  });
});

