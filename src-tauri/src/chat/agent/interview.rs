//! 面试会话工具：将外部 survey MCP 的生命周期映射到本地 SQLite。

use serde_json::{json, Value};

use crate::{
    ai::error::ModelError,
    library::{interview as sessions, LibraryRepository},
};

use super::types::ToolExecution;

const MAX_PREVIEW_CHARS: usize = 2_000;
const MAX_OUTPUT_CHARS: usize = 100_000;

pub(super) fn list_available() -> Result<ToolExecution, ModelError> {
    execution(sessions::list_available())
}

pub(super) fn start(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let scenario_id = required_string(arguments, "scenarioId")?;
    let participant_id = required_string(arguments, "participantId")?;
    let session = sessions::start(
        repository,
        scenario_id,
        participant_id,
        None,
        arguments.get("metadata").cloned(),
    )
    .map_err(ModelError::invalid_configuration)?;
    let progress = sessions::progress(&session);
    execution(json!({
        "sessionId": session.id,
        "scenarioId": session.scenario_id,
        "status": session.status,
        "questions": session.questions,
        "progress": progress,
        "nextQuestion": next_question(&session),
        "guidanceForLLM": "一次只提出 nextQuestion，收到回答后调用 interview_submit_response，再继续下一问。"
    }))
}

pub(super) fn get_question(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let session = get_session(repository, arguments)?;
    execution(json!({
        "sessionId": session.id,
        "status": session.status,
        "question": next_question(&session),
        "progress": sessions::progress(&session)
    }))
}

pub(super) fn submit_response(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let session_id = required_string(arguments, "sessionId")?;
    let question_id = required_string(arguments, "questionId")?;
    let value = arguments
        .get("value")
        .cloned()
        .ok_or_else(|| ModelError::invalid_configuration("缺少工具参数 value。"))?;
    let session = sessions::submit(repository, session_id, question_id, value)
        .map_err(ModelError::invalid_configuration)?;
    execution(json!({
        "success": true,
        "sessionId": session.id,
        "questionId": question_id,
        "progress": sessions::progress(&session),
        "nextQuestion": next_question(&session),
        "guidanceForLLM": "只继续询问 nextQuestion；如果 question 为空，调用 interview_complete_session。"
    }))
}

pub(super) fn get_progress(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let session = get_session(repository, arguments)?;
    execution(
        serde_json::to_value(sessions::progress(&session)).map_err(|error| {
            ModelError::invalid_configuration(format!("序列化面试进度失败：{error}"))
        })?,
    )
}

pub(super) fn complete(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let session_id = required_string(arguments, "sessionId")?;
    let session =
        sessions::complete(repository, session_id).map_err(ModelError::invalid_configuration)?;
    execution(json!({
        "success": true,
        "sessionId": session.id,
        "status": session.status,
        "completedAt": session.completed_at,
        "progress": sessions::progress(&session),
        "summary": {"answeredQuestions": session.answers.len(), "totalQuestions": session.questions.len()}
    }))
}

pub(super) fn export(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let session_id = required_string(arguments, "sessionId")?;
    let format = arguments
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("json");
    let content = sessions::export(repository, session_id, format)
        .map_err(ModelError::invalid_configuration)?;
    let output = json!({"sessionId": session_id, "format": format, "data": content});
    let mut result = execution(output)?;
    result.output_truncated = result.output_chars > MAX_OUTPUT_CHARS;
    Ok(result)
}

pub(super) fn resume(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let session = get_session(repository, arguments)?;
    execution(json!({
        "resumed": true,
        "sessionId": session.id,
        "scenarioId": session.scenario_id,
        "status": session.status,
        "lastActivity": session.updated_at,
        "progress": sessions::progress(&session),
        "nextQuestion": next_question(&session),
        "guidanceForLLM": "恢复后继续一次一问；不要重复已经有回答的问题。"
    }))
}

fn get_session(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<sessions::InterviewSession, ModelError> {
    let id = required_string(arguments, "sessionId")?;
    sessions::get(repository, id).map_err(ModelError::invalid_configuration)
}

fn next_question(session: &sessions::InterviewSession) -> Option<Value> {
    session
        .questions
        .iter()
        .find(|question| !session.answers.contains_key(&question.id))
        .map(|question| json!({"id": question.id, "text": question.text, "required": question.required}))
}

fn execution(value: Value) -> Result<ToolExecution, ModelError> {
    let content = serde_json::to_string(&value).map_err(|error| {
        ModelError::invalid_configuration(format!("序列化面试工具结果失败：{error}"))
    })?;
    if content.chars().count() > MAX_OUTPUT_CHARS {
        return Err(ModelError::invalid_configuration(
            "面试工具输出超过 100000 个字符。",
        ));
    }
    Ok(ToolExecution {
        preview: truncate_chars(&content, MAX_PREVIEW_CHARS),
        output_chars: content.chars().count(),
        content,
        is_error: false,
        activated_skill_id: None,
        output_truncated: false,
    })
}

fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ModelError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ModelError::invalid_configuration(format!("缺少工具参数 {key}。")))
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let head = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}
