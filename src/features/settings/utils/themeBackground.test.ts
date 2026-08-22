import { describe, expect, it } from "vitest";
import {
  resolveThemeBackgroundCss,
  validateThemeBackgroundCss,
} from "./themeBackground";

describe("theme background validation", () => {
  it("接受颜色和渐变背景", () => {
    expect(validateThemeBackgroundCss("#f7f8f6")).toBeNull();
    expect(validateThemeBackgroundCss("linear-gradient(135deg, #f7f8f6, #dfeae3)")).toBeNull();
    expect(validateThemeBackgroundCss("repeating-linear-gradient(135deg, transparent 0 12px, #fff 12px 13px)")).toBeNull();
  });

  it("拒绝外部资源和完整样式表", () => {
    expect(validateThemeBackgroundCss("url(https://example.com/bg.png)")).toBeNull();
    expect(validateThemeBackgroundCss("body { color: red; }")).toContain("完整样式表");
    expect(validateThemeBackgroundCss("filter(blur(4px))")).toContain("不支持");
  });

  it("只解析启用且有效的背景", () => {
    expect(resolveThemeBackgroundCss({ enabled: false, css: "red", surfaceOpacity: 92 })).toBeNull();
    expect(resolveThemeBackgroundCss({ enabled: true, css: "red", surfaceOpacity: 92 })).toBe("red");
    expect(resolveThemeBackgroundCss({ enabled: true, css: "url(x)", surfaceOpacity: 92 })).toBeNull();
    expect(resolveThemeBackgroundCss({
      enabled: true,
      css: "center / cover no-repeat url('https://images.example.com/background.webp')",
      surfaceOpacity: 92,
    })).toContain("https://images.example.com/background.webp");
    expect(resolveThemeBackgroundCss({ enabled: true, css: "url(javascript:alert(1))", surfaceOpacity: 92 })).toBeNull();
    expect(resolveThemeBackgroundCss({ enabled: true, css: "url(file:///C:/secret.png)", surfaceOpacity: 92 })).toBeNull();
  });
});
