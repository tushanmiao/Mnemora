//! 供应商无关的 AI 能力层。
//!
//! `dispatcher` 选择协议适配器，`providers` 处理 wire protocol，`types` 和 `error`
//! 向 Chat 业务层提供稳定合同，`http` 只保存共享的安全 HTTP 辅助逻辑。

pub mod dispatcher;
pub mod error;
pub mod model;
pub mod providers;
pub mod stream;
pub mod types;

mod http;
