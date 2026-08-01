import { describe, expect, it } from "vitest";
import { isMermaidLanguage, normalizeCodeLanguage } from "./codeBlock";

describe("enhanced code language", () => {
  it("normalizes common aliases without automatic language detection", () => {
    expect(normalizeCodeLanguage("TS")).toBe("typescript");
    expect(normalizeCodeLanguage("rs")).toBe("rust");
    expect(normalizeCodeLanguage(undefined)).toBeNull();
  });

  it("recognizes Mermaid case-insensitively", () => {
    expect(isMermaidLanguage("MerMaid")).toBe(true);
    expect(isMermaidLanguage("markdown")).toBe(false);
  });
});

