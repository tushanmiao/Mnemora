import { describe, expect, it } from "vitest";
import { judgeEnglishAnswer, normalizeEnglishAnswer, suggestEnglishRating } from "./answerNormalization";

describe("English answer scoring", () => {
  it("normalizes whitespace and case without removing punctuation", () => {
    expect(normalizeEnglishAnswer("  Mother-in-law  ")).toBe("mother-in-law");
    expect(normalizeEnglishAnswer("can't")).not.toBe(normalizeEnglishAnswer("cant"));
  });

  it("accepts an explicit meaning segment", () => {
    expect(judgeEnglishAnswer("meaning_recall", "存款", "deposit", "n. 存款；押金")).toBe("acceptable");
  });

  it("caps strong hints at hard and full answers at again", () => {
    expect(suggestEnglishRating("correct", 3, 1000)).toBe("hard");
    expect(suggestEnglishRating("correct", 5, 1000)).toBe("again");
  });
});
