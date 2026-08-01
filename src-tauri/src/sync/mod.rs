//! 笔记同步领域模块。
//!
//! 同步配置、映射和目标适配器与 Chat、文献库解耦：Obsidian 使用本地 Markdown
//! 文件适配器，Notion 使用官方 HTTP API 适配器。同步只在用户手动触发时运行，
//! 不启动常驻进程、不监听整个 Vault，保持托盘和空闲状态的低占用。

pub mod mapping;
pub mod markdown;
pub mod notion;
pub mod obsidian;
pub mod repository;
pub mod secrets;
pub mod service;
pub mod types;

pub use repository::SyncSettingsRepository;
pub use secrets::SyncSecretStore;
pub use types::SyncSettings;
