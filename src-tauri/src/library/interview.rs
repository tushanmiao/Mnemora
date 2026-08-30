//! 本地面试会话存储。
//!
//! 这是对 survey-mcp-server 会话生命周期的 SQLite 适配层：问题和回答以 JSON
//! 保存在本机，工具层只暴露有界的会话操作，不启动外部 MCP 进程或联网。

use std::collections::BTreeMap;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::LibraryRepository;

const MAX_QUESTIONS: usize = 100;
const MAX_QUESTION_CHARS: usize = 4_000;
const MAX_ANSWER_CHARS: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewQuestion {
    pub id: String,
    pub text: String,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewSession {
    pub id: String,
    pub scenario_id: String,
    pub participant_id: String,
    pub status: String,
    pub questions: Vec<InterviewQuestion>,
    pub answers: BTreeMap<String, Value>,
    pub metadata: Value,
    pub created_at: u64,
    pub updated_at: u64,
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewProgress {
    pub session_id: String,
    pub status: String,
    pub total_questions: usize,
    pub answered_questions: usize,
    pub required_remaining: usize,
    pub percent_complete: u8,
    pub can_complete: bool,
}

pub fn list_available() -> Value {
    serde_json::json!({
        "scenarios": [
            {"id": "general", "title": "通用面试", "description": "按简历和岗位逐题追问。"},
            {"id": "backend", "title": "后端面试", "description": "覆盖基础、系统设计、项目深挖和工程权衡。"},
            {"id": "algorithms", "title": "算法面试", "description": "逐题编码、提示、复杂度和边界复盘。"},
            {"id": "sql", "title": "SQL / 数据分析", "description": "业务题、数据集、SQL 推导和结果验证。"},
            {"id": "llm", "title": "大模型面试", "description": "RAG、Agent、训练、评测和推理优化。"}
        ],
        "storage": "local-sqlite",
        "externalProcess": false
    })
}

pub fn start(
    repository: &LibraryRepository,
    scenario_id: &str,
    participant_id: &str,
    questions: Option<Vec<InterviewQuestion>>,
    metadata: Option<Value>,
) -> Result<InterviewSession, String> {
    ensure_schema(repository)?;
    let scenario_id = normalize_id("场景 ID", scenario_id)?;
    if !is_supported_scenario(&scenario_id) {
        return Err(
            "不支持的面试场景。请先调用 interview_list_available 查看可用场景。".to_string(),
        );
    }
    let participant_id = normalize_id("参与者 ID", participant_id)?;
    let questions = questions.unwrap_or_else(|| default_questions(&scenario_id));
    let questions = normalize_questions(questions)?;
    let metadata = metadata.unwrap_or_else(|| serde_json::json!({}));
    let metadata_json = bounded_json(&metadata, 20_000, "面试元数据")?;
    let id = format!("is_{}", Uuid::new_v4().simple());
    let now = now_millis();
    let connection = repository.open_connection()?;
    connection
        .execute(
            "INSERT INTO interview_sessions (
                id, scenario_id, participant_id, status, questions_json,
                answers_json, metadata_json, created_at, updated_at, completed_at
             ) VALUES (?, ?, ?, 'active', ?, '{}', ?, ?, ?, NULL)",
            params![
                id,
                scenario_id,
                participant_id,
                serde_json::to_string(&questions)
                    .map_err(|error| format!("序列化面试问题失败：{error}"))?,
                metadata_json,
                now,
                now,
            ],
        )
        .map_err(|error| format!("创建面试会话失败：{error}"))?;
    get(repository, &id)
}

pub fn get(repository: &LibraryRepository, session_id: &str) -> Result<InterviewSession, String> {
    ensure_schema(repository)?;
    let session_id = normalize_id("面试会话 ID", session_id)?;
    let connection = repository.open_connection()?;
    connection
        .query_row(
            "SELECT id, scenario_id, participant_id, status, questions_json,
                    answers_json, metadata_json, created_at, updated_at, completed_at
             FROM interview_sessions WHERE id = ?",
            params![session_id],
            decode_session,
        )
        .optional()
        .map_err(|error| format!("读取面试会话失败：{error}"))?
        .ok_or_else(|| "面试会话不存在。".to_string())
}

pub fn submit(
    repository: &LibraryRepository,
    session_id: &str,
    question_id: &str,
    value: Value,
) -> Result<InterviewSession, String> {
    let mut session = get(repository, session_id)?;
    if session.status != "active" {
        return Err("面试会话当前不可提交回答。".to_string());
    }
    let question_id = normalize_id("问题 ID", question_id)?;
    if !session
        .questions
        .iter()
        .any(|question| question.id == question_id)
    {
        return Err("问题不属于当前面试会话。".to_string());
    }
    if value.is_null() {
        return Err("回答不能为空。".to_string());
    }
    let serialized = bounded_json(&value, MAX_ANSWER_CHARS, "面试回答")?;
    session.answers.insert(question_id, value);
    let answers_json = serde_json::to_string(&session.answers)
        .map_err(|error| format!("序列化面试回答失败：{error}"))?;
    let now = now_millis();
    let connection = repository.open_connection()?;
    let changed = connection
        .execute(
            "UPDATE interview_sessions SET answers_json = ?, updated_at = ?
             WHERE id = ? AND status = 'active'",
            params![answers_json, now, session.id],
        )
        .map_err(|error| format!("保存面试回答失败：{error}"))?;
    if changed == 0 {
        return Err("面试会话已被其他操作结束。".to_string());
    }
    let _ = serialized;
    get(repository, &session.id)
}

pub fn progress(session: &InterviewSession) -> InterviewProgress {
    let required_remaining = session
        .questions
        .iter()
        .filter(|question| question.required && !session.answers.contains_key(&question.id))
        .count();
    let total = session.questions.len();
    let answered = session.answers.len().min(total);
    InterviewProgress {
        session_id: session.id.clone(),
        status: session.status.clone(),
        total_questions: total,
        answered_questions: answered,
        required_remaining,
        percent_complete: if total == 0 {
            100
        } else {
            ((answered * 100) / total) as u8
        },
        can_complete: required_remaining == 0,
    }
}

pub fn complete(
    repository: &LibraryRepository,
    session_id: &str,
) -> Result<InterviewSession, String> {
    let session = get(repository, session_id)?;
    let progress = progress(&session);
    if !progress.can_complete {
        return Err(format!(
            "还有 {} 个必答问题未完成。",
            progress.required_remaining
        ));
    }
    if session.status == "completed" {
        return Ok(session);
    }
    let now = now_millis();
    let connection = repository.open_connection()?;
    connection
        .execute(
            "UPDATE interview_sessions SET status = 'completed', completed_at = ?, updated_at = ?
             WHERE id = ? AND status = 'active'",
            params![now, now, session.id],
        )
        .map_err(|error| format!("完成面试会话失败：{error}"))?;
    get(repository, session_id)
}

pub fn export(
    repository: &LibraryRepository,
    session_id: &str,
    format: &str,
) -> Result<String, String> {
    let session = get(repository, session_id)?;
    let format = if format.trim().is_empty() {
        "json"
    } else {
        format.trim()
    };
    let value =
        serde_json::to_value(&session).map_err(|error| format!("序列化面试结果失败：{error}"))?;
    match format {
        "json" => bounded_json(&value, 100_000, "面试导出"),
        "markdown" => {
            let mut output = format!(
                "# 面试会话 {}\n\n- 场景：{}\n- 状态：{}\n\n",
                session.id, session.scenario_id, session.status
            );
            for question in &session.questions {
                output.push_str(&format!("## {}\n\n{}\n\n", question.id, question.text));
                if let Some(answer) = session.answers.get(&question.id) {
                    output.push_str(&format!(
                        "**回答**\n\n```json\n{}\n```\n\n",
                        serde_json::to_string_pretty(answer).unwrap_or_default()
                    ));
                } else {
                    output.push_str("**回答**：未回答\n\n");
                }
            }
            if output.chars().count() > 100_000 {
                return Err("面试导出超过 100000 个字符。".to_string());
            }
            Ok(output)
        }
        _ => Err("导出格式必须是 json 或 markdown。".to_string()),
    }
}

fn ensure_schema(repository: &LibraryRepository) -> Result<(), String> {
    let connection = repository.open_connection()?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS interview_sessions (
            id TEXT PRIMARY KEY,
            scenario_id TEXT NOT NULL,
            participant_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('active', 'completed', 'cancelled')),
            questions_json TEXT NOT NULL,
            answers_json TEXT NOT NULL DEFAULT '{}',
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            completed_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS interview_sessions_participant
            ON interview_sessions(participant_id, updated_at DESC);
        CREATE INDEX IF NOT EXISTS interview_sessions_status_updated
            ON interview_sessions(status, updated_at DESC);",
        )
        .map_err(|error| format!("创建面试会话表失败：{error}"))
}

fn decode_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<InterviewSession> {
    let questions_json: String = row.get(4)?;
    let answers_json: String = row.get(5)?;
    let metadata_json: String = row.get(6)?;
    Ok(InterviewSession {
        id: row.get(0)?,
        scenario_id: row.get(1)?,
        participant_id: row.get(2)?,
        status: row.get(3)?,
        questions: serde_json::from_str(&questions_json)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        answers: serde_json::from_str(&answers_json).map_err(|_| rusqlite::Error::InvalidQuery)?,
        metadata: serde_json::from_str(&metadata_json)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get::<_, i64>(7)?.max(0) as u64,
        updated_at: row.get::<_, i64>(8)?.max(0) as u64,
        completed_at: row
            .get::<_, Option<i64>>(9)?
            .map(|value| value.max(0) as u64),
    })
}

fn normalize_questions(
    mut questions: Vec<InterviewQuestion>,
) -> Result<Vec<InterviewQuestion>, String> {
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(format!("面试问题数量必须在 1 到 {MAX_QUESTIONS} 之间。"));
    }
    let mut ids = std::collections::HashSet::new();
    for question in &mut questions {
        question.id = normalize_id("问题 ID", &question.id)?;
        question.text = question.text.trim().to_string();
        if question.text.is_empty() || question.text.chars().count() > MAX_QUESTION_CHARS {
            return Err("面试问题文本不能为空且不能过长。".to_string());
        }
        if !ids.insert(question.id.clone()) {
            return Err("面试问题 ID 不能重复。".to_string());
        }
    }
    Ok(questions)
}

fn default_questions(scenario_id: &str) -> Vec<InterviewQuestion> {
    let topics = match scenario_id {
        "backend" => vec![
            "请介绍一个你负责的后端项目。",
            "如何处理该系统的高并发和故障？",
            "你做过哪些性能或成本权衡？",
        ],
        "algorithms" => vec![
            "请先澄清这道算法题的约束。",
            "给出算法并说明复杂度。",
            "如何验证边界情况？",
        ],
        "sql" => vec![
            "如何定义这个业务指标？",
            "请写出查询并说明索引。",
            "结果异常时如何排查？",
        ],
        "llm" => vec![
            "请介绍一个 RAG 或 Agent 项目。",
            "如何设计评测和失败分析？",
            "如何平衡质量、延迟和成本？",
        ],
        _ => vec![
            "请做一个简短的自我介绍。",
            "请介绍最能代表你的项目。",
            "你遇到过什么失败，如何复盘？",
        ],
    };
    topics
        .into_iter()
        .enumerate()
        .map(|(index, text)| InterviewQuestion {
            id: format!("q{}", index + 1),
            text: text.to_string(),
            required: true,
        })
        .collect()
}

fn is_supported_scenario(scenario_id: &str) -> bool {
    matches!(
        scenario_id,
        "general" | "backend" | "algorithms" | "sql" | "llm"
    )
}

fn normalize_id(label: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > 128
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(format!("{label}无效。"));
    }
    Ok(value.to_string())
}

fn bounded_json(value: &Value, max_chars: usize, label: &str) -> Result<String, String> {
    let serialized =
        serde_json::to_string(value).map_err(|error| format!("序列化{label}失败：{error}"))?;
    if serialized.chars().count() > max_chars {
        return Err(format!("{label}超过 {max_chars} 个字符。"));
    }
    Ok(serialized)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use serde_json::json;

    use super::*;

    fn repository(label: &str) -> (LibraryRepository, PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "mnemora-interview-{label}-{}",
            Uuid::new_v4().simple()
        ));
        let repository = LibraryRepository::new(directory.clone());
        // schema 迁移不再挂在 `open_connection` 上，必须显式初始化一次。
        repository.initialize().unwrap();
        (repository, directory)
    }

    #[test]
    fn interview_session_lifecycle_is_resumable_and_exportable() {
        let (repository, directory) = repository("lifecycle");
        let session = start(
            &repository,
            "backend",
            "candidate-1",
            None,
            Some(json!({"source": "test"})),
        )
        .unwrap();
        assert_eq!(session.status, "active");
        assert_eq!(progress(&session).answered_questions, 0);

        let resumed = get(&repository, &session.id).unwrap();
        assert_eq!(resumed.questions.len(), 3);
        let first_id = resumed.questions[0].id.clone();
        let after_first = submit(&repository, &session.id, &first_id, json!("answer-1")).unwrap();
        assert_eq!(progress(&after_first).answered_questions, 1);
        assert!(!progress(&after_first).can_complete);

        let second_id = after_first.questions[1].id.clone();
        let third_id = after_first.questions[2].id.clone();
        let after_second = submit(
            &repository,
            &session.id,
            &second_id,
            json!({"answer": "answer-2"}),
        )
        .unwrap();
        let completed = submit(&repository, &session.id, &third_id, json!("answer-3")).unwrap();
        assert!(progress(&completed).can_complete);
        assert_eq!(progress(&after_second).answered_questions, 2);

        let completed = complete(&repository, &session.id).unwrap();
        assert_eq!(completed.status, "completed");
        assert!(completed.completed_at.is_some());

        let json_export = export(&repository, &session.id, "json").unwrap();
        assert!(json_export.contains(&session.id));
        let markdown_export = export(&repository, &session.id, "markdown").unwrap();
        assert!(markdown_export.contains("# 面试会话"));

        // 重新构造 Repository，确认会话依赖 SQLite 而非进程内状态。
        // 这里刻意**不**调 `initialize()`：库已经建好了，重开连接不该再需要迁移。
        let reopened = LibraryRepository::new(directory.clone());
        assert_eq!(get(&reopened, &session.id).unwrap().status, "completed");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn interview_rejects_incomplete_completion_and_invalid_answers() {
        let (repository, directory) = repository("validation");
        assert!(start(&repository, "unknown", "candidate-2", None, None).is_err());
        let session = start(&repository, "general", "candidate-2", None, None).unwrap();
        assert!(complete(&repository, &session.id).is_err());
        assert!(submit(&repository, &session.id, "unknown", json!("answer")).is_err());
        assert!(submit(&repository, &session.id, "q1", Value::Null).is_err());
        let _ = fs::remove_dir_all(directory);
    }
}
