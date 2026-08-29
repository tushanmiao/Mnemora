//! 本地 Skill 系统。
//!
//! - `parser`：解析和校验 `SKILL.md`。
//! - `repository`：发现内置/用户技能、保存启用状态、按需读取详情与正文。
//! - `installer`：有界复制或解压到 staging，校验后原子安装。
//! - `types`：Tauri Commands 与内部流程共用的数据合同。

mod installer;
mod parser;
mod repository;
pub mod types;

pub(crate) use installer::stage_package_source;
pub(crate) use parser::validate_skill_id;
pub use repository::SkillRepository;
