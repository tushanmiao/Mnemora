//! Chat 业务编排层。
//!
//! `types` 定义 React 可以提交的非流式聊天合同并执行输入边界校验，`service` 根据
//! `providerId + modelId` 解析配置、从系统凭据读取 Key，再调用 AI dispatcher。

pub mod agent;
pub mod attachments;
pub mod conversation_types;
pub mod service;
pub mod storage;
pub mod types;
