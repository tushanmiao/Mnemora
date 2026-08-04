//! 英语词库的按需下载、本地索引和单词查询。
//!
//! 词库来源页面使用 Base85 + Brotli 将数据嵌入 HTML。下载阶段才解码这份较大的
//! 中间数据，并落盘为索引 JSON 与逐条 JSONL；正常查询不会把完整词库加载到内存。

pub mod learning;
mod repository;
pub mod types;

pub use repository::{download_source_with_progress, EnglishRepository};
