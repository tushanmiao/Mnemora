//! 模型设置与密钥存储层。
//!
//! - `types`：Rust 统一模型设置结构、默认值、规范化和校验。
//! - `repository`：把非敏感配置保存到版本化 JSON 文件。
//! - `secrets`：把 API Key 保存到操作系统凭据存储。
//!
//! 普通 `ModelSettings` 不包含完整 API Key，只通过 `has_api_key` 表示凭据状态。

pub mod repository;
pub mod secrets;
pub mod types;
