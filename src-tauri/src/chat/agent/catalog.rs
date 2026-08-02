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
    Skill,
    Attachment,
    Document,
    Memory,
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
    Skill,
    ReadAttachmentText,
    ReadPdfPages,
    ReadDocxBlocks,
    ReadXlsxRows,
    MemoryRead,
    MemorySearch,
    MemoryModify,
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
pub(super) const MAX_SKILL_ARGUMENT_CHARS: usize = 2_000;
pub(super) const DEFAULT_ATTACHMENT_READ_BYTES: usize = 8_000;
pub(super) const MAX_ATTACHMENT_READ_BYTES: usize = 32_000;
pub(super) const MAX_PDF_PAGES_PER_CALL: usize = 12;
pub(super) const DEFAULT_MEMORY_READ_BYTES: usize = 8_000;
pub(super) const MAX_MEMORY_READ_BYTES: usize = 32_000;
pub(super) const MAX_MEMORY_MODIFY_BYTES: usize = 16_000;

impl ToolEntry {
    pub fn model_tool(&self) -> ModelTool {
        ModelTool {
            name: self.name.to_string(),
            description: self.description.to_string(),
            input_schema: (self.input_schema)(),
        }
    }
}

fn skill_schema() -> Value {
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
        name: "skill",
        description: "按 ID 加载一个可用技能的完整工作说明；同一运行无需重复加载。",
        input_schema: skill_schema,
        namespace: ToolNamespace::Skill,
        handler: ToolHandler::Skill,
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
            ToolNamespace::Skill
                | ToolNamespace::Attachment
                | ToolNamespace::Document
                | ToolNamespace::Memory
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
}
