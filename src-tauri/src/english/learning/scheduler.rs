use chrono::{DateTime, TimeZone, Utc};
use rs_fsrs::{Card, Parameters, Rating, State, FSRS};

use super::types::{EnglishRating, EnglishRatingPreview};

pub const SCHEDULER_VERSION: &str = "rs-fsrs-1.2.1";

pub fn new_card(now_ms: i64) -> Card {
    let now = timestamp(now_ms);
    Card {
        due: now,
        last_review: now,
        ..Card::default()
    }
}

pub fn schedule(card: Card, rating: EnglishRating, retention: f64, now_ms: i64) -> Card {
    engine(retention)
        .next(card, timestamp(now_ms), to_fsrs_rating(rating))
        .card
}

pub fn previews(card: &Card, retention: f64, now_ms: i64) -> Vec<EnglishRatingPreview> {
    let log = engine(retention).repeat(card.clone(), timestamp(now_ms));
    [
        EnglishRating::Again,
        EnglishRating::Hard,
        EnglishRating::Good,
        EnglishRating::Easy,
    ]
    .into_iter()
    .filter_map(|rating| {
        log.get(&to_fsrs_rating(rating))
            .map(|info| EnglishRatingPreview {
                rating,
                due_at: info.card.due.timestamp_millis().max(0) as u64,
                scheduled_days: info.card.scheduled_days,
            })
    })
    .collect()
}

pub fn state_name(state: State) -> &'static str {
    match state {
        State::New => "new",
        State::Learning => "learning",
        State::Review => "review",
        State::Relearning => "relearning",
    }
}

fn engine(retention: f64) -> FSRS {
    let mut parameters = Parameters::default();
    parameters.request_retention = retention.clamp(0.8, 0.97);
    parameters.enable_fuzz = false;
    FSRS::new(parameters)
}

fn timestamp(value: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(value)
        .single()
        .unwrap_or_else(Utc::now)
}

fn to_fsrs_rating(rating: EnglishRating) -> Rating {
    match rating {
        EnglishRating::Again => Rating::Again,
        EnglishRating::Hard => Rating::Hard,
        EnglishRating::Good => Rating::Good,
        EnglishRating::Easy => Rating::Easy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_ratings_produce_ordered_due_dates() {
        let now = 1_700_000_000_000i64;
        let previews = previews(&new_card(now), 0.9, now);
        assert_eq!(previews.len(), 4);
        assert!(previews[0].due_at < previews[1].due_at);
        assert!(previews[1].due_at < previews[2].due_at);
        assert!(previews[2].due_at < previews[3].due_at);
    }
}
