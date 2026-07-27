import { describe, expect, it } from "vitest";
import {
  DEFAULT_LAYOUT_PREFERENCES,
  normalizeLayoutPreferences,
} from "./useLayoutPreferences";

describe("normalizeLayoutPreferences", () => {
  it("为缺失或无效字段使用默认宽度", () => {
    expect(normalizeLayoutPreferences({
      chatSidebarWidth: Number.NaN,
      workContextWidth: "wide",
    })).toEqual(DEFAULT_LAYOUT_PREFERENCES);
  });

  it("把持久化宽度限制在各面板允许范围内", () => {
    expect(normalizeLayoutPreferences({
      chatSidebarWidth: 9999,
      workSidebarWidth: 100,
      workContextWidth: 480.6,
    })).toEqual({
      chatSidebarWidth: 380,
      workSidebarWidth: 220,
      workContextWidth: 481,
    });
  });
});
