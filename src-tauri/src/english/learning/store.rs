use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{Datelike, Duration, Local, TimeZone};
use rs_fsrs::Card;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::english::types::EnglishWordEntry;

use super::{
    scheduler::{self, SCHEDULER_VERSION},
    scoring::{judge_answer, suggest_rating},
    types::{
        EnglishArchivedItem, EnglishAttemptHistoryItem, EnglishAttemptHistoryPage,
        EnglishAttemptResult, EnglishAudioCacheStatus, EnglishCachedAudio, EnglishCreatePlanInput,
        EnglishExerciseKind, EnglishLearningOverview, EnglishLearningSnapshot,
        EnglishLearningStats, EnglishNextBatchInput, EnglishPlanSettings, EnglishPlanSummary,
        EnglishPortableBook, EnglishQueueItem, EnglishQueueMode, EnglishRating,
        EnglishSkillSummary, EnglishSubmitAttemptInput, EnglishUpdatePlanInput, EnglishVerdict,
        PORTABLE_BOOK_FORMAT, PORTABLE_BOOK_VERSION,
    },
};

const LEARNING_SCHEMA_VERSION: i64 = 3;
const MAX_ATTEMPT_HISTORY: usize = 1_000;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone)]
pub struct EnglishLearningRepository {
    database_path: PathBuf,
    audio_cache_path: PathBuf,
}

#[derive(Debug)]
struct QueueRecord {
    progress_id: String,
    item_id: String,
    state: String,
    due_at: Option<i64>,
    card_json: String,
    snapshot: EnglishLearningSnapshot,
}

#[derive(Debug)]
struct AttemptProgress {
    progress_id: String,
    state: String,
    audit_step: i64,
    card_json: String,
    retention: f64,
    snapshot: EnglishLearningSnapshot,
}

impl EnglishLearningRepository {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            database_path: app_data_dir.join("english").join("learning.sqlite3"),
            audio_cache_path: app_data_dir.join("english").join("audio-cache"),
        }
    }

    pub fn overview(&self) -> Result<EnglishLearningOverview, String> {
        let connection = self.open_connection()?;
        overview_with_connection(&connection, now_millis())
    }

    pub fn export_active_book(&self) -> Result<String, String> {
        let connection = self.open_connection()?;
        let plan = active_plan_with_connection(&connection)?
            .ok_or_else(|| "当前没有可导出的英语学习计划。".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT i.snapshot_json FROM english_book_items bi
                 JOIN english_learning_items i ON i.id = bi.item_id
                 WHERE bi.book_id = ? ORDER BY bi.position",
            )
            .map_err(|error| format!("准备导出英语词书失败：{error}"))?;
        let rows = statement
            .query_map(params![plan.book_id], |row| row.get::<_, String>(0))
            .map_err(|error| format!("读取英语词书失败：{error}"))?;
        let mut entries = Vec::with_capacity(plan.item_count);
        for row in rows {
            let snapshot_json = row.map_err(|error| format!("读取英语单词快照失败：{error}"))?;
            entries.push(
                serde_json::from_str(&snapshot_json)
                    .map_err(|error| format!("解析英语单词快照失败：{error}"))?,
            );
        }
        serde_json::to_string_pretty(&EnglishPortableBook {
            format: PORTABLE_BOOK_FORMAT.to_string(),
            version: PORTABLE_BOOK_VERSION,
            name: plan.book_name,
            settings: plan.settings,
            entries,
        })
        .map_err(|error| format!("序列化英语词书失败：{error}"))
    }

    pub fn import_portable_book(&self, contents: &str) -> Result<EnglishPlanSummary, String> {
        let mut portable: EnglishPortableBook = serde_json::from_str(contents)
            .map_err(|error| format!("解析英语词书文件失败：{error}"))?;
        if portable.format != PORTABLE_BOOK_FORMAT || portable.version != PORTABLE_BOOK_VERSION {
            return Err("不支持的英语词书格式或版本。".to_string());
        }
        portable.name = portable.name.trim().to_string();
        if portable.name.is_empty() || portable.name.chars().count() > 80 {
            return Err("词书名称不能为空且最多 80 个字符。".to_string());
        }
        if portable.entries.is_empty() || portable.entries.len() > 50_000 {
            return Err("词书必须包含 1 到 50000 个单词。".to_string());
        }
        let mut seen = HashSet::new();
        for entry in &mut portable.entries {
            entry.word = entry.word.trim().to_string();
            if entry.word.is_empty() {
                return Err("词书中存在空单词。".to_string());
            }
            let identity = format!(
                "{}\n{}\n{}\n{}",
                portable.name.to_lowercase(),
                entry.word.to_lowercase(),
                entry.translation.trim(),
                entry.pronunciation.trim()
            );
            entry.entry_key = format!("custom:{:x}", Sha256::digest(identity.as_bytes()));
            if !seen.insert(entry.entry_key.clone()) {
                return Err(format!("词书中存在重复单词：{}", entry.word));
            }
            entry.dictionary_id = 0;
            entry.group_id = 0;
            if entry.group_name.trim().is_empty() {
                entry.group_name = portable.name.clone();
            }
            entry.source_version = format!("portable-v{}", portable.version);
        }
        let summary = self.create_plan(
            EnglishCreatePlanInput {
                name: portable.name,
                group_ids: vec![0],
                settings: portable.settings,
            },
            portable.entries,
        )?;
        let connection = self.open_connection()?;
        connection
            .execute(
                "UPDATE english_books SET source_kind = 'custom', source_ref = ? WHERE id = ?",
                params![PORTABLE_BOOK_FORMAT, summary.book_id],
            )
            .map_err(|error| format!("标记自定义英语词书失败：{error}"))?;
        Ok(summary)
    }

    pub fn audio_cache_settings(&self) -> Result<(u64, u8), String> {
        let connection = self.open_connection()?;
        let settings = active_plan_with_connection(&connection)?
            .map(|plan| plan.settings)
            .unwrap_or_default();
        Ok((
            u64::from(settings.audio_cache_max_mb) * 1024 * 1024,
            settings.audio_prefetch_days,
        ))
    }

    pub fn cached_audio(&self, url: &str) -> Option<EnglishCachedAudio> {
        let path = self.audio_cache_file(url);
        path.is_file().then(|| EnglishCachedAudio {
            path: path.to_string_lossy().into_owned(),
            cached: true,
        })
    }

    pub fn store_cached_audio(
        &self,
        url: &str,
        bytes: &[u8],
        max_bytes: u64,
    ) -> Result<EnglishCachedAudio, String> {
        fs::create_dir_all(&self.audio_cache_path)
            .map_err(|error| format!("创建英语音频缓存目录失败：{error}"))?;
        let path = self.audio_cache_file(url);
        let temporary = path.with_extension("download");
        fs::write(&temporary, bytes).map_err(|error| format!("写入英语音频缓存失败：{error}"))?;
        if path.exists() {
            fs::remove_file(&temporary).ok();
        } else {
            fs::rename(&temporary, &path)
                .map_err(|error| format!("保存英语音频缓存失败：{error}"))?;
        }
        self.prune_audio_cache(max_bytes)?;
        if !path.is_file() {
            return Err("音频文件超过缓存容量上限。".to_string());
        }
        Ok(EnglishCachedAudio {
            path: path.to_string_lossy().into_owned(),
            cached: true,
        })
    }

    pub fn audio_cache_status(&self) -> Result<EnglishAudioCacheStatus, String> {
        let (max_bytes, prefetch_days) = self.audio_cache_settings()?;
        let (bytes, files) = cache_usage(&self.audio_cache_path)?;
        Ok(EnglishAudioCacheStatus {
            bytes,
            files,
            max_bytes,
            prefetch_days,
        })
    }

    pub fn clear_audio_cache(&self) -> Result<EnglishAudioCacheStatus, String> {
        if self.audio_cache_path.exists() {
            fs::remove_dir_all(&self.audio_cache_path)
                .map_err(|error| format!("清理英语音频缓存失败：{error}"))?;
        }
        self.audio_cache_status()
    }

    pub fn prefetch_audio_urls(&self) -> Result<Vec<String>, String> {
        let connection = self.open_connection()?;
        let Some(plan) = active_plan_with_connection(&connection)? else {
            return Ok(Vec::new());
        };
        if plan.settings.audio_prefetch_days == 0 || plan.settings.audio_cache_max_mb == 0 {
            return Ok(Vec::new());
        }
        let limit = usize::try_from(plan.settings.daily_new_target)
            .unwrap_or(0)
            .saturating_mul(usize::from(plan.settings.audio_prefetch_days))
            .saturating_add(plan.settings.review_batch_size as usize)
            .clamp(1, 1000);
        let cutoff = now_millis() + i64::from(plan.settings.audio_prefetch_days) * DAY_MS;
        let mut statement = connection
            .prepare(
                "SELECT i.snapshot_json FROM english_item_progress p
                 JOIN english_learning_items i ON i.id = p.item_id
                 WHERE p.plan_id = ? AND p.state != 'archived'
                   AND (p.state = 'new' OR p.due_at <= ? OR p.audit_due_at <= ?)
                 ORDER BY CASE WHEN p.state = 'new' THEN 1 ELSE 0 END, COALESCE(p.due_at, p.audit_due_at, 0)
                 LIMIT ?",
            )
            .map_err(|error| format!("准备英语音频预下载失败：{error}"))?;
        let rows = statement
            .query_map(params![plan.id, cutoff, cutoff, limit as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| format!("读取英语音频预下载队列失败：{error}"))?;
        let mut urls = Vec::new();
        let mut seen = HashSet::new();
        for row in rows {
            let snapshot: EnglishLearningSnapshot = serde_json::from_str(
                &row.map_err(|error| format!("读取英语音频快照失败：{error}"))?,
            )
            .map_err(|error| format!("解析英语音频快照失败：{error}"))?;
            let url = if plan.settings.preferred_accent == "american" {
                snapshot.american_audio
            } else {
                snapshot.british_audio
            };
            if !url.trim().is_empty() && seen.insert(url.clone()) {
                urls.push(url);
            }
        }
        Ok(urls)
    }

    fn audio_cache_file(&self, url: &str) -> PathBuf {
        let extension = audio_extension(url);
        let name = format!("{:x}.{extension}", Sha256::digest(url.as_bytes()));
        self.audio_cache_path.join(name)
    }

    fn prune_audio_cache(&self, max_bytes: u64) -> Result<(), String> {
        let mut files = cache_files(&self.audio_cache_path)?;
        let mut total = files.iter().map(|item| item.1).sum::<u64>();
        files.sort_by_key(|item| item.2);
        for (path, size, _) in files {
            if total <= max_bytes {
                break;
            }
            fs::remove_file(&path).map_err(|error| format!("删除过期英语音频缓存失败：{error}"))?;
            total = total.saturating_sub(size);
        }
        Ok(())
    }

    pub fn create_plan(
        &self,
        input: EnglishCreatePlanInput,
        snapshots: Vec<EnglishLearningSnapshot>,
    ) -> Result<EnglishPlanSummary, String> {
        let input = input.validate()?;
        if snapshots.is_empty() {
            return Err("词书中没有可学习的单词。".to_string());
        }
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始英语计划事务失败：{error}"))?;
        let now = now_millis();
        transaction
            .execute(
                "UPDATE english_plans SET status = 'paused', updated_at = ? WHERE status = 'active'",
                params![now],
            )
            .map_err(|error| format!("暂停旧英语计划失败：{error}"))?;

        let book_id = Uuid::new_v4().to_string();
        let plan_id = Uuid::new_v4().to_string();
        let source_ref = serde_json::to_string(&input.group_ids)
            .map_err(|error| format!("序列化词典分组失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO english_books (
                    id, name, source_kind, source_ref, item_count, created_at, updated_at
                 ) VALUES (?, ?, 'dictionary', ?, ?, ?, ?)",
                params![
                    book_id,
                    input.name,
                    source_ref,
                    snapshots.len() as i64,
                    now,
                    now
                ],
            )
            .map_err(|error| format!("创建英语词书失败：{error}"))?;

        for (position, snapshot) in snapshots.into_iter().enumerate() {
            let snapshot_json = serde_json::to_string(&snapshot)
                .map_err(|error| format!("序列化单词快照失败：{error}"))?;
            let entry_key = if snapshot.entry_key.trim().is_empty() {
                format!(
                    "legacy:{}:{}:{}",
                    snapshot.group_id,
                    snapshot.word.trim().to_lowercase(),
                    snapshot.pronunciation
                )
            } else {
                snapshot.entry_key.clone()
            };
            transaction
                .execute(
                    "INSERT INTO english_learning_items (
                        id, kind, source_kind, source_ref, source_version, entry_key,
                        canonical_text, normalized_text, translation, snapshot_json,
                        created_at, updated_at
                     ) VALUES (?, 'word', 'dictionary', ?, ?, ?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(source_kind, entry_key) DO UPDATE SET
                        source_ref = excluded.source_ref,
                        source_version = excluded.source_version,
                        canonical_text = excluded.canonical_text,
                        normalized_text = excluded.normalized_text,
                        translation = excluded.translation,
                        snapshot_json = excluded.snapshot_json,
                        updated_at = excluded.updated_at",
                    params![
                        Uuid::new_v4().to_string(),
                        snapshot.dictionary_id.to_string(),
                        snapshot.source_version,
                        entry_key,
                        snapshot.word,
                        snapshot.word.trim().to_lowercase(),
                        snapshot.translation,
                        snapshot_json,
                        now,
                        now,
                    ],
                )
                .map_err(|error| format!("保存英语学习项失败：{error}"))?;
            let item_id: String = transaction
                .query_row(
                    "SELECT id FROM english_learning_items WHERE source_kind = 'dictionary' AND entry_key = ?",
                    params![entry_key],
                    |row| row.get(0),
                )
                .map_err(|error| format!("读取英语学习项失败：{error}"))?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO english_book_items (book_id, item_id, position) VALUES (?, ?, ?)",
                    params![book_id, item_id, position as i64],
                )
                .map_err(|error| format!("保存词书顺序失败：{error}"))?;
        }
        transaction
            .execute(
                "UPDATE english_books SET item_count = (
                    SELECT COUNT(*) FROM english_book_items WHERE book_id = ?
                 ), updated_at = ? WHERE id = ?",
                params![book_id, now, book_id],
            )
            .map_err(|error| format!("更新英语词书数量失败：{error}"))?;

        let settings_json = serde_json::to_string(&input.settings)
            .map_err(|error| format!("序列化英语计划设置失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO english_plans (
                    id, book_id, status, settings_json, started_at, updated_at
                 ) VALUES (?, ?, 'active', ?, ?, ?)",
                params![plan_id, book_id, settings_json, now, now],
            )
            .map_err(|error| format!("创建英语学习计划失败：{error}"))?;

        let card_json = serde_json::to_string(&scheduler::new_card(now))
            .map_err(|error| format!("序列化 FSRS 初始状态失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO english_item_progress (
                    id, plan_id, item_id, state, due_at, card_json, scheduler_version,
                    created_at, updated_at
                 )
                 SELECT lower(hex(randomblob(16))), ?, bi.item_id, 'new', NULL, ?, ?, ?, ?
                 FROM english_book_items bi WHERE bi.book_id = ?",
                params![plan_id, card_json, SCHEDULER_VERSION, now, now, book_id],
            )
            .map_err(|error| format!("初始化英语学习进度失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("保存英语学习计划失败：{error}"))?;

        let connection = self.open_connection()?;
        active_plan_with_connection(&connection)?
            .ok_or_else(|| "英语学习计划创建后不可用。".to_string())
    }

    pub fn add_word(&self, entry: &EnglishWordEntry) -> Result<EnglishLearningOverview, String> {
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始加入英语学习项事务失败：{error}"))?;
        let (plan_id, book_id): (String, String) = transaction
            .query_row(
                "SELECT p.id, p.book_id FROM english_plans p
                 WHERE p.status = 'active' LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| format!("当前没有启用的英语学习计划：{error}"))?;
        let snapshot = EnglishLearningSnapshot::from(entry);
        if snapshot.entry_key.trim().is_empty() {
            return Err("词典条目缺少稳定身份，无法加入学习计划。".to_string());
        }
        let now = now_millis();
        let snapshot_json = serde_json::to_string(&snapshot)
            .map_err(|error| format!("序列化单词快照失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO english_learning_items (
                    id, kind, source_kind, source_ref, source_version, entry_key,
                    canonical_text, normalized_text, translation, snapshot_json,
                    created_at, updated_at
                 ) VALUES (?, 'word', 'dictionary', ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(source_kind, entry_key) DO UPDATE SET
                    source_version = excluded.source_version,
                    canonical_text = excluded.canonical_text,
                    normalized_text = excluded.normalized_text,
                    translation = excluded.translation,
                    snapshot_json = excluded.snapshot_json,
                    updated_at = excluded.updated_at",
                params![
                    Uuid::new_v4().to_string(),
                    snapshot.dictionary_id.to_string(),
                    snapshot.source_version,
                    snapshot.entry_key,
                    snapshot.word,
                    snapshot.word.trim().to_lowercase(),
                    snapshot.translation,
                    snapshot_json,
                    now,
                    now,
                ],
            )
            .map_err(|error| format!("保存英语学习项失败：{error}"))?;
        let item_id: String = transaction
            .query_row(
                "SELECT id FROM english_learning_items WHERE source_kind = 'dictionary' AND entry_key = ?",
                params![EnglishLearningSnapshot::from(entry).entry_key],
                |row| row.get(0),
            )
            .map_err(|error| format!("读取英语学习项失败：{error}"))?;
        let already_linked: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM english_book_items WHERE book_id = ? AND item_id = ?)",
                params![book_id, item_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("检查英语词书关联失败：{error}"))?;
        if !already_linked {
            let position: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(position) + 1, 0) FROM english_book_items WHERE book_id = ?",
                    params![book_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("读取英语词书位置失败：{error}"))?;
            transaction
                .execute(
                    "INSERT INTO english_book_items (book_id, item_id, position) VALUES (?, ?, ?)",
                    params![book_id, item_id, position],
                )
                .map_err(|error| format!("加入英语词书失败：{error}"))?;
            let card_json = serde_json::to_string(&scheduler::new_card(now))
                .map_err(|error| format!("序列化 FSRS 初始状态失败：{error}"))?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO english_item_progress (
                        id, plan_id, item_id, state, due_at, card_json, scheduler_version,
                        created_at, updated_at
                     ) VALUES (?, ?, ?, 'new', NULL, ?, ?, ?, ?)",
                    params![
                        Uuid::new_v4().to_string(),
                        plan_id,
                        item_id,
                        card_json,
                        SCHEDULER_VERSION,
                        now,
                        now
                    ],
                )
                .map_err(|error| format!("初始化英语学习进度失败：{error}"))?;
            transaction
                .execute(
                    "UPDATE english_books SET item_count = (
                        SELECT COUNT(*) FROM english_book_items WHERE book_id = ?
                     ), updated_at = ? WHERE id = ?",
                    params![book_id, now, book_id],
                )
                .map_err(|error| format!("更新英语词书数量失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("保存英语学习项失败：{error}"))?;
        let connection = self.open_connection()?;
        overview_with_connection(&connection, now)
    }

    pub fn update_plan(&self, input: EnglishUpdatePlanInput) -> Result<EnglishPlanSummary, String> {
        let settings = input.settings.validate()?;
        let audio_cache_max_bytes = u64::from(settings.audio_cache_max_mb) * 1024 * 1024;
        if input.plan_id.trim().is_empty() {
            return Err("英语学习计划 ID 无效。".to_string());
        }
        let connection = self.open_connection()?;
        let settings_json = serde_json::to_string(&settings)
            .map_err(|error| format!("序列化英语计划设置失败：{error}"))?;
        let changed = connection
            .execute(
                "UPDATE english_plans SET settings_json = ?, updated_at = ? WHERE id = ?",
                params![settings_json, now_millis(), input.plan_id],
            )
            .map_err(|error| format!("更新英语学习计划失败：{error}"))?;
        if changed == 0 {
            return Err("英语学习计划不存在。".to_string());
        }
        let plan = plan_by_id(&connection, &input.plan_id)?
            .ok_or_else(|| "英语学习计划不存在。".to_string())?;
        self.prune_audio_cache(audio_cache_max_bytes)?;
        Ok(plan)
    }

    pub fn pause_plan(
        &self,
        plan_id: &str,
        paused: bool,
    ) -> Result<Option<EnglishPlanSummary>, String> {
        if plan_id.trim().is_empty() {
            return Err("英语学习计划 ID 无效。".to_string());
        }
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始切换英语计划事务失败：{error}"))?;
        if !paused {
            transaction.execute(
                "UPDATE english_plans SET status = 'paused', updated_at = ? WHERE status = 'active'",
                params![now_millis()],
            ).map_err(|error| format!("暂停当前英语计划失败：{error}"))?;
        }
        let status = if paused { "paused" } else { "active" };
        let changed = transaction
            .execute(
                "UPDATE english_plans SET status = ?, updated_at = ? WHERE id = ?",
                params![status, now_millis(), plan_id],
            )
            .map_err(|error| format!("切换英语学习计划失败：{error}"))?;
        if changed == 0 {
            return Err("英语学习计划不存在。".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("保存英语计划状态失败：{error}"))?;
        let connection = self.open_connection()?;
        if paused {
            Ok(None)
        } else {
            plan_by_id(&connection, plan_id)
        }
    }

    pub fn next_batch(
        &self,
        input: EnglishNextBatchInput,
    ) -> Result<Vec<EnglishQueueItem>, String> {
        let connection = self.open_connection()?;
        let Some(plan) = active_plan_with_connection(&connection)? else {
            return Ok(Vec::new());
        };
        let now = now_millis();
        let mut records = Vec::new();
        let review_limit = plan.settings.review_batch_size as usize;
        let new_remaining = if is_rest_day(now, &plan.settings.rest_days) {
            0
        } else {
            plan.settings
                .daily_new_target
                .saturating_sub(
                    count_today_new(&connection, &plan.id, day_start_millis(now))? as u32,
                ) as usize
        };

        match input.mode {
            EnglishQueueMode::Review
            | EnglishQueueMode::Mixed
            | EnglishQueueMode::Dictation
            | EnglishQueueMode::Spelling => {
                records.extend(query_due(&connection, &plan.id, now, review_limit)?);
                if records.is_empty()
                    && !matches!(input.mode, EnglishQueueMode::Review)
                    && !plan.settings.pause_new_words
                    && new_remaining > 0
                {
                    records.extend(query_new(
                        &connection,
                        &plan.id,
                        (plan.settings.new_batch_size as usize).min(new_remaining),
                    )?);
                }
            }
            EnglishQueueMode::New => {
                if !plan.settings.pause_new_words && new_remaining > 0 {
                    records.extend(query_new(
                        &connection,
                        &plan.id,
                        (plan.settings.new_batch_size as usize).min(new_remaining),
                    )?);
                }
            }
            EnglishQueueMode::Mistakes => {
                records.extend(query_mistakes(&connection, &plan.id, review_limit)?);
            }
            EnglishQueueMode::Mastered => {
                records.extend(query_mastered(&connection, &plan.id, now, review_limit)?);
            }
        }

        records
            .into_iter()
            .map(|record| {
                queue_item(
                    &connection,
                    record,
                    input.mode,
                    plan.settings.desired_retention,
                    now,
                )
            })
            .collect()
    }

    pub fn get_item(&self, progress_id: &str) -> Result<EnglishQueueItem, String> {
        let connection = self.open_connection()?;
        let plan = active_plan_with_connection(&connection)?
            .ok_or_else(|| "当前没有启用的英语学习计划。".to_string())?;
        let record = query_progress(&connection, progress_id)?
            .ok_or_else(|| "英语学习项不存在。".to_string())?;
        queue_item(
            &connection,
            record,
            EnglishQueueMode::Mixed,
            plan.settings.desired_retention,
            now_millis(),
        )
    }

    pub fn submit_attempt(
        &self,
        input: EnglishSubmitAttemptInput,
    ) -> Result<EnglishAttemptResult, String> {
        let input = input.validate()?;
        let mut connection = self.open_connection()?;
        if let Some(result) = duplicate_attempt_result(&connection, &input.attempt_id)? {
            return Ok(result);
        }
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始保存英语答题事务失败：{error}"))?;
        let progress = attempt_progress(&transaction, &input.progress_id)?
            .ok_or_else(|| "英语学习项不存在。".to_string())?;
        if progress.state == "archived" {
            return Err("已归档单词不能提交复习记录。".to_string());
        }
        let now = now_millis();
        let (normalized_answer, verdict) = judge_answer(
            input.exercise_kind,
            &input.raw_answer,
            &progress.snapshot.word,
            &progress.snapshot.translation,
        );
        let suggested_rating = suggest_rating(verdict, input.hint_level, input.response_ms);
        let previous_card = parse_card(&progress.card_json, now);
        let next_card = scheduler::schedule(
            previous_card.clone(),
            input.final_rating,
            progress.retention,
            now,
        );
        let was_new = progress.state == "new";
        let previous_state_json = serde_json::to_string(&previous_card)
            .map_err(|error| format!("序列化旧 FSRS 状态失败：{error}"))?;
        let next_state_json = serde_json::to_string(&next_card)
            .map_err(|error| format!("序列化新 FSRS 状态失败：{error}"))?;

        let (next_state, audit_due_at, audit_step) = if progress.state == "mastered" {
            if matches!(
                input.final_rating,
                EnglishRating::Again | EnglishRating::Hard
            ) {
                ("relearning".to_string(), None, progress.audit_step)
            } else {
                let step = (progress.audit_step + 1).min(2);
                let days = if step == 1 { 90 } else { 180 };
                ("mastered".to_string(), Some(now + days * DAY_MS), step)
            }
        } else {
            (
                scheduler::state_name(next_card.state).to_string(),
                None,
                progress.audit_step,
            )
        };

        transaction
            .execute(
                "INSERT INTO english_attempts (
                id, progress_id, exercise_kind, raw_answer, normalized_answer, verdict,
                suggested_rating, final_rating, hint_level, hint_count, response_ms,
                previous_state_json, next_state_json, was_new, reviewed_at, scheduler_version
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    input.attempt_id,
                    progress.progress_id,
                    input.exercise_kind.as_str(),
                    input.raw_answer,
                    normalized_answer,
                    verdict.as_str(),
                    suggested_rating.as_str(),
                    input.final_rating.as_str(),
                    i64::from(input.hint_level),
                    i64::from(input.hint_count),
                    i64::from(input.response_ms),
                    previous_state_json,
                    next_state_json,
                    if was_new { 1 } else { 0 },
                    now,
                    SCHEDULER_VERSION,
                ],
            )
            .map_err(|error| format!("保存英语答题记录失败：{error}"))?;

        let is_correct = matches!(
            verdict,
            EnglishVerdict::Correct | EnglishVerdict::Acceptable
        );
        transaction
            .execute(
                "INSERT INTO english_skill_stats (
                progress_id, skill_kind, attempts, correct, hint_uses,
                average_response_ms, last_attempt_at, last_error_at
             ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
             ON CONFLICT(progress_id, skill_kind) DO UPDATE SET
                average_response_ms = (
                    english_skill_stats.average_response_ms * english_skill_stats.attempts
                    + excluded.average_response_ms
                ) / (english_skill_stats.attempts + 1),
                attempts = english_skill_stats.attempts + 1,
                correct = english_skill_stats.correct + excluded.correct,
                hint_uses = english_skill_stats.hint_uses + excluded.hint_uses,
                last_attempt_at = excluded.last_attempt_at,
                last_error_at = CASE
                    WHEN excluded.last_error_at IS NULL THEN english_skill_stats.last_error_at
                    ELSE excluded.last_error_at
                END",
                params![
                    progress.progress_id,
                    input.exercise_kind.skill(),
                    if is_correct { 1 } else { 0 },
                    if input.hint_count > 0 { 1 } else { 0 },
                    i64::from(input.response_ms),
                    now,
                    if is_correct { None } else { Some(now) },
                ],
            )
            .map_err(|error| format!("更新英语技能统计失败：{error}"))?;

        transaction
            .execute(
                "UPDATE english_item_progress SET
                state = ?, stability = ?, difficulty = ?, due_at = ?, last_reviewed_at = ?,
                scheduled_days = ?, reps = ?, lapses = ?,
                introduced_at = COALESCE(introduced_at, ?), audit_due_at = ?, audit_step = ?,
                card_json = ?, scheduler_version = ?, updated_at = ?
             WHERE id = ?",
                params![
                    next_state,
                    next_card.stability,
                    next_card.difficulty,
                    next_card.due.timestamp_millis(),
                    now,
                    next_card.scheduled_days,
                    next_card.reps,
                    next_card.lapses,
                    if was_new { Some(now) } else { None },
                    audit_due_at,
                    audit_step,
                    next_state_json,
                    SCHEDULER_VERSION,
                    now,
                    progress.progress_id,
                ],
            )
            .map_err(|error| format!("更新英语学习进度失败：{error}"))?;
        prune_attempt_history(&transaction)?;
        transaction
            .commit()
            .map_err(|error| format!("提交英语答题记录失败：{error}"))?;

        let connection = self.open_connection()?;
        Ok(EnglishAttemptResult {
            attempt_id: input.attempt_id,
            duplicate: false,
            verdict,
            suggested_rating,
            final_rating: input.final_rating,
            next_due_at: next_card.due.timestamp_millis().max(0) as u64,
            scheduled_days: next_card.scheduled_days,
            state: next_state,
            overview: overview_with_connection(&connection, now)?,
        })
    }

    pub fn mark_mastered(&self, progress_id: &str) -> Result<EnglishLearningOverview, String> {
        let connection = self.open_connection()?;
        let now = now_millis();
        let settings_json: Option<String> = connection
            .query_row(
                "SELECT plan.settings_json FROM english_item_progress p
                 JOIN english_plans plan ON plan.id = p.plan_id WHERE p.id = ?",
                params![progress_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取英语计划设置失败：{error}"))?;
        let settings = settings_json
            .as_deref()
            .map(serde_json::from_str::<EnglishPlanSettings>)
            .transpose()
            .map_err(|error| format!("解析英语计划设置失败：{error}"))?
            .ok_or_else(|| "英语学习项不存在。".to_string())?;
        let audit_due_at = settings.mastered_audits.then_some(now + 30 * DAY_MS);
        let changed = connection.execute(
            "UPDATE english_item_progress SET
                state = 'mastered', mastered_at = ?, audit_due_at = ?, audit_step = 0, updated_at = ?
             WHERE id = ? AND state != 'archived'",
            params![now, audit_due_at, now, progress_id],
        ).map_err(|error| format!("标记已掌握失败：{error}"))?;
        if changed == 0 {
            return Err("英语学习项不存在或已归档。".to_string());
        }
        overview_with_connection(&connection, now)
    }

    pub fn archive_item(&self, progress_id: &str) -> Result<EnglishLearningOverview, String> {
        let connection = self.open_connection()?;
        let now = now_millis();
        let changed = connection
            .execute(
                "UPDATE english_item_progress SET archived_from_state = state, state = 'archived',
                    updated_at = ?
                 WHERE id = ? AND state != 'archived'",
                params![now, progress_id],
            )
            .map_err(|error| format!("归档英语学习项失败：{error}"))?;
        if changed == 0 {
            return Err("英语学习项不存在或已经归档。".to_string());
        }
        overview_with_connection(&connection, now)
    }

    pub fn restore_item(&self, progress_id: &str) -> Result<EnglishLearningOverview, String> {
        let connection = self.open_connection()?;
        let now = now_millis();
        let changed = connection
            .execute(
                "UPDATE english_item_progress SET
                    state = CASE
                        WHEN archived_from_state IN ('new', 'learning', 'review', 'relearning', 'mastered')
                            THEN archived_from_state
                        ELSE 'review'
                    END,
                    due_at = CASE
                        WHEN archived_from_state = 'new' THEN NULL
                        ELSE COALESCE(due_at, ?)
                    END,
                    archived_from_state = NULL, updated_at = ?
                 WHERE id = ? AND state = 'archived'",
                params![now, now, progress_id],
            )
            .map_err(|error| format!("恢复英语学习项失败：{error}"))?;
        if changed == 0 {
            return Err("归档单词不存在或已经恢复。".to_string());
        }
        overview_with_connection(&connection, now)
    }

    pub fn list_archived(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EnglishArchivedItem>, String> {
        let connection = self.open_connection()?;
        let Some(plan) = active_plan_with_connection(&connection)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection
            .prepare(
                "SELECT p.id, i.canonical_text, i.translation, i.snapshot_json,
                        COALESCE(p.archived_from_state, 'review'), p.updated_at
                 FROM english_item_progress p
                 JOIN english_learning_items i ON i.id = p.item_id
                 WHERE p.plan_id = ? AND p.state = 'archived'
                 ORDER BY p.updated_at DESC LIMIT ? OFFSET ?",
            )
            .map_err(|error| format!("准备归档单词列表失败：{error}"))?;
        let rows = statement
            .query_map(
                params![
                    plan.id,
                    limit.clamp(1, 100) as i64,
                    offset.min(i64::MAX as usize) as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(|error| format!("读取归档单词列表失败：{error}"))?;
        let mut archived = Vec::new();
        for row in rows {
            let (progress_id, word, translation, snapshot_json, previous_state, archived_at) =
                row.map_err(|error| format!("读取归档单词失败：{error}"))?;
            let snapshot: EnglishLearningSnapshot = serde_json::from_str(&snapshot_json)
                .map_err(|error| format!("解析归档单词快照失败：{error}"))?;
            archived.push(EnglishArchivedItem {
                progress_id,
                word,
                translation,
                pronunciation: snapshot.pronunciation,
                previous_state,
                archived_at: i64_to_u64(archived_at),
            });
        }
        Ok(archived)
    }

    pub fn stats(&self) -> Result<EnglishLearningStats, String> {
        let connection = self.open_connection()?;
        let Some(plan) = active_plan_with_connection(&connection)? else {
            return Ok(EnglishLearningStats::default());
        };
        let now = now_millis();
        let from = now - 7 * DAY_MS;
        let (attempts, correct, hints, average): (i64, i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN a.verdict IN ('correct', 'acceptable') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN a.hint_count > 0 THEN 1 ELSE 0 END), 0),
                    CAST(COALESCE(AVG(a.response_ms), 0) AS INTEGER)
             FROM english_attempts a
             JOIN english_item_progress p ON p.id = a.progress_id
             WHERE p.plan_id = ? AND a.reviewed_at >= ?",
            params![plan.id, from],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|error| format!("读取近七日英语统计失败：{error}"))?;
        let mut statement = connection
            .prepare(
                "SELECT s.skill_kind, SUM(s.attempts), SUM(s.correct), SUM(s.hint_uses),
                    CASE WHEN SUM(s.attempts) = 0 THEN 0
                         ELSE SUM(s.average_response_ms * s.attempts) / SUM(s.attempts) END
             FROM english_skill_stats s
             JOIN english_item_progress p ON p.id = s.progress_id
             WHERE p.plan_id = ?
             GROUP BY s.skill_kind ORDER BY s.skill_kind",
            )
            .map_err(|error| format!("准备英语技能统计失败：{error}"))?;
        let rows = statement
            .query_map(params![plan.id], |row| {
                Ok(EnglishSkillSummary {
                    skill: row.get(0)?,
                    attempts: i64_to_usize(row.get(1)?),
                    correct: i64_to_usize(row.get(2)?),
                    hint_uses: i64_to_usize(row.get(3)?),
                    average_response_ms: i64_to_u64(row.get(4)?),
                })
            })
            .map_err(|error| format!("查询英语技能统计失败：{error}"))?;
        let mut skills = Vec::new();
        for row in rows {
            skills.push(row.map_err(|error| format!("读取英语技能统计失败：{error}"))?);
        }
        let due_backlog: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM english_item_progress
             WHERE plan_id = ? AND state IN ('learning', 'review', 'relearning') AND due_at <= ?",
                params![plan.id, now],
                |row| row.get(0),
            )
            .map_err(|error| format!("读取英语到期积压失败：{error}"))?;
        let activity_dates = activity_dates(&connection, &plan.id)?;
        let (active_days_7d, current_streak_days) =
            activity_summary(now, &activity_dates, &plan.settings.rest_days);
        Ok(EnglishLearningStats {
            attempts_7d: i64_to_usize(attempts),
            correct_7d: i64_to_usize(correct),
            hint_uses_7d: i64_to_usize(hints),
            average_response_ms_7d: i64_to_u64(average),
            due_backlog: i64_to_usize(due_backlog),
            active_days_7d,
            current_streak_days,
            skills,
        })
    }

    pub fn list_history(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<EnglishAttemptHistoryPage, String> {
        let connection = self.open_connection()?;
        let Some(plan) = active_plan_with_connection(&connection)? else {
            return Ok(EnglishAttemptHistoryPage::default());
        };
        let total: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM english_attempts a
                 JOIN english_item_progress p ON p.id = a.progress_id
                 WHERE p.plan_id = ?",
                params![plan.id],
                |row| row.get(0),
            )
            .map_err(|error| format!("读取英语答题历史数量失败：{error}"))?;
        let mut statement = connection
            .prepare(
                "SELECT a.id, i.canonical_text, a.exercise_kind, a.raw_answer, a.verdict,
                        a.suggested_rating, a.final_rating, a.hint_level, a.hint_count,
                        a.response_ms, a.reviewed_at, a.next_state_json
                 FROM english_attempts a
                 JOIN english_item_progress p ON p.id = a.progress_id
                 JOIN english_learning_items i ON i.id = p.item_id
                 WHERE p.plan_id = ?
                 ORDER BY a.reviewed_at DESC, a.rowid DESC LIMIT ? OFFSET ?",
            )
            .map_err(|error| format!("准备英语答题历史失败：{error}"))?;
        let rows = statement
            .query_map(
                params![
                    plan.id,
                    limit.clamp(1, 100) as i64,
                    offset.min(i64::MAX as usize) as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, String>(11)?,
                    ))
                },
            )
            .map_err(|error| format!("查询英语答题历史失败：{error}"))?;
        let mut history = Vec::new();
        for row in rows {
            let (
                id,
                word,
                exercise,
                raw_answer,
                verdict,
                suggested,
                final_rating,
                hint_level,
                hint_count,
                response_ms,
                reviewed_at,
                next_state_json,
            ) = row.map_err(|error| format!("读取英语答题历史失败：{error}"))?;
            let next_card = serde_json::from_str::<Card>(&next_state_json)
                .map_err(|error| format!("解析英语复习状态失败：{error}"))?;
            history.push(EnglishAttemptHistoryItem {
                id,
                word,
                exercise_kind: parse_exercise(&exercise)?,
                raw_answer,
                verdict: parse_verdict(&verdict)?,
                suggested_rating: parse_rating(&suggested)?,
                final_rating: parse_rating(&final_rating)?,
                hint_level: u8::try_from(hint_level).unwrap_or(0),
                hint_count: u16::try_from(hint_count).unwrap_or(0),
                response_ms: u32::try_from(response_ms).unwrap_or(0),
                reviewed_at: i64_to_u64(reviewed_at),
                next_due_at: i64_to_u64(next_card.due.timestamp_millis()),
            });
        }
        Ok(EnglishAttemptHistoryPage {
            items: history,
            total: i64_to_usize(total),
        })
    }

    fn open_connection(&self) -> Result<Connection, String> {
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建英语学习目录失败：{error}"))?;
        }
        let connection = Connection::open(&self.database_path)
            .map_err(|error| format!("打开英语学习数据库失败：{error}"))?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| format!("启用英语学习数据库外键失败：{error}"))?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| format!("设置英语学习数据库等待时间失败：{error}"))?;
        migrate(&connection)?;
        Ok(connection)
    }
}

fn migrate(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| format!("读取英语学习数据库版本失败：{error}"))?;
    if version > LEARNING_SCHEMA_VERSION {
        return Err("英语学习数据库版本高于当前应用支持的版本。".to_string());
    }
    if version == 0 {
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE english_learning_items (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK (kind IN ('word', 'phrase', 'sentence', 'media_segment')),
                source_kind TEXT NOT NULL,
                source_ref TEXT NOT NULL,
                source_version TEXT NOT NULL DEFAULT '',
                entry_key TEXT NOT NULL,
                canonical_text TEXT NOT NULL,
                normalized_text TEXT NOT NULL,
                translation TEXT NOT NULL DEFAULT '',
                snapshot_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(source_kind, entry_key)
             );
             CREATE TABLE english_books (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_ref TEXT NOT NULL,
                item_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
             );
             CREATE TABLE english_book_items (
                book_id TEXT NOT NULL REFERENCES english_books(id) ON DELETE CASCADE,
                item_id TEXT NOT NULL REFERENCES english_learning_items(id) ON DELETE CASCADE,
                position INTEGER NOT NULL,
                PRIMARY KEY(book_id, item_id)
             );
             CREATE TABLE english_plans (
                id TEXT PRIMARY KEY,
                book_id TEXT NOT NULL REFERENCES english_books(id),
                status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'completed')),
                settings_json TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
             );
             CREATE UNIQUE INDEX english_one_active_plan
                ON english_plans(status) WHERE status = 'active';
             CREATE TABLE english_item_progress (
                id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL REFERENCES english_plans(id) ON DELETE CASCADE,
                item_id TEXT NOT NULL REFERENCES english_learning_items(id) ON DELETE CASCADE,
                state TEXT NOT NULL CHECK (state IN ('new', 'learning', 'review', 'relearning', 'mastered', 'archived')),
                stability REAL NOT NULL DEFAULT 0,
                difficulty REAL NOT NULL DEFAULT 0,
                due_at INTEGER,
                last_reviewed_at INTEGER,
                scheduled_days INTEGER NOT NULL DEFAULT 0,
                reps INTEGER NOT NULL DEFAULT 0,
                lapses INTEGER NOT NULL DEFAULT 0,
                introduced_at INTEGER,
                mastered_at INTEGER,
                audit_due_at INTEGER,
                audit_step INTEGER NOT NULL DEFAULT 0,
                archived_from_state TEXT,
                card_json TEXT NOT NULL,
                scheduler_version TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(plan_id, item_id)
             );
             CREATE TABLE english_skill_stats (
                progress_id TEXT NOT NULL REFERENCES english_item_progress(id) ON DELETE CASCADE,
                skill_kind TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                correct INTEGER NOT NULL DEFAULT 0,
                hint_uses INTEGER NOT NULL DEFAULT 0,
                average_response_ms INTEGER NOT NULL DEFAULT 0,
                last_attempt_at INTEGER,
                last_error_at INTEGER,
                PRIMARY KEY(progress_id, skill_kind)
             );
             CREATE TABLE english_attempts (
                id TEXT PRIMARY KEY,
                progress_id TEXT NOT NULL REFERENCES english_item_progress(id) ON DELETE CASCADE,
                exercise_kind TEXT NOT NULL,
                raw_answer TEXT NOT NULL,
                normalized_answer TEXT NOT NULL,
                verdict TEXT NOT NULL CHECK (verdict IN ('correct', 'acceptable', 'incorrect', 'skipped')),
                suggested_rating TEXT NOT NULL,
                final_rating TEXT NOT NULL,
                hint_level INTEGER NOT NULL DEFAULT 0,
                hint_count INTEGER NOT NULL DEFAULT 0,
                response_ms INTEGER NOT NULL DEFAULT 0,
                previous_state_json TEXT NOT NULL,
                next_state_json TEXT NOT NULL,
                was_new INTEGER NOT NULL DEFAULT 0,
                reviewed_at INTEGER NOT NULL,
                scheduler_version TEXT NOT NULL
             );
             CREATE INDEX english_progress_due ON english_item_progress(plan_id, state, due_at);
             CREATE INDEX english_progress_audit ON english_item_progress(plan_id, state, audit_due_at);
             CREATE INDEX english_attempts_plan_time ON english_attempts(progress_id, reviewed_at DESC);
             CREATE INDEX english_attempts_time ON english_attempts(reviewed_at DESC);
             PRAGMA user_version = 3;
             COMMIT;"
        ).map_err(|error| format!("创建英语学习数据库失败：{error}"))?;
        return Ok(());
    }
    if version < 2 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE english_item_progress ADD COLUMN archived_from_state TEXT;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )
            .map_err(|error| format!("升级英语学习数据库到版本 2 失败：{error}"))?;
    }
    if version < 3 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 DELETE FROM english_attempts
                 WHERE rowid IN (
                    SELECT rowid FROM english_attempts
                    ORDER BY reviewed_at DESC, rowid DESC
                    LIMIT -1 OFFSET 1000
                 );
                 CREATE INDEX english_attempts_time ON english_attempts(reviewed_at DESC);
                 PRAGMA user_version = 3;
                 COMMIT;",
            )
            .map_err(|error| format!("升级英语学习数据库到版本 3 失败：{error}"))?;
    }
    Ok(())
}

fn prune_attempt_history(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM english_attempts
             WHERE rowid IN (
                SELECT rowid FROM english_attempts
                ORDER BY reviewed_at DESC, rowid DESC
                LIMIT -1 OFFSET ?
             )",
            params![MAX_ATTEMPT_HISTORY as i64],
        )
        .map_err(|error| format!("清理旧英语答题记录失败：{error}"))?;
    Ok(())
}

fn active_plan_with_connection(
    connection: &Connection,
) -> Result<Option<EnglishPlanSummary>, String> {
    connection
        .query_row(
            "SELECT p.id, p.book_id, b.name, p.status, b.item_count, p.settings_json,
                p.started_at, p.updated_at
         FROM english_plans p JOIN english_books b ON b.id = p.book_id
         WHERE p.status = 'active' LIMIT 1",
            [],
            plan_from_row,
        )
        .optional()
        .map_err(|error| format!("读取当前英语学习计划失败：{error}"))?
        .transpose()
}

fn plan_by_id(
    connection: &Connection,
    plan_id: &str,
) -> Result<Option<EnglishPlanSummary>, String> {
    connection
        .query_row(
            "SELECT p.id, p.book_id, b.name, p.status, b.item_count, p.settings_json,
                p.started_at, p.updated_at
         FROM english_plans p JOIN english_books b ON b.id = p.book_id WHERE p.id = ?",
            params![plan_id],
            plan_from_row,
        )
        .optional()
        .map_err(|error| format!("读取英语学习计划失败：{error}"))?
        .transpose()
}

fn plan_from_row(row: &Row<'_>) -> rusqlite::Result<Result<EnglishPlanSummary, String>> {
    let settings_json: String = row.get(5)?;
    Ok(serde_json::from_str::<EnglishPlanSettings>(&settings_json)
        .map_err(|error| format!("解析英语学习计划设置失败：{error}"))
        .map(|settings| EnglishPlanSummary {
            id: row.get(0).unwrap_or_default(),
            book_id: row.get(1).unwrap_or_default(),
            book_name: row.get(2).unwrap_or_default(),
            status: row.get(3).unwrap_or_default(),
            item_count: i64_to_usize(row.get(4).unwrap_or(0)),
            settings,
            started_at: i64_to_u64(row.get(6).unwrap_or(0)),
            updated_at: i64_to_u64(row.get(7).unwrap_or(0)),
        }))
}

fn overview_with_connection(
    connection: &Connection,
    now: i64,
) -> Result<EnglishLearningOverview, String> {
    let Some(plan) = active_plan_with_connection(connection)? else {
        return Ok(EnglishLearningOverview::default());
    };
    let day_start = day_start_millis(now);
    let counts: (i64, i64, i64, i64, i64, i64, i64) = connection.query_row(
        "SELECT
            SUM(CASE WHEN state IN ('learning', 'review', 'relearning') AND due_at <= ? THEN 1 ELSE 0 END),
            SUM(CASE WHEN state IN ('learning', 'review', 'relearning') AND due_at < ? THEN 1 ELSE 0 END),
            SUM(CASE WHEN state = 'mastered' AND audit_due_at <= ? THEN 1 ELSE 0 END),
            SUM(CASE WHEN state = 'new' THEN 1 ELSE 0 END),
            SUM(CASE WHEN state != 'new' AND state != 'archived' THEN 1 ELSE 0 END),
            SUM(CASE WHEN state = 'mastered' THEN 1 ELSE 0 END),
            SUM(CASE WHEN state = 'archived' THEN 1 ELSE 0 END)
         FROM english_item_progress WHERE plan_id = ?",
        params![now, day_start, now, plan.id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
    ).map_err(|error| format!("读取英语今日概览失败：{error}"))?;
    let today_new_done = count_today_new(connection, &plan.id, day_start)?;
    let today_review_done: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM english_attempts a
         JOIN english_item_progress p ON p.id = a.progress_id
         WHERE p.plan_id = ? AND a.reviewed_at >= ? AND a.was_new = 0",
            params![plan.id, day_start],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取今日复习进度失败：{error}"))?;
    let weak_skill = connection
        .query_row(
            "SELECT s.skill_kind FROM english_skill_stats s
         JOIN english_item_progress p ON p.id = s.progress_id
         WHERE p.plan_id = ? AND s.attempts > 0
         GROUP BY s.skill_kind
         ORDER BY CAST(SUM(s.correct) AS REAL) / SUM(s.attempts) ASC, SUM(s.attempts) DESC
         LIMIT 1",
            params![plan.id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("读取英语薄弱技能失败：{error}"))?;
    let new_available = i64_to_usize(counts.3);
    let is_rest_day = is_rest_day(now, &plan.settings.rest_days);
    let estimated_completion_at = if new_available == 0 {
        Some(now.max(0) as u64)
    } else if plan.settings.pause_new_words {
        None
    } else {
        Some(estimate_completion_at(
            now,
            new_available,
            i64_to_usize(today_new_done),
            &plan.settings,
        ))
    };
    Ok(EnglishLearningOverview {
        active_plan: Some(plan),
        due_count: i64_to_usize(counts.0),
        overdue_count: i64_to_usize(counts.1),
        mastered_due_count: i64_to_usize(counts.2),
        new_available,
        today_new_done: i64_to_usize(today_new_done),
        today_review_done: i64_to_usize(today_review_done),
        learned_count: i64_to_usize(counts.4),
        mastered_count: i64_to_usize(counts.5),
        archived_count: i64_to_usize(counts.6),
        weak_skill,
        estimated_completion_at,
        is_rest_day,
    })
}

fn count_today_new(connection: &Connection, plan_id: &str, day_start: i64) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM english_item_progress WHERE plan_id = ? AND introduced_at >= ?",
            params![plan_id, day_start],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取今日新词进度失败：{error}"))
}

fn activity_dates(connection: &Connection, plan_id: &str) -> Result<HashSet<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT date(a.reviewed_at / 1000, 'unixepoch', 'localtime')
             FROM english_attempts a
             JOIN english_item_progress p ON p.id = a.progress_id
             WHERE p.plan_id = ? ORDER BY a.reviewed_at DESC LIMIT 400",
        )
        .map_err(|error| format!("准备英语学习日期统计失败：{error}"))?;
    let rows = statement
        .query_map(params![plan_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("读取英语学习日期失败：{error}"))?;
    let mut dates = HashSet::new();
    for row in rows {
        dates.insert(row.map_err(|error| format!("读取英语学习日期失败：{error}"))?);
    }
    Ok(dates)
}

fn activity_summary(now: i64, dates: &HashSet<String>, rest_days: &[u8]) -> (usize, usize) {
    let Some(today) = Local
        .timestamp_millis_opt(now)
        .single()
        .map(|time| time.date_naive())
    else {
        return (0, 0);
    };
    let active_days_7d = (0..7)
        .filter(|offset| dates.contains(&(today - Duration::days(*offset)).to_string()))
        .count();
    let mut current_streak_days = 0;
    let mut day = today;
    for offset in 0..400 {
        if dates.contains(&day.to_string()) {
            current_streak_days += 1;
        } else if is_rest_date(day, rest_days) || offset == 0 {
        } else {
            break;
        }
        day -= Duration::days(1);
    }
    (active_days_7d, current_streak_days)
}

fn query_due(
    connection: &Connection,
    plan_id: &str,
    now: i64,
    limit: usize,
) -> Result<Vec<QueueRecord>, String> {
    query_queue(
        connection,
        "SELECT p.id, p.item_id, p.state, p.due_at, p.card_json, i.snapshot_json
         FROM english_item_progress p JOIN english_learning_items i ON i.id = p.item_id
         JOIN english_book_items bi ON bi.item_id = p.item_id
         JOIN english_plans plan ON plan.book_id = bi.book_id AND plan.id = p.plan_id
         WHERE p.plan_id = ? AND p.state IN ('learning', 'review', 'relearning') AND p.due_at <= ?
         ORDER BY p.due_at ASC LIMIT ?",
        params![plan_id, now, limit as i64],
    )
}

fn query_new(
    connection: &Connection,
    plan_id: &str,
    limit: usize,
) -> Result<Vec<QueueRecord>, String> {
    query_queue(
        connection,
        "SELECT p.id, p.item_id, p.state, p.due_at, p.card_json, i.snapshot_json
         FROM english_item_progress p JOIN english_learning_items i ON i.id = p.item_id
         JOIN english_book_items bi ON bi.item_id = p.item_id
         JOIN english_plans plan ON plan.book_id = bi.book_id AND plan.id = p.plan_id
         WHERE p.plan_id = ? AND p.state = 'new'
         ORDER BY bi.position ASC LIMIT ?",
        params![plan_id, limit as i64],
    )
}

fn query_mistakes(
    connection: &Connection,
    plan_id: &str,
    limit: usize,
) -> Result<Vec<QueueRecord>, String> {
    query_queue(
        connection,
        "SELECT p.id, p.item_id, p.state, p.due_at, p.card_json, i.snapshot_json
         FROM english_item_progress p JOIN english_learning_items i ON i.id = p.item_id
         JOIN english_skill_stats s ON s.progress_id = p.id
         WHERE p.plan_id = ? AND p.state NOT IN ('new', 'archived') AND s.attempts > s.correct
         GROUP BY p.id ORDER BY SUM(s.attempts - s.correct) DESC, MAX(s.last_error_at) DESC LIMIT ?",
        params![plan_id, limit as i64],
    )
}

fn query_mastered(
    connection: &Connection,
    plan_id: &str,
    now: i64,
    limit: usize,
) -> Result<Vec<QueueRecord>, String> {
    query_queue(
        connection,
        "SELECT p.id, p.item_id, p.state, p.due_at, p.card_json, i.snapshot_json
         FROM english_item_progress p JOIN english_learning_items i ON i.id = p.item_id
         WHERE p.plan_id = ? AND p.state = 'mastered' AND p.audit_due_at <= ?
         ORDER BY p.audit_due_at ASC LIMIT ?",
        params![plan_id, now, limit as i64],
    )
}

fn query_progress(
    connection: &Connection,
    progress_id: &str,
) -> Result<Option<QueueRecord>, String> {
    connection
        .query_row(
            "SELECT p.id, p.item_id, p.state, p.due_at, p.card_json, i.snapshot_json
         FROM english_item_progress p JOIN english_learning_items i ON i.id = p.item_id
         WHERE p.id = ?",
            params![progress_id],
            queue_record_from_row,
        )
        .optional()
        .map_err(|error| format!("读取英语学习项失败：{error}"))?
        .transpose()
}

fn query_queue<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<QueueRecord>, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("准备英语学习队列失败：{error}"))?;
    let rows = statement
        .query_map(params, queue_record_from_row)
        .map_err(|error| format!("查询英语学习队列失败：{error}"))?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|error| format!("读取英语学习队列失败：{error}"))??);
    }
    Ok(result)
}

fn queue_record_from_row(row: &Row<'_>) -> rusqlite::Result<Result<QueueRecord, String>> {
    let snapshot_json: String = row.get(5)?;
    Ok(
        serde_json::from_str::<EnglishLearningSnapshot>(&snapshot_json)
            .map_err(|error| format!("解析英语单词快照失败：{error}"))
            .map(|snapshot| QueueRecord {
                progress_id: row.get(0).unwrap_or_default(),
                item_id: row.get(1).unwrap_or_default(),
                state: row.get(2).unwrap_or_default(),
                due_at: row.get(3).unwrap_or(None),
                card_json: row.get(4).unwrap_or_default(),
                snapshot,
            }),
    )
}

fn queue_item(
    connection: &Connection,
    record: QueueRecord,
    mode: EnglishQueueMode,
    retention: f64,
    now: i64,
) -> Result<EnglishQueueItem, String> {
    let exercise_kind = choose_exercise(connection, &record, mode)?;
    let card = parse_card(&record.card_json, now);
    Ok(EnglishQueueItem {
        progress_id: record.progress_id,
        item_id: record.item_id,
        state: record.state,
        exercise_kind,
        snapshot: record.snapshot,
        due_at: record.due_at.map(i64_to_u64),
        rating_previews: scheduler::previews(&card, retention, now),
    })
}

fn choose_exercise(
    _connection: &Connection,
    record: &QueueRecord,
    mode: EnglishQueueMode,
) -> Result<EnglishExerciseKind, String> {
    if matches!(mode, EnglishQueueMode::Dictation) && has_audio(&record.snapshot) {
        return Ok(EnglishExerciseKind::Dictation);
    }
    if matches!(mode, EnglishQueueMode::Spelling) {
        return Ok(EnglishExerciseKind::Spelling);
    }
    Ok(EnglishExerciseKind::Spelling)
}

fn has_audio(snapshot: &EnglishLearningSnapshot) -> bool {
    !snapshot.british_audio.is_empty() || !snapshot.american_audio.is_empty()
}

fn attempt_progress(
    transaction: &Transaction<'_>,
    progress_id: &str,
) -> Result<Option<AttemptProgress>, String> {
    transaction
        .query_row(
            "SELECT p.id, p.state, p.audit_step, p.card_json,
                plan.settings_json, i.snapshot_json
         FROM english_item_progress p JOIN english_plans plan ON plan.id = p.plan_id
         JOIN english_learning_items i ON i.id = p.item_id WHERE p.id = ?",
            params![progress_id],
            |row| {
                let settings_json: String = row.get(4)?;
                let snapshot_json: String = row.get(5)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    settings_json,
                    snapshot_json,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取答题进度失败：{error}"))?
        .map(
            |(id, state, audit_step, card_json, settings_json, snapshot_json)| {
                let settings: EnglishPlanSettings = serde_json::from_str(&settings_json)
                    .map_err(|error| format!("解析英语计划设置失败：{error}"))?;
                let snapshot = serde_json::from_str(&snapshot_json)
                    .map_err(|error| format!("解析英语单词快照失败：{error}"))?;
                Ok(AttemptProgress {
                    progress_id: id,
                    state,
                    audit_step,
                    card_json,
                    retention: settings.desired_retention,
                    snapshot,
                })
            },
        )
        .transpose()
}

fn duplicate_attempt_result(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Option<EnglishAttemptResult>, String> {
    let stored = connection.query_row(
        "SELECT a.verdict, a.suggested_rating, a.final_rating, p.due_at, p.scheduled_days, p.state
         FROM english_attempts a JOIN english_item_progress p ON p.id = a.progress_id WHERE a.id = ?",
        params![attempt_id], |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            row.get::<_, Option<i64>>(3)?, row.get::<_, i64>(4)?, row.get::<_, String>(5)?,
        )),
    ).optional().map_err(|error| format!("检查重复英语答题记录失败：{error}"))?;
    stored
        .map(
            |(verdict, suggested, final_rating, due_at, scheduled_days, state)| {
                Ok(EnglishAttemptResult {
                    attempt_id: attempt_id.to_string(),
                    duplicate: true,
                    verdict: parse_verdict(&verdict)?,
                    suggested_rating: parse_rating(&suggested)?,
                    final_rating: parse_rating(&final_rating)?,
                    next_due_at: i64_to_u64(due_at.unwrap_or(0)),
                    scheduled_days,
                    state,
                    overview: overview_with_connection(connection, now_millis())?,
                })
            },
        )
        .transpose()
}

fn parse_card(value: &str, now: i64) -> Card {
    serde_json::from_str(value).unwrap_or_else(|_| scheduler::new_card(now))
}

fn parse_rating(value: &str) -> Result<EnglishRating, String> {
    match value {
        "again" => Ok(EnglishRating::Again),
        "hard" => Ok(EnglishRating::Hard),
        "good" => Ok(EnglishRating::Good),
        "easy" => Ok(EnglishRating::Easy),
        _ => Err("英语评级记录无效。".to_string()),
    }
}

fn parse_verdict(value: &str) -> Result<EnglishVerdict, String> {
    match value {
        "correct" => Ok(EnglishVerdict::Correct),
        "acceptable" => Ok(EnglishVerdict::Acceptable),
        "incorrect" => Ok(EnglishVerdict::Incorrect),
        "skipped" => Ok(EnglishVerdict::Skipped),
        _ => Err("英语判题记录无效。".to_string()),
    }
}

fn parse_exercise(value: &str) -> Result<EnglishExerciseKind, String> {
    match value {
        "meaning_recall" => Ok(EnglishExerciseKind::MeaningRecall),
        "spelling" => Ok(EnglishExerciseKind::Spelling),
        "dictation" => Ok(EnglishExerciseKind::Dictation),
        _ => Err("英语题型记录无效。".to_string()),
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn day_start_millis(now: i64) -> i64 {
    Local
        .timestamp_millis_opt(now)
        .single()
        .and_then(|time| time.date_naive().and_hms_opt(0, 0, 0))
        .and_then(|time| time.and_local_timezone(Local).single())
        .map(|time| time.timestamp_millis())
        .unwrap_or_else(|| now - now.rem_euclid(DAY_MS))
}

fn is_rest_day(timestamp: i64, rest_days: &[u8]) -> bool {
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|time| is_rest_date(time.date_naive(), rest_days))
        .unwrap_or(false)
}

fn is_rest_date(day: chrono::NaiveDate, rest_days: &[u8]) -> bool {
    rest_days.contains(&(day.weekday().num_days_from_sunday() as u8))
}

fn estimate_completion_at(
    now: i64,
    new_available: usize,
    today_new_done: usize,
    settings: &EnglishPlanSettings,
) -> u64 {
    let daily_target = settings.daily_new_target.max(1) as usize;
    let mut remaining = new_available;
    let cursor = Local
        .timestamp_millis_opt(day_start_millis(now))
        .single()
        .map(|time| time.date_naive());
    let Some(mut day) = cursor else {
        return now.max(0) as u64;
    };
    if !is_rest_date(day, &settings.rest_days) {
        remaining = remaining.saturating_sub(daily_target.saturating_sub(today_new_done));
    }
    while remaining > 0 {
        day += Duration::days(1);
        if !is_rest_date(day, &settings.rest_days) {
            remaining = remaining.saturating_sub(daily_target);
        }
    }
    day.and_hms_opt(12, 0, 0)
        .and_then(|time| time.and_local_timezone(Local).single())
        .map(|time| time.timestamp_millis().max(0) as u64)
        .unwrap_or(now.max(0) as u64)
}

fn audio_extension(url: &str) -> &'static str {
    let path = url.split('?').next().unwrap_or(url).to_lowercase();
    if path.ends_with(".wav") {
        "wav"
    } else if path.ends_with(".m4a") {
        "m4a"
    } else if path.ends_with(".ogg") {
        "ogg"
    } else {
        "mp3"
    }
}

fn cache_files(path: &PathBuf) -> Result<Vec<(PathBuf, u64, SystemTime)>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(path).map_err(|error| format!("读取英语音频缓存失败：{error}"))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取英语音频缓存项失败：{error}"))?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取英语音频缓存信息失败：{error}"))?;
        if metadata.is_file() {
            files.push((
                entry.path(),
                metadata.len(),
                metadata.modified().unwrap_or(UNIX_EPOCH),
            ));
        }
    }
    Ok(files)
}

fn cache_usage(path: &PathBuf) -> Result<(u64, usize), String> {
    let files = cache_files(path)?;
    Ok((files.iter().map(|item| item.1).sum(), files.len()))
}

fn i64_to_usize(value: i64) -> usize {
    usize::try_from(value).unwrap_or(0)
}
fn i64_to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> EnglishLearningRepository {
        let root = std::env::temp_dir().join(format!("mnemora-learning-{}", Uuid::new_v4()));
        EnglishLearningRepository::new(root)
    }

    fn entry(id: u32, word: &str) -> EnglishWordEntry {
        EnglishWordEntry {
            id,
            entry_key: format!("test:{word}"),
            source_version: "test-v1".to_string(),
            word: word.to_string(),
            group_id: 0,
            group_name: "Test".to_string(),
            pronunciation: "test".to_string(),
            translation: "测试".to_string(),
            example: String::new(),
            example_translation: String::new(),
            british_audio: String::new(),
            american_audio: String::new(),
            mnemonic: String::new(),
            root_affixes: String::new(),
            english_definition: String::new(),
            derived_words: Vec::new(),
            occurrence: None,
            exam_examples: Vec::new(),
        }
    }

    #[test]
    fn creates_plan_and_keeps_attempt_submission_idempotent() {
        let repository = repository();
        repository
            .create_plan(
                EnglishCreatePlanInput {
                    name: "Test book".to_string(),
                    group_ids: vec![0],
                    settings: EnglishPlanSettings::default(),
                },
                vec![EnglishLearningSnapshot::from(&entry(1, "test"))],
            )
            .unwrap();
        let batch = repository
            .next_batch(EnglishNextBatchInput {
                mode: EnglishQueueMode::New,
            })
            .unwrap();
        assert_eq!(batch.len(), 1);
        let input = EnglishSubmitAttemptInput {
            attempt_id: Uuid::new_v4().to_string(),
            progress_id: batch[0].progress_id.clone(),
            exercise_kind: EnglishExerciseKind::MeaningRecall,
            raw_answer: "测试".to_string(),
            hint_level: 0,
            hint_count: 0,
            response_ms: 3000,
            final_rating: EnglishRating::Good,
        };
        let first = repository.submit_attempt(input.clone()).unwrap();
        let duplicate = repository.submit_attempt(input).unwrap();
        assert!(!first.duplicate);
        assert!(duplicate.duplicate);
        assert_eq!(duplicate.overview.today_new_done, 1);
        let stats = repository.stats().unwrap();
        assert_eq!(stats.attempts_7d, 1);
        assert_eq!(stats.average_response_ms_7d, 3000);
        let overview = repository.add_word(&entry(2, "added")).unwrap();
        assert_eq!(overview.active_plan.unwrap().item_count, 2);
        assert_eq!(overview.new_available, 1);
    }

    #[test]
    fn attempt_history_is_pruned_to_the_latest_thousand_records() {
        let repository = repository();
        repository
            .create_plan(
                EnglishCreatePlanInput {
                    name: "History retention test".to_string(),
                    group_ids: vec![0],
                    settings: EnglishPlanSettings::default(),
                },
                vec![EnglishLearningSnapshot::from(&entry(1, "history"))],
            )
            .unwrap();
        let progress_id = repository
            .next_batch(EnglishNextBatchInput {
                mode: EnglishQueueMode::New,
            })
            .unwrap()[0]
            .progress_id
            .clone();
        let mut connection = repository.open_connection().unwrap();
        let card_json: String = connection
            .query_row(
                "SELECT card_json FROM english_item_progress WHERE id = ?",
                params![progress_id],
                |row| row.get(0),
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "WITH RECURSIVE sequence(value) AS (
                    SELECT 0 UNION ALL SELECT value + 1 FROM sequence WHERE value < 1004
                 )
                 INSERT INTO english_attempts (
                    id, progress_id, exercise_kind, raw_answer, normalized_answer, verdict,
                    suggested_rating, final_rating, hint_level, hint_count, response_ms,
                    previous_state_json, next_state_json, was_new, reviewed_at, scheduler_version
                 )
                 SELECT printf('attempt-%d', value), ?, 'spelling', '', '', 'skipped',
                    'again', 'again', 0, 0, 0, '{}', ?, 0, value, 'test'
                 FROM sequence",
                params![progress_id, card_json],
            )
            .unwrap();
        prune_attempt_history(&transaction).unwrap();
        transaction.commit().unwrap();

        let (count, oldest): (i64, i64) = connection
            .query_row(
                "SELECT COUNT(*), MIN(reviewed_at) FROM english_attempts",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, MAX_ATTEMPT_HISTORY as i64);
        assert_eq!(oldest, 5);
        drop(connection);

        let first_page = repository.list_history(20, 0).unwrap();
        let second_page = repository.list_history(20, 20).unwrap();
        assert_eq!(first_page.total, MAX_ATTEMPT_HISTORY);
        assert_eq!(second_page.total, MAX_ATTEMPT_HISTORY);
        assert_eq!(first_page.items.len(), 20);
        assert_eq!(second_page.items.len(), 20);
        assert_ne!(first_page.items[0].id, second_page.items[0].id);
    }

    #[test]
    fn page_sizes_are_independent_and_accept_the_legacy_dictionary_setting() {
        let legacy: EnglishPlanSettings =
            serde_json::from_value(serde_json::json!({ "pageSize": 40 })).unwrap();
        assert_eq!(legacy.dictionary_page_size, 40);
        assert_eq!(legacy.archive_page_size, 20);
        assert_eq!(legacy.history_page_size, 20);

        let settings = EnglishPlanSettings {
            archive_page_size: 40,
            history_page_size: 40,
            ..EnglishPlanSettings::default()
        }
        .validate()
        .unwrap();
        assert_eq!(settings.dictionary_page_size, 20);
        assert_eq!(settings.archive_page_size, 40);
        assert_eq!(settings.history_page_size, 40);

        assert!(EnglishPlanSettings {
            history_page_size: 30,
            ..EnglishPlanSettings::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn portable_books_round_trip_into_an_active_custom_plan() {
        let repository = repository();
        repository
            .create_plan(
                EnglishCreatePlanInput {
                    name: "Exported book".to_string(),
                    group_ids: vec![0],
                    settings: EnglishPlanSettings::default(),
                },
                vec![EnglishLearningSnapshot::from(&entry(1, "portable"))],
            )
            .unwrap();
        let exported = repository.export_active_book().unwrap();
        let portable: EnglishPortableBook = serde_json::from_str(&exported).unwrap();
        assert_eq!(portable.format, PORTABLE_BOOK_FORMAT);
        assert_eq!(portable.entries[0].word, "portable");

        let imported = repository.import_portable_book(&exported).unwrap();
        assert_eq!(imported.book_name, "Exported book");
        assert_eq!(imported.item_count, 1);
        assert_eq!(repository.overview().unwrap().new_available, 1);
    }

    #[test]
    fn rest_days_are_skipped_in_completion_estimates() {
        let now = Local
            .with_ymd_and_hms(2026, 8, 4, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let settings = EnglishPlanSettings {
            daily_new_target: 10,
            rest_days: vec![2],
            ..EnglishPlanSettings::default()
        };
        let completion = estimate_completion_at(now, 10, 0, &settings);
        let completion_date = Local
            .timestamp_millis_opt(completion as i64)
            .single()
            .unwrap()
            .date_naive();
        assert_eq!(completion_date.to_string(), "2026-08-05");
    }

    #[test]
    fn configured_rest_days_do_not_break_the_factual_streak() {
        let now = Local
            .with_ymd_and_hms(2026, 8, 4, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let dates = HashSet::from(["2026-08-01".to_string(), "2026-08-03".to_string()]);
        let (active_days, streak) = activity_summary(now, &dates, &[0]);
        assert_eq!(active_days, 2);
        assert_eq!(streak, 2);
    }

    #[test]
    fn audio_cache_can_be_measured_and_cleared() {
        let repository = repository();
        repository
            .store_cached_audio("https://example.com/test.mp3", b"abc", 1024)
            .unwrap();
        let status = repository.audio_cache_status().unwrap();
        assert_eq!(status.files, 1);
        assert_eq!(status.bytes, 3);
        let cleared = repository.clear_audio_cache().unwrap();
        assert_eq!(cleared.files, 0);
        assert_eq!(cleared.bytes, 0);
    }

    #[test]
    fn archived_new_word_is_listed_and_restored_as_new() {
        let repository = repository();
        repository
            .create_plan(
                EnglishCreatePlanInput {
                    name: "Archive test".to_string(),
                    group_ids: vec![0],
                    settings: EnglishPlanSettings::default(),
                },
                vec![EnglishLearningSnapshot::from(&entry(1, "recover"))],
            )
            .unwrap();
        let batch = repository
            .next_batch(EnglishNextBatchInput {
                mode: EnglishQueueMode::New,
            })
            .unwrap();
        assert_eq!(batch[0].exercise_kind, EnglishExerciseKind::Spelling);

        repository.archive_item(&batch[0].progress_id).unwrap();
        let archived = repository.list_archived(20, 0).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].word, "recover");
        assert_eq!(archived[0].previous_state, "new");

        repository.restore_item(&batch[0].progress_id).unwrap();
        assert!(repository.list_archived(20, 0).unwrap().is_empty());
        let restored = repository
            .next_batch(EnglishNextBatchInput {
                mode: EnglishQueueMode::New,
            })
            .unwrap();
        assert_eq!(restored[0].state, "new");
    }

    #[test]
    fn archived_words_are_loaded_in_bounded_pages() {
        let repository = repository();
        repository
            .create_plan(
                EnglishCreatePlanInput {
                    name: "Archive pagination test".to_string(),
                    group_ids: vec![0],
                    settings: EnglishPlanSettings::default(),
                },
                ["one", "two", "three"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, word)| {
                        EnglishLearningSnapshot::from(&entry(index as u32 + 1, word))
                    })
                    .collect(),
            )
            .unwrap();
        let batch = repository
            .next_batch(EnglishNextBatchInput {
                mode: EnglishQueueMode::New,
            })
            .unwrap();
        for item in &batch {
            repository.archive_item(&item.progress_id).unwrap();
        }

        let first_page = repository.list_archived(2, 0).unwrap();
        let second_page = repository.list_archived(2, 2).unwrap();
        assert_eq!(first_page.len(), 2);
        assert_eq!(second_page.len(), 1);
        let progress_ids = first_page
            .iter()
            .chain(second_page.iter())
            .map(|item| item.progress_id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(progress_ids.len(), 3);
    }

    #[test]
    fn archived_mastered_word_keeps_its_audit_schedule_when_restored() {
        let repository = repository();
        repository
            .create_plan(
                EnglishCreatePlanInput {
                    name: "Mastered archive test".to_string(),
                    group_ids: vec![0],
                    settings: EnglishPlanSettings::default(),
                },
                vec![EnglishLearningSnapshot::from(&entry(1, "retain"))],
            )
            .unwrap();
        let batch = repository
            .next_batch(EnglishNextBatchInput {
                mode: EnglishQueueMode::New,
            })
            .unwrap();
        let progress_id = &batch[0].progress_id;

        repository.mark_mastered(progress_id).unwrap();
        let connection = repository.open_connection().unwrap();
        let audit_due_before: i64 = connection
            .query_row(
                "SELECT audit_due_at FROM english_item_progress WHERE id = ?",
                params![progress_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);

        repository.archive_item(progress_id).unwrap();
        assert_eq!(
            repository.list_archived(20, 0).unwrap()[0].previous_state,
            "mastered"
        );
        repository.restore_item(progress_id).unwrap();

        let connection = repository.open_connection().unwrap();
        let (state, audit_due_after): (String, i64) = connection
            .query_row(
                "SELECT state, audit_due_at FROM english_item_progress WHERE id = ?",
                params![progress_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "mastered");
        assert_eq!(audit_due_after, audit_due_before);
    }

    #[test]
    fn migrates_version_one_archive_state_column() {
        let root = std::env::temp_dir().join(format!("mnemora-learning-v1-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("learning.sqlite3");
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE english_item_progress (id TEXT PRIMARY KEY);
                 CREATE TABLE english_attempts (reviewed_at INTEGER);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        migrate(&connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        let has_column: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('english_item_progress')
                 WHERE name = 'archived_from_state')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has_column);
    }
}
