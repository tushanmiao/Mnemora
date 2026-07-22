import { describe, expect, it } from "vitest";
import {
  formatCost,
  formatDate,
  formatDuration,
  formatNumber,
  formatSpeed,
} from "./usageFormatters";

describe("usageFormatters", () => {
  it.each([null, undefined, Number.NaN, Number.POSITIVE_INFINITY])(
    "将不可用数值 %s 显示为占位符",
    (value) => {
      expect(formatNumber(value)).toBe("-");
      expect(formatDuration(value)).toBe("-");
      expect(formatSpeed(value)).toBe("-");
      expect(formatCost(value)).toBe("-");
      expect(formatDate(value)).toBe("-");
    },
  );

  it("正常格式化有效的用量数值", () => {
    expect(formatDuration(1_500)).toBe("1.5 s");
    expect(formatSpeed(12.34)).toBe("12.3 tok/s");
    expect(formatCost(0.001)).toBe("$0.00100");
  });
});
