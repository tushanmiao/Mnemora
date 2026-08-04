import type { EnglishExerciseKind, EnglishRating, EnglishVerdict } from "../api/learning";

export function normalizeEnglishAnswer(value: string) {
  return value.trim().replace(/\s+/g, " ").toLocaleLowerCase("en-US");
}

export function judgeEnglishAnswer(
  exercise: EnglishExerciseKind,
  rawAnswer: string,
  word: string,
  translation: string,
): EnglishVerdict {
  const answer = normalizeEnglishAnswer(rawAnswer);
  if (!answer) return "skipped";
  const expected = normalizeEnglishAnswer(exercise === "meaning_recall" ? translation : word);
  if (answer === expected) return "correct";
  if (exercise === "meaning_recall") {
    const meanings = expected.split(/[；;，,\n]/).map((part) => part.trim()).filter(Boolean);
    if (meanings.some((meaning) => answer === meaning || expected.includes(answer))) return "acceptable";
  }
  return "incorrect";
}

export function suggestEnglishRating(
  verdict: EnglishVerdict,
  hintLevel: number,
  responseMs: number,
): EnglishRating {
  if (hintLevel >= 5 || verdict === "incorrect" || verdict === "skipped") return "again";
  if (hintLevel > 0 || verdict === "acceptable") return "hard";
  return responseMs > 0 && responseMs <= 5_000 ? "easy" : "good";
}
