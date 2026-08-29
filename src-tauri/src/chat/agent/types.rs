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
