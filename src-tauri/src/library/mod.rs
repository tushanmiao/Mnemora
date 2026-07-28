//! Work 文献库领域模块。
//!
//! SQLite 只保存文献元数据、分类、标签和阅读状态；PDF 文件以应用内快照形式保存在
//! `app_data/library/files`。仓库对象只保存路径，每次命令短暂打开数据库连接，避免托盘
//! 状态长期保留 SQLite 连接和页面缓存。

pub mod import;
pub mod store;
pub mod types;

pub use store::LibraryRepository;
