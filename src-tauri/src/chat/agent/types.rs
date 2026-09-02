//! Agent 工具执行和前端轨迹的数据合同。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolRisk {
    BuiltinRead,
    ConversationRead,
    NetworkRead,
    MemoryRead,
    MemoryWrite,
    NoteWrite,
    ExternalTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolTraceStatus {
    Proposed,
    AwaitingApproval,
    Approved,
    Answered,
    Queued,
    Running,
    Completed,
    Rejected,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTraceSnapshot {
    pub call_id: String,
    pub name: String,
    pub status: ToolTraceStatus,
    pub risk: ToolRisk,
    pub argument_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
}

/// 提问工具里的一个可选项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolQuestionOption {
    pub label: String,
    pub description: String,
}

/// 提问工具里的一个问题。
///
/// 「其他」不在 `options` 里：它由前端固定补一项加输入框，模型没法把它删掉——
/// 选项是模型给的猜测，用户永远得有一条不受猜测约束的出口。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolQuestion {
    pub question: String,
    pub header: String,
    pub options: Vec<ToolQuestionOption>,
    #[serde(default)]
    pub multi_select: bool,
}

/// 用户对一个问题的回答。`values` 可能包含用户自填的「其他」文本，不限于 `options`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolQuestionAnswer {
    pub header: String,
    pub values: Vec<String>,
}

/// 等待用户的两种理由，决定前端渲染哪种界面。
///
/// 审批问「危险动作放不放行」，提问问「歧义怎么定」。合成一种就没法在轨迹里
/// 区分「批准了一次删库」和「在两个方案里选了 B」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ToolInterruptKind {
    Approval,
    Question { questions: Vec<ToolQuestion> },
}

impl ToolQuestion {
    /// 能否渲染成一个可用的弹窗。
    ///
    /// 选项少于两个就不是选择题，多于四个在窄侧栏里排不下；标签空着用户看不到点什么。
    /// 「其他」由前端固定追加，所以不计入上限。
    pub fn is_renderable(&self) -> bool {
        !self.question.trim().is_empty()
            && !self.header.trim().is_empty()
            && (2..=super::catalog::ASK_USER_MAX_OPTIONS).contains(&self.options.len())
            && self
                .options
                .iter()
                .all(|option| !option.label.trim().is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecution {
    pub content: String,
    pub preview: String,
    pub is_error: bool,
    pub activated_skill_id: Option<String>,
    pub output_chars: usize,
    pub output_truncated: bool,
}

impl ToolExecution {
    pub fn error_kind(&self) -> Option<String> {
        if !self.is_error {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(&self.content)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/error/code")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| Some("toolExecution".to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolCallSnapshot {
    pub call_id: String,
    pub name: String,
    pub state: String,
    pub state_version: u32,
    pub execution_version: u32,
    pub approval_id: Option<String>,
    pub risk: String,
    pub source: serde_json::Value,
    pub catalog_revision: String,
    pub result_preview: String,
    pub error_kind: Option<String>,
    pub expires_at: Option<u64>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunSnapshot {
    pub id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub state: String,
    pub activity: String,
    pub state_version: u32,
    pub execution_version: u32,
    pub runtime_instance_id: Option<String>,
    pub model_id: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub heartbeat_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    pub finished_at: Option<u64>,
    pub tool_calls: Vec<AgentToolCallSnapshot>,
}
