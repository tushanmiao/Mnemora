//! 笔记同步领域模块。
//!
//! 同步配置、映射和目标适配器与 Chat、文献库解耦：飞书和 Notion 使用官方 HTTP
//! API，Obsidian 使用本地 Markdown 文件适配器。同步只在用户手动触发时运行，
//! 不启动常驻进程、不刷新远端令牌、不监听整个 Vault。

pub mod feishu;
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
