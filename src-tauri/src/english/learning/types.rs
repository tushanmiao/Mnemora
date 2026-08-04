use serde::{Deserialize, Serialize};

use crate::english::types::EnglishWordEntry;

pub const MAX_BOOK_NAME_CHARS: usize = 80;
pub const MAX_ANSWER_CHARS: usize = 500;
pub const MAX_BATCH_SIZE: u32 = 100;
pub const PORTABLE_BOOK_FORMAT: &str = "mnemora-english-word-book";
pub const PORTABLE_BOOK_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnglishRating {
    Again,
    Hard,
    Good,
    Easy,
}

impl EnglishRating {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Again => "again",
            Self::Hard => "hard",
            Self::Good => "good",
            Self::Easy => "easy",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnglishExerciseKind {
    MeaningRecall,
    Spelling,
    Dictation,
}

impl EnglishExerciseKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MeaningRecall => "meaning_recall",
            Self::Spelling => "spelling",
            Self::Dictation => "dictation",
        }
    }

    pub fn skill(self) -> &'static str {
        match self {
            Self::MeaningRecall => "meaning",
            Self::Spelling => "spelling",
            Self::Dictation => "listening",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnglishVerdict {
    Correct,
    Acceptable,
    Incorrect,
    Skipped,
}

impl EnglishVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Correct => "correct",
            Self::Acceptable => "acceptable",
            Self::Incorrect => "incorrect",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnglishQueueMode {
    Mixed,
    Review,
    New,
    Dictation,
    Spelling,
    Mistakes,
    Mastered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnglishPlanSettings {
    pub new_batch_size: u32,
    pub daily_new_target: u32,
    pub review_batch_size: u32,
    pub daily_review_target: u32,
    pub desired_retention: f64,
    pub preferred_accent: String,
    pub auto_play: bool,
    pub playback_rate: f64,
    pub mastered_audits: bool,
    pub pause_new_words: bool,
    pub rest_days: Vec<u8>,
    pub audio_cache_max_mb: u32,
    pub audio_prefetch_days: u8,
    #[serde(alias = "pageSize")]
    pub dictionary_page_size: u16,
    pub archive_page_size: u16,
    pub history_page_size: u16,
}

impl Default for EnglishPlanSettings {
    fn default() -> Self {
        Self {
            new_batch_size: 10,
            daily_new_target: 20,
            review_batch_size: 20,
            daily_review_target: 50,
            desired_retention: 0.9,
            preferred_accent: "british".to_string(),
            auto_play: true,
            playback_rate: 1.0,
            mastered_audits: true,
            pause_new_words: false,
            rest_days: Vec::new(),
            audio_cache_max_mb: 256,
            audio_prefetch_days: 3,
            dictionary_page_size: 20,
            archive_page_size: 20,
            history_page_size: 20,
        }
    }
}

impl EnglishPlanSettings {
    pub fn validate(mut self) -> Result<Self, String> {
        if !(1..=MAX_BATCH_SIZE).contains(&self.new_batch_size)
            || !(1..=MAX_BATCH_SIZE).contains(&self.review_batch_size)
        {
            return Err("每组单词数必须在 1 到 100 之间。".to_string());
        }
        if !(1..=500).contains(&self.daily_new_target)
            || !(1..=2000).contains(&self.daily_review_target)
        {
            return Err("每日目标超出允许范围。".to_string());
        }
        if !(0.8..=0.97).contains(&self.desired_retention) {
            return Err("目标保持率必须在 0.80 到 0.97 之间。".to_string());
        }
        if !matches!(self.preferred_accent.as_str(), "british" | "american") {
            return Err("首选发音必须是英音或美音。".to_string());
        }
        if !(0.6..=1.5).contains(&self.playback_rate) {
            return Err("播放速度必须在 0.6x 到 1.5x 之间。".to_string());
        }
        self.rest_days.sort_unstable();
        self.rest_days.dedup();
        if self.rest_days.iter().any(|day| *day > 6) || self.rest_days.len() >= 7 {
            return Err(invalid_plan_settings_error());
        }
        if self.audio_cache_max_mb > 2048 || self.audio_prefetch_days > 30 {
            return Err(invalid_plan_settings_error());
        }
        if [
            self.dictionary_page_size,
            self.archive_page_size,
            self.history_page_size,
        ]
        .into_iter()
        .any(|page_size| !matches!(page_size, 20 | 40))
        {
            return Err("词典、归档和答题记录的每页数量只能是 20 或 40。".to_string());
        }
        Ok(self)
    }
}

fn invalid_plan_settings_error() -> String {
    "休息日或音频缓存设置无效。".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishCreatePlanInput {
    pub name: String,
    pub group_ids: Vec<u32>,
    #[serde(default)]
    pub settings: EnglishPlanSettings,
}

impl EnglishCreatePlanInput {
    pub fn validate(self) -> Result<Self, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() || name.chars().count() > MAX_BOOK_NAME_CHARS {
            return Err("词书名称不能为空且最多 80 个字符。".to_string());
        }
        if self.group_ids.is_empty() || self.group_ids.len() > 100 {
            return Err("请至少选择一个词典分组。".to_string());
        }
        let mut group_ids = self.group_ids;
        group_ids.sort_unstable();
        group_ids.dedup();
        Ok(Self {
            name,
            group_ids,
            settings: self.settings.validate()?,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishUpdatePlanInput {
    pub plan_id: String,
    pub settings: EnglishPlanSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishNextBatchInput {
    #[serde(default = "default_queue_mode")]
    pub mode: EnglishQueueMode,
}

fn default_queue_mode() -> EnglishQueueMode {
    EnglishQueueMode::Mixed
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishSubmitAttemptInput {
    pub attempt_id: String,
    pub progress_id: String,
    pub exercise_kind: EnglishExerciseKind,
    pub raw_answer: String,
    pub hint_level: u8,
    pub hint_count: u16,
    pub response_ms: u32,
    pub final_rating: EnglishRating,
}

impl EnglishSubmitAttemptInput {
    pub fn validate(self) -> Result<Self, String> {
        if self.attempt_id.trim().is_empty() || self.attempt_id.len() > 80 {
            return Err("答题记录 ID 无效。".to_string());
        }
        if self.progress_id.trim().is_empty() || self.progress_id.len() > 80 {
            return Err("学习进度 ID 无效。".to_string());
        }
        if self.raw_answer.chars().count() > MAX_ANSWER_CHARS {
            return Err("答案最多 500 个字符。".to_string());
        }
        if self.hint_level > 5 || self.hint_count > 50 || self.response_ms > 3_600_000 {
            return Err("答题数据超出允许范围。".to_string());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EnglishLearningSnapshot {
    pub dictionary_id: u32,
    pub entry_key: String,
    pub source_version: String,
    pub word: String,
    pub group_id: u32,
    pub group_name: String,
    pub pronunciation: String,
    pub translation: String,
    pub example: String,
    pub example_translation: String,
    pub british_audio: String,
    pub american_audio: String,
    pub mnemonic: String,
    pub root_affixes: String,
}

impl From<&EnglishWordEntry> for EnglishLearningSnapshot {
    fn from(entry: &EnglishWordEntry) -> Self {
        Self {
            dictionary_id: entry.id,
            entry_key: entry.entry_key.clone(),
            source_version: entry.source_version.clone(),
            word: entry.word.clone(),
            group_id: entry.group_id,
            group_name: entry.group_name.clone(),
            pronunciation: entry.pronunciation.clone(),
            translation: entry.translation.clone(),
            example: entry.example.clone(),
            example_translation: entry.example_translation.clone(),
            british_audio: entry.british_audio.clone(),
            american_audio: entry.american_audio.clone(),
            mnemonic: entry.mnemonic.clone(),
            root_affixes: entry.root_affixes.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishPlanSummary {
    pub id: String,
    pub book_id: String,
    pub book_name: String,
    pub status: String,
    pub item_count: usize,
    pub settings: EnglishPlanSettings,
    pub started_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnglishLearningOverview {
    pub active_plan: Option<EnglishPlanSummary>,
    pub due_count: usize,
    pub overdue_count: usize,
    pub mastered_due_count: usize,
    pub new_available: usize,
    pub today_new_done: usize,
    pub today_review_done: usize,
    pub learned_count: usize,
    pub mastered_count: usize,
    pub archived_count: usize,
    pub weak_skill: Option<String>,
    pub estimated_completion_at: Option<u64>,
    pub is_rest_day: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishRatingPreview {
    pub rating: EnglishRating,
    pub due_at: u64,
    pub scheduled_days: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishQueueItem {
    pub progress_id: String,
    pub item_id: String,
    pub state: String,
    pub exercise_kind: EnglishExerciseKind,
    pub snapshot: EnglishLearningSnapshot,
    pub due_at: Option<u64>,
    pub rating_previews: Vec<EnglishRatingPreview>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishAttemptResult {
    pub attempt_id: String,
    pub duplicate: bool,
    pub verdict: EnglishVerdict,
    pub suggested_rating: EnglishRating,
    pub final_rating: EnglishRating,
    pub next_due_at: u64,
    pub scheduled_days: i64,
    pub state: String,
    pub overview: EnglishLearningOverview,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnglishSkillSummary {
    pub skill: String,
    pub attempts: usize,
    pub correct: usize,
    pub hint_uses: usize,
    pub average_response_ms: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnglishLearningStats {
    pub attempts_7d: usize,
    pub correct_7d: usize,
    pub hint_uses_7d: usize,
    pub average_response_ms_7d: u64,
    pub due_backlog: usize,
    pub active_days_7d: usize,
    pub current_streak_days: usize,
    pub skills: Vec<EnglishSkillSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnglishPortableBook {
    pub format: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub settings: EnglishPlanSettings,
    pub entries: Vec<EnglishLearningSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishAudioCacheStatus {
    pub bytes: u64,
    pub files: usize,
    pub max_bytes: u64,
    pub prefetch_days: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishCachedAudio {
    pub path: String,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishAttemptHistoryItem {
    pub id: String,
    pub word: String,
    pub exercise_kind: EnglishExerciseKind,
    pub raw_answer: String,
    pub verdict: EnglishVerdict,
    pub suggested_rating: EnglishRating,
    pub final_rating: EnglishRating,
    pub hint_level: u8,
    pub hint_count: u16,
    pub response_ms: u32,
    pub reviewed_at: u64,
    pub next_due_at: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnglishAttemptHistoryPage {
    pub items: Vec<EnglishAttemptHistoryItem>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishArchivedItem {
    pub progress_id: String,
    pub word: String,
    pub translation: String,
    pub pronunciation: String,
    pub previous_state: String,
    pub archived_at: u64,
}
