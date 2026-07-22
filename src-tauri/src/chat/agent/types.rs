//! Agent 工具执行和前端轨迹的数据合同。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolRisk {
    BuiltinRead,
    ConversationRead,
    MemoryRead,
    MemoryWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolTraceStatus {
    AwaitingApproval,
    Running,
    Completed,
    Rejected,
    Failed,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecution {
    pub content: String,
    pub preview: String,
    pub is_error: bool,
    pub activated_skill_id: Option<String>,
}
