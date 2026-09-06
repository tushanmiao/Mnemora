//! PDF/Markdown 知识库域。
//!
//! 知识库只保存 `library_items` 中的 PDF 文献和 `library_notes` 中的 Markdown
//! 笔记的派生索引。原始业务表仍然是唯一权威来源；本模块中的 revision、chunk、
//! FTS 和任务记录都可以被删除并重建。

pub mod embedding;
mod embedding_worker;
mod markdown;
pub mod mineru;
pub mod repository;
pub mod schema;
pub mod types;
pub mod worker;

pub use repository::KnowledgeRepository;

/// 由 library schema 迁移调用。单独导出这个小入口，避免把知识表的 DDL
/// 再复制到 `library/store.rs` 的历史迁移链中。
pub(crate) fn migrate_v19(connection: &rusqlite::Connection) -> Result<(), String> {
    schema::migrate_v19(connection)
}

pub(crate) fn migrate_v20(connection: &rusqlite::Connection) -> Result<(), String> {
    schema::migrate_v20(connection)
}
