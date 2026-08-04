use super::types::{EnglishExerciseKind, EnglishRating, EnglishVerdict};

pub fn normalize_answer(value: &str) -> String {
    value
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn judge_answer(
    exercise: EnglishExerciseKind,
    raw_answer: &str,
    word: &str,
    translation: &str,
) -> (String, EnglishVerdict) {
    let normalized = normalize_answer(raw_answer);
    if normalized.is_empty() {
        return (normalized, EnglishVerdict::Skipped);
    }
    let expected = match exercise {
        EnglishExerciseKind::MeaningRecall => normalize_answer(translation),
        EnglishExerciseKind::Spelling | EnglishExerciseKind::Dictation => normalize_answer(word),
    };
    if normalized == expected {
        return (normalized, EnglishVerdict::Correct);
    }
    if matches!(exercise, EnglishExerciseKind::MeaningRecall)
        && expected
            .split(['；', ';', '，', ',', '\n'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .any(|value| normalized == value || expected.contains(&normalized))
    {
        return (normalized, EnglishVerdict::Acceptable);
    }
    (normalized, EnglishVerdict::Incorrect)
}

pub fn suggest_rating(verdict: EnglishVerdict, hint_level: u8, response_ms: u32) -> EnglishRating {
    if hint_level >= 5 || matches!(verdict, EnglishVerdict::Incorrect | EnglishVerdict::Skipped) {
        return EnglishRating::Again;
    }
    if hint_level >= 3 || hint_level > 0 || matches!(verdict, EnglishVerdict::Acceptable) {
        return EnglishRating::Hard;
    }
    if response_ms > 0 && response_ms <= 5_000 {
        EnglishRating::Easy
    } else {
        EnglishRating::Good
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_meaningful_hyphens_and_apostrophes() {
        assert_eq!(normalize_answer("  Mother-in-law  "), "mother-in-law");
        assert_ne!(normalize_answer("cant"), normalize_answer("can't"));
    }

    #[test]
    fn strong_hints_cap_the_suggestion() {
        assert_eq!(
            suggest_rating(EnglishVerdict::Correct, 3, 1000),
            EnglishRating::Hard
        );
        assert_eq!(
            suggest_rating(EnglishVerdict::Correct, 5, 1000),
            EnglishRating::Again
        );
    }
}
