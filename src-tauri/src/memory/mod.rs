//! 轻量双层记忆。
//!
//! L1 是可选的短在线记忆，L2 只通过有界工具按需读取。仓库不建立数据库、索引、
//! 文件监听器或常驻缓存；每次操作结束后，文件正文随局部变量释放。

mod repository;
mod types;

pub use repository::MemoryRepository;
pub use types::{MemoryLayer, MemoryModification, MemoryOperation, MemorySettings};
