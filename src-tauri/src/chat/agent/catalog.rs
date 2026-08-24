//! Agent Tool 的统一静态注册表。
//!
//! 注册表只保存轻量元数据，不持有附件内容、Skill 正文或 MCP 连接。一次请求结束后，
//! ToolRuntimeContext 释放，注册表本身不会造成随会话增长的内存占用。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ai::types::ModelTool;

use super::types::ToolRisk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolNamespace {
    Discovery,
    Skill,
    Attachment,
    Document,
    Memory,
    Workspace,
    Knowledge,
    Web,
    Artifact,
    Note,
    Interview,
}

impl ToolNamespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Skill => "skill",
            Self::Attachment => "attachment",
            Self::Document => "document",
            Self::Memory => "memory",
            Self::Workspace => "workspace",
            Self::Knowledge => "knowledge",
            Self::Web => "web",
            Self::Artifact => "artifact",
            Self::Note => "note",
            Self::Interview => "interview",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolResourceCost {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolHandler {
    SearchTools,
    InspectTool,
    SearchSkills,
    InspectSkill,
    ActivateSkill,
    ReadSkillResource,
    ReadAttachmentText,
    ReadPdfPages,
    ReadDocxBlocks,
    ReadXlsxRows,
    MemoryRead,
    MemorySearch,
    MemoryModify,
    WorkspaceList,
    WorkspaceGlob,
    WorkspaceSearch,
    WorkspaceRead,
    KnowledgeList,
    KnowledgeSearch,
    KnowledgeRead,
    WebSearch,
    WebFetch,
    PresentArtifact,
    NoteList,
    NoteRead,
    NoteCreate,
    NoteUpdate,
    InterviewListAvailable,
    InterviewStartSession,
    InterviewGetQuestion,
    InterviewSubmitResponse,
    InterviewGetProgress,
    InterviewCompleteSession,
    InterviewExportResults,
    InterviewResumeSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalPolicy {
    Never,
    ReadOnly,
    MemoryRead,
    Sensitive,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    pub namespace: ToolNamespace,
    pub handler: ToolHandler,
    pub risk: ToolRisk,
    pub read_only: bool,
    pub parallel_safe: bool,
    pub approval: ToolApprovalPolicy,
    pub resource_cost: ToolResourceCost,
    pub max_output_chars: usize,
}

const DEFAULT_OUTPUT_LIMIT: usize = 20_000;
pub(super) const MAX_DISCOVERY_QUERY_CHARS: usize = 200;
pub(super) const MAX_DISCOVERY_RESULTS: usize = 12;
pub(super) const MAX_SKILL_ARGUMENT_CHARS: usize = 2_000;
pub(super) const MAX_SKILL_RESOURCE_PATH_CHARS: usize = 1_000;
pub(super) const MAX_SKILL_RESOURCE_READ_BYTES: usize = 32_000;
pub(super) const DEFAULT_ATTACHMENT_READ_BYTES: usize = 8_000;
pub(super) const MAX_ATTACHMENT_READ_BYTES: usize = 32_000;
pub(super) const MAX_PDF_PAGES_PER_CALL: usize = 12;
pub(super) const DEFAULT_MEMORY_READ_BYTES: usize = 8_000;
pub(super) const MAX_MEMORY_READ_BYTES: usize = 32_000;
pub(super) const MAX_MEMORY_MODIFY_BYTES: usize = 16_000;
pub(super) const MAX_WORKSPACE_PATH_CHARS: usize = 2_000;
pub(super) const MAX_WEB_URL_CHARS: usize = 4_096;
pub(super) const MAX_ARTIFACT_CHARS: usize = 100_000;

impl ToolEntry {
    pub fn model_tool(&self) -> ModelTool {
        ModelTool {
            name: self.name.to_string(),
            description: self.description.to_string(),
            input_schema: (self.input_schema)(),
        }
    }
}

fn discovery_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "minLength": 1, "maxLength": MAX_DISCOVERY_QUERY_CHARS },
            "limit": { "type": "integer", "minimum": 1, "maximum": MAX_DISCOVERY_RESULTS }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn inspect_tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "minLength": 1, "maxLength": 128 }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn skill_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "minLength": 1, "maxLength": 128 }
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

fn activate_skill_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "arguments": { "type": "string", "maxLength": MAX_SKILL_ARGUMENT_CHARS }
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

fn read_skill_resource_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "minLength": 1, "maxLength": 64 },
            "path": { "type": "string", "minLength": 1, "maxLength": MAX_SKILL_RESOURCE_PATH_CHARS },
            "startLine": { "type": "integer", "minimum": 1 },
            "endLine": { "type": "integer", "minimum": 1 },
            "maxBytes": { "type": "integer", "minimum": 1, "maximum": MAX_SKILL_RESOURCE_READ_BYTES }
        },
        "required": ["id", "path"],
        "additionalProperties": false
    })
}

fn workspace_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "maxLength": MAX_WORKSPACE_PATH_CHARS },
            "depth": { "type": "integer", "minimum": 1, "maximum": 4 },
            "cursor": { "type": "integer", "minimum": 0, "maximum": 100000 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
        },
        "additionalProperties": false
    })
}

fn workspace_glob_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "minLength": 1, "maxLength": 500 },
            "cursor": { "type": "integer", "minimum": 0, "maximum": 100000 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
        },
        "required": ["pattern"],
        "additionalProperties": false
    })
}

fn workspace_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "minLength": 1, "maxLength": 500 },
            "glob": { "type": "string", "maxLength": 500 },
            "caseSensitive": { "type": "boolean" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn workspace_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "minLength": 1, "maxLength": MAX_WORKSPACE_PATH_CHARS },
            "startLine": { "type": "integer", "minimum": 1 },
            "endLine": { "type": "integer", "minimum": 1 },
            "maxBytes": { "type": "integer", "minimum": 1, "maximum": 32000 }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn knowledge_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "enum": ["all", "note", "document"] },
            "query": { "type": "string", "maxLength": 500 },
            "cursor": { "type": "integer", "minimum": 0, "maximum": 500 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
        },
        "additionalProperties": false
    })
}

fn knowledge_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "minLength": 1, "maxLength": 500 },
            "kind": { "type": "string", "enum": ["all", "note", "document"] },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn knowledge_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "enum": ["note", "document"] },
            "id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "startLine": { "type": "integer", "minimum": 1 },
            "endLine": { "type": "integer", "minimum": 1 },
            "maxBytes": { "type": "integer", "minimum": 1, "maximum": 32000 },
            "pages": {
                "type": "array",
                "items": { "type": "integer", "minimum": 1 },
                "minItems": 1,
                "maxItems": 12
            }
        },
        "required": ["kind", "id"],
        "additionalProperties": false
    })
}

fn web_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "minLength": 1, "maxLength": 500 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 20 }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn web_fetch_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "minLength": 1, "maxLength": MAX_WEB_URL_CHARS },
            "maxBytes": { "type": "integer", "minimum": 1, "maximum": 2097152 }
        },
        "required": ["url"],
        "additionalProperties": false
    })
}

fn present_artifact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "minLength": 1, "maxLength": 200 },
            "kind": { "type": "string", "enum": ["markdown", "code", "json", "mermaid", "html", "text"] },
            "language": { "type": "string", "maxLength": 80 },
            "content": { "type": "string", "minLength": 1, "maxLength": MAX_ARTIFACT_CHARS }
        },
        "required": ["title", "kind", "content"],
        "additionalProperties": false
    })
}

fn note_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "maxLength": 500 },
            "cursor": { "type": "integer", "minimum": 0, "maximum": 100000 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
        },
        "additionalProperties": false
    })
}

fn note_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "startLine": { "type": "integer", "minimum": 1 },
            "endLine": { "type": "integer", "minimum": 1 },
            "maxBytes": { "type": "integer", "minimum": 1, "maximum": 32000 }
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

fn note_create_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "minLength": 1, "maxLength": 200 },
            "content": { "type": "string", "minLength": 1, "maxLength": 100000 },
            "groupName": { "type": "string", "maxLength": 120 }
        },
        "required": ["title", "content"],
        "additionalProperties": false
    })
}

fn note_update_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "title": { "type": "string", "minLength": 1, "maxLength": 200 },
            "content": { "type": "string", "minLength": 1, "maxLength": 100000 }
        },
        "required": ["id", "title", "content"],
        "additionalProperties": false
    })
}

fn interview_list_available_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn interview_start_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "scenarioId": { "type": "string", "minLength": 1, "maxLength": 128 },
            "participantId": { "type": "string", "minLength": 1, "maxLength": 128 },
            "metadata": { "type": "object", "additionalProperties": true }
        },
        "required": ["scenarioId", "participantId"],
        "additionalProperties": false
    })
}

fn interview_session_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sessionId": { "type": "string", "minLength": 1, "maxLength": 128 }
        },
        "required": ["sessionId"],
        "additionalProperties": false
    })
}

fn interview_submit_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sessionId": { "type": "string", "minLength": 1, "maxLength": 128 },
            "questionId": { "type": "string", "minLength": 1, "maxLength": 128 },
            "value": {}
        },
        "required": ["sessionId", "questionId", "value"],
        "additionalProperties": false
    })
}

fn interview_export_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sessionId": { "type": "string", "minLength": 1, "maxLength": 128 },
            "format": { "type": "string", "enum": ["json", "markdown"] }
        },
        "required": ["sessionId"],
        "additionalProperties": false
    })
}

fn attachment_text_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "attachmentId": { "type": "string" },
            "startLine": { "type": "integer", "minimum": 1 },
            "endLine": { "type": "integer", "minimum": 1 },
            "maxBytes": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_ATTACHMENT_READ_BYTES
            }
        },
        "required": ["attachmentId"],
        "additionalProperties": false
    })
}

fn pdf_pages_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "attachmentId": { "type": "string" },
            "pages": {
                "type": "array",
                "items": { "type": "integer", "minimum": 1 },
                "minItems": 1,
                "maxItems": MAX_PDF_PAGES_PER_CALL
            }
        },
        "required": ["attachmentId", "pages"],
        "additionalProperties": false
    })
}

fn docx_blocks_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "attachmentId": { "type": "string" },
            "startBlock": { "type": "integer", "minimum": 1 },
            "endBlock": { "type": "integer", "minimum": 1 }
        },
        "required": ["attachmentId"],
        "additionalProperties": false
    })
}

fn xlsx_rows_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "attachmentId": { "type": "string" },
            "sheetName": { "type": "string", "maxLength": 128 },
            "startRow": { "type": "integer", "minimum": 1 },
            "endRow": { "type": "integer", "minimum": 1 }
        },
        "required": ["attachmentId"],
        "additionalProperties": false
    })
}

fn memory_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "layer": { "type": "string", "enum": ["l1", "l2"] },
            "startLine": { "type": "integer", "minimum": 1 },
            "endLine": { "type": "integer", "minimum": 1 },
            "maxBytes": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_MEMORY_READ_BYTES
            }
        },
        "required": ["layer"],
        "additionalProperties": false
    })
}

fn memory_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "minLength": 1, "maxLength": 200 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 20 }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn memory_modify_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "layer": { "type": "string", "enum": ["l1", "l2"] },
            "operation": { "type": "string", "enum": ["append", "replace", "remove"] },
            "target": { "type": "string" },
            "content": { "type": "string", "maxLength": MAX_MEMORY_MODIFY_BYTES }
        },
        "required": ["layer", "operation"],
        "additionalProperties": false
    })
}

pub static TOOL_ENTRIES: &[ToolEntry] = &[
    ToolEntry {
        name: "search_tools",
        description: "按任务关键词搜索当前会话可用工具的轻量目录；命中后必须先 inspect_tool 查看契约，不能直接猜参数执行。",
        input_schema: discovery_schema,
        namespace: ToolNamespace::Discovery,
        handler: ToolHandler::SearchTools,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "inspect_tool",
        description: "查看 search_tools 命中的工具完整参数契约、权限、成本和输出边界；成功后该工具才会在下一轮变为可执行。",
        input_schema: inspect_tool_schema,
        namespace: ToolNamespace::Discovery,
        handler: ToolHandler::InspectTool,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "search_skills",
        description: "按任务关键词补充搜索当前工作区可用 Skill 的轻量目录；不加载 Skill 正文。",
        input_schema: discovery_schema,
        namespace: ToolNamespace::Discovery,
        handler: ToolHandler::SearchSkills,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "inspect_skill",
        description: "查看轻量目录中某个 Skill 的适用范围、来源、资源和所需工具，不加载完整 SKILL.md 正文。",
        input_schema: skill_id_schema,
        namespace: ToolNamespace::Skill,
        handler: ToolHandler::InspectSkill,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "activate_skill",
        description: "加载已经 inspect_skill 检查过且与当前任务匹配的 Skill 完整工作说明；同一运行无需重复激活。",
        input_schema: activate_skill_schema,
        namespace: ToolNamespace::Skill,
        handler: ToolHandler::ActivateSkill,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "read_skill_resource",
        description: "按需读取已激活 Skill 目录内的有界 UTF-8 参考资源；拒绝路径逃逸、隐藏路径、符号链接和审计文件。",
        input_schema: read_skill_resource_schema,
        namespace: ToolNamespace::Skill,
        handler: ToolHandler::ReadSkillResource,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: 40_000,
    },
    // Compatibility lookup for persisted traces and older integrations. It is
    // intentionally never disclosed by configure_model_request.
    ToolEntry {
        name: "skill",
        description: "兼容旧版本的 Skill 激活名称；新请求必须使用 inspect_skill -> activate_skill。",
        input_schema: activate_skill_schema,
        namespace: ToolNamespace::Skill,
        handler: ToolHandler::ActivateSkill,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "read_attachment_text",
        description: "读取当前会话中文本、Markdown、数据、配置或源代码附件的指定行范围。",
        input_schema: attachment_text_schema,
        namespace: ToolNamespace::Attachment,
        handler: ToolHandler::ReadAttachmentText,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "read_pdf_pages",
        description: "读取当前会话 PDF 安全副本的指定页文本，并返回可引用的页码标识。",
        input_schema: pdf_pages_schema,
        namespace: ToolNamespace::Document,
        handler: ToolHandler::ReadPdfPages,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::High,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "read_docx_blocks",
        description: "按内容块读取当前会话 DOCX 安全副本的正文和表格文本。",
        input_schema: docx_blocks_schema,
        namespace: ToolNamespace::Document,
        handler: ToolHandler::ReadDocxBlocks,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "read_xlsx_rows",
        description: "读取当前会话 XLSX 安全副本的工作表目录和指定行范围。",
        input_schema: xlsx_rows_schema,
        namespace: ToolNamespace::Document,
        handler: ToolHandler::ReadXlsxRows,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "memory_read",
        description: "按行读取用户记忆。L1 是短记忆，L2 是长期记忆；只读取完成任务所需的最小范围。",
        input_schema: memory_read_schema,
        namespace: ToolNamespace::Memory,
        handler: ToolHandler::MemoryRead,
        risk: ToolRisk::MemoryRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::MemoryRead,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "memory_search",
        description: "用关键词搜索 L2 长期记忆，返回少量匹配片段。",
        input_schema: memory_search_schema,
        namespace: ToolNamespace::Memory,
        handler: ToolHandler::MemorySearch,
        risk: ToolRisk::MemoryRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Sensitive,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "memory_modify",
        description: "在用户允许且会话权限通过时修改记忆。replace/remove 的 target 必须唯一匹配。禁止写入凭据或外部指令。",
        input_schema: memory_modify_schema,
        namespace: ToolNamespace::Memory,
        handler: ToolHandler::MemoryModify,
        risk: ToolRisk::MemoryWrite,
        read_only: false,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Sensitive,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "workspace_list",
        description: "在用户配置的工作目录内列出有界目录树；跳过依赖、构建产物、符号链接和敏感文件。",
        input_schema: workspace_list_schema,
        namespace: ToolNamespace::Workspace,
        handler: ToolHandler::WorkspaceList,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "workspace_glob",
        description: "用 Glob 模式查找用户工作目录内的文件；结果分页且不会越过工作区根目录。",
        input_schema: workspace_glob_schema,
        namespace: ToolNamespace::Workspace,
        handler: ToolHandler::WorkspaceGlob,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "workspace_search",
        description: "在用户工作目录的文本和代码文件中进行有界关键词搜索，返回文件、行号和稳定引用。",
        input_schema: workspace_search_schema,
        namespace: ToolNamespace::Workspace,
        handler: ToolHandler::WorkspaceSearch,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: 32_000,
    },
    ToolEntry {
        name: "workspace_read",
        description: "按行读取用户工作目录内的 UTF-8 文本或代码文件；拒绝路径逃逸、符号链接和敏感凭据。",
        input_schema: workspace_read_schema,
        namespace: ToolNamespace::Workspace,
        handler: ToolHandler::WorkspaceRead,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: 32_000,
    },
    ToolEntry {
        name: "knowledge_list",
        description: "分页列出 Mnemora 本地笔记与文献的轻量目录，不预载完整正文或 PDF。",
        input_schema: knowledge_list_schema,
        namespace: ToolNamespace::Knowledge,
        handler: ToolHandler::KnowledgeList,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "knowledge_search",
        description: "在 Mnemora 本地笔记正文和文献元数据中执行有界词法检索，明确区分无结果与读取失败。",
        input_schema: knowledge_search_schema,
        namespace: ToolNamespace::Knowledge,
        handler: ToolHandler::KnowledgeSearch,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: 32_000,
    },
    ToolEntry {
        name: "knowledge_read",
        description: "按行读取本地笔记，或按页读取文献 PDF 文本层，并返回稳定知识库引用。",
        input_schema: knowledge_read_schema,
        namespace: ToolNamespace::Knowledge,
        handler: ToolHandler::KnowledgeRead,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::High,
        max_output_chars: 32_000,
    },
    ToolEntry {
        name: "web_search",
        description: "搜索公开网页，返回带来源 ID、标题、URL、摘要和检索时间的外部不可信结果。",
        input_schema: web_search_schema,
        namespace: ToolNamespace::Web,
        handler: ToolHandler::WebSearch,
        risk: ToolRisk::NetworkRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: 32_000,
    },
    ToolEntry {
        name: "web_fetch",
        description: "安全抓取公开 HTTP/HTTPS 文本页面；逐次校验重定向和 DNS，拒绝本机及私有网络地址。",
        input_schema: web_fetch_schema,
        namespace: ToolNamespace::Web,
        handler: ToolHandler::WebFetch,
        risk: ToolRisk::NetworkRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: 40_000,
    },
    ToolEntry {
        name: "present_artifact",
        description: "把 Markdown、代码、JSON、Mermaid、HTML 或纯文本整理为本轮结构化交付；不会自动写入磁盘。",
        input_schema: present_artifact_schema,
        namespace: ToolNamespace::Artifact,
        handler: ToolHandler::PresentArtifact,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: MAX_ARTIFACT_CHARS,
    },
    ToolEntry {
        name: "note_list",
        description: "分页列出 Mnemora 笔记轻量目录；只返回标题、预览、分组和稳定笔记 ID。",
        input_schema: note_list_schema,
        namespace: ToolNamespace::Note,
        handler: ToolHandler::NoteList,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: 32_000,
    },
    ToolEntry {
        name: "note_read",
        description: "按行读取 Mnemora 笔记正文，并返回稳定的笔记与行号引用。",
        input_schema: note_read_schema,
        namespace: ToolNamespace::Note,
        handler: ToolHandler::NoteRead,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: 32_000,
    },
    ToolEntry {
        name: "note_create",
        description: "在用户权限允许时创建一条 Mnemora 全局 Markdown 笔记；返回真实持久化后的笔记 ID。",
        input_schema: note_create_schema,
        namespace: ToolNamespace::Note,
        handler: ToolHandler::NoteCreate,
        risk: ToolRisk::NoteWrite,
        read_only: false,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Sensitive,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "note_update",
        description: "在用户权限允许时更新指定 Mnemora 笔记的标题和 Markdown 正文；不会删除笔记。",
        input_schema: note_update_schema,
        namespace: ToolNamespace::Note,
        handler: ToolHandler::NoteUpdate,
        risk: ToolRisk::NoteWrite,
        read_only: false,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Sensitive,
        resource_cost: ToolResourceCost::Medium,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "interview_list_available",
        description: "列出本地可用的面试场景模板；不读取简历、不启动外部服务。",
        input_schema: interview_list_available_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewListAvailable,
        risk: ToolRisk::BuiltinRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::Never,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "interview_start_session",
        description: "在本地创建可恢复的面试练习会话，并返回场景问题目录。",
        input_schema: interview_start_session_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewStartSession,
        risk: ToolRisk::NoteWrite,
        read_only: false,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Sensitive,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "interview_get_question",
        description: "读取面试会话的当前下一道未回答问题，不暴露未来题目答案。",
        input_schema: interview_session_id_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewGetQuestion,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "interview_submit_response",
        description: "保存一轮面试回答并更新本地进度；无效回答不会推进问题状态。",
        input_schema: interview_submit_response_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewSubmitResponse,
        risk: ToolRisk::NoteWrite,
        read_only: false,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Sensitive,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "interview_get_progress",
        description: "读取面试会话进度、必答问题剩余量和当前状态。",
        input_schema: interview_session_id_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewGetProgress,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "interview_complete_session",
        description: "在所有必答问题完成后结束面试会话并生成摘要。",
        input_schema: interview_session_id_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewCompleteSession,
        risk: ToolRisk::NoteWrite,
        read_only: false,
        parallel_safe: false,
        approval: ToolApprovalPolicy::Sensitive,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
    ToolEntry {
        name: "interview_export_results",
        description: "导出本地面试会话的结构化回答记录为 JSON 或 Markdown。",
        input_schema: interview_export_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewExportResults,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: true,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: 100_000,
    },
    ToolEntry {
        name: "interview_resume_session",
        description: "恢复本地未完成的面试会话，并返回断点和下一道问题。",
        input_schema: interview_session_id_schema,
        namespace: ToolNamespace::Interview,
        handler: ToolHandler::InterviewResumeSession,
        risk: ToolRisk::ConversationRead,
        read_only: true,
        parallel_safe: false,
        approval: ToolApprovalPolicy::ReadOnly,
        resource_cost: ToolResourceCost::Low,
        max_output_chars: DEFAULT_OUTPUT_LIMIT,
    },
];

pub fn find_tool(name: &str) -> Option<&'static ToolEntry> {
    TOOL_ENTRIES.iter().find(|entry| entry.name == name)
}

pub fn assert_valid_registry() {
    debug_assert!(TOOL_ENTRIES
        .iter()
        .all(|entry| { entry.read_only || !entry.parallel_safe }));
    debug_assert!(TOOL_ENTRIES.iter().all(|entry| {
        matches!(
            entry.namespace,
            ToolNamespace::Discovery
                | ToolNamespace::Skill
                | ToolNamespace::Attachment
                | ToolNamespace::Document
                | ToolNamespace::Memory
                | ToolNamespace::Workspace
                | ToolNamespace::Knowledge
                | ToolNamespace::Web
                | ToolNamespace::Artifact
                | ToolNamespace::Note
                | ToolNamespace::Interview
        ) && matches!(
            entry.resource_cost,
            ToolResourceCost::Low | ToolResourceCost::Medium | ToolResourceCost::High
        )
    }));
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{find_tool, TOOL_ENTRIES};

    #[test]
    fn tool_names_are_unique_and_lookup_is_stable() {
        let names = TOOL_ENTRIES
            .iter()
            .map(|entry| entry.name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), TOOL_ENTRIES.len());
        assert_eq!(find_tool("read_pdf_pages").unwrap().name, "read_pdf_pages");
        assert!(find_tool("unknown").is_none());
    }

    #[test]
    fn every_entry_builds_its_model_definition() {
        for entry in TOOL_ENTRIES {
            let definition = entry.model_tool();
            assert_eq!(definition.name, entry.name);
            assert!(!definition.description.trim().is_empty());
            assert_eq!(definition.input_schema["type"], "object");
            assert_eq!(definition.input_schema["additionalProperties"], false);
        }
    }

    #[test]
    fn network_tools_use_a_distinct_network_read_risk() {
        use crate::chat::agent::types::ToolRisk;

        assert_eq!(find_tool("web_search").unwrap().risk, ToolRisk::NetworkRead);
        assert_eq!(find_tool("web_fetch").unwrap().risk, ToolRisk::NetworkRead);
    }

    #[test]
    fn note_mutations_use_a_distinct_note_write_risk() {
        use crate::chat::agent::types::ToolRisk;

        assert_eq!(find_tool("note_create").unwrap().risk, ToolRisk::NoteWrite);
        assert_eq!(find_tool("note_update").unwrap().risk, ToolRisk::NoteWrite);
    }
}
