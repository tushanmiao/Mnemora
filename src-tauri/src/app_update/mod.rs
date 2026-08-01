//! GitHub 稳定版更新领域。
//!
//! 第一阶段只在用户点击时检查固定仓库的最新稳定 Release，不下载或执行安装包。
//! 后续接入 Tauri Updater 时，签名下载与安装仍由独立命令负责。

mod github;
mod types;

pub use github::check_latest_release;
pub use types::UpdateCheckResult;
