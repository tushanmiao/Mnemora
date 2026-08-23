//! 供应商无关的 Agent 工具层。
//!
//! `registry` 只暴露固定的只读工具，并负责参数校验、路径边界、超时前的有界输入和
//! 输出截断；模型协议转换仍由 `ai/providers` 负责。一次 Run 的 Skill 正文缓存由
//! `SkillRunCache` 持有，Run 结束后随栈帧释放。

mod artifacts;
pub mod catalog;
mod documents;
mod knowledge;
mod notes;
pub mod registry;
pub mod run_machine;
pub mod types;
mod web;
mod workspace;

pub use registry::{
    apply_tool_disclosures, argument_summary, build_runtime_context, configure_model_request,
    execute_bounded_attachment_reader, execute_bounded_text_window, execute_tool, parallel_safe,
    requires_approval, tool_risk, validate_disclosed_tool_calls, SkillRunCache, ToolRuntimeContext,
};
pub use types::{ToolExecution, ToolTraceSnapshot, ToolTraceStatus};
