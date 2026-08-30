//! Agent 工具：在 GitHub 上搜索可安装的 Skill / 插件 / 宠物资源包。
//!
//! **这个工具只读**。它把候选仓库和判断信号交回给模型陈述，安装动作
//! 完全不在模型的能力范围内——用户必须自己在确认对话框里看过清单与
//! 权限声明才能装。
//!
//! 为什么这条边界不能松：插件可以贡献 Skill，而 Skill 正文会随
//! `activate_skill` 进入模型上下文，且该工具的审批策略是 Never。
//! 于是恶意包的实际风险不是执行代码，而是**带持久性的提示注入**；
//! 同时插件签名验证尚未接入可信发布者目录。让模型自己决定装哪个仓库，
//! 等于把这个信任判断交给一个可被搜索结果影响的对象。

use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::{
    ai::error::ModelError,
    network,
    packages::{github::search_repositories, types::RemotePackageKind},
    settings::app_types::UpdateProxySettings,
};

use super::types::ToolExecution;

const MAX_PREVIEW_CHARS: usize = 400;
/// 交给模型的条数比 UI 少：模型只需要够用来陈述选项，不需要全量列表。
const MAX_MODEL_RESULTS: usize = 8;

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    value.chars().take(limit).collect::<String>() + "…"
}

fn parse_kind(value: &str) -> Result<RemotePackageKind, ModelError> {
    match value {
        "skill" => Ok(RemotePackageKind::Skill),
        "plugin" => Ok(RemotePackageKind::Plugin),
        "pet" => Ok(RemotePackageKind::Pet),
        other => Err(ModelError::invalid_configuration(format!(
            "kind 必须是 skill、plugin 或 pet，收到 {other}。"
        ))),
    }
}

/// 搜索用的 HTTP 客户端跟随用户的代理设置，与应用更新、网页工具一致。
fn build_client(proxy_settings: &UpdateProxySettings) -> Result<Client, ModelError> {
    let builder = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(45))
        .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")));
    let (builder, _) = network::configure_reqwest_builder(builder, proxy_settings)
        .map_err(ModelError::invalid_configuration)?;
    builder
        .build()
        .map_err(|error| ModelError::provider(format!("创建资源包搜索客户端失败：{error}")))
}

pub(super) async fn search_remote_packages(
    arguments: &Value,
    cancellation: &CancellationToken,
    proxy_settings: &UpdateProxySettings,
) -> Result<ToolExecution, ModelError> {
    let kind_text = arguments
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| ModelError::invalid_configuration("kind 是必填参数。"))?;
    let kind = parse_kind(kind_text)?;
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    let client = build_client(proxy_settings)?;
    let outcome = tokio::select! {
        _ = cancellation.cancelled() => return Err(ModelError::cancelled()),
        result = search_repositories(&client, kind, &query) => result,
    };

    // 搜索失败（速率限制、关键词非法、网络不可达）作为工具错误交回模型，
    // 让它能改写关键词重试或向用户说明，而不是让整个 run 失败。
    let outcome = match outcome {
        Ok(value) => value,
        Err(message) => {
            let content = serde_json::to_string(&json!({
                "status": "error",
                "error": { "code": "packageSearchFailed", "message": message, "retryable": true }
            }))
            .map_err(|error| {
                ModelError::invalid_configuration(format!("序列化搜索错误失败：{error}"))
            })?;
            let output_chars = content.chars().count();
            return Ok(ToolExecution {
                preview: truncate_chars(&message, MAX_PREVIEW_CHARS),
                content,
                is_error: true,
                activated_skill_id: None,
                output_chars,
                output_truncated: false,
            });
        }
    };

    let candidates = outcome
        .candidates
        .iter()
        .take(MAX_MODEL_RESULTS)
        .map(|candidate| {
            json!({
                "repository": candidate.full_name,
                "description": truncate_chars(&candidate.description, 200),
                "stars": candidate.stars,
                "pushedAt": candidate.pushed_at,
                "archived": candidate.archived,
                "license": candidate.license,
                "url": candidate.html_url,
            })
        })
        .collect::<Vec<_>>();

    // 明确告诉模型「你不能装」，避免它反复尝试寻找安装工具，
    // 也避免它对用户承诺自己会完成安装。
    let payload = json!({
        "status": "ok",
        "kind": kind_text,
        "query": query,
        "totalCount": outcome.total_count,
        "returned": candidates.len(),
        "candidates": candidates,
        "installGuidance": {
            "canInstall": false,
            "reason": "安装需要用户在确认对话框中查看清单与权限后自行批准；模型无法安装。",
            "userAction": format!("请用户在 Chat 中执行 /install {kind_text} <功能描述或名称>；已知仓库可写 /install {kind_text} <owner/repo>。"),
            "advice": "陈述候选时请给出星数、最近更新时间与是否归档，并提示未签名包的风险，让用户自己选择。"
        }
    });

    let content = serde_json::to_string(&payload)
        .map_err(|error| ModelError::invalid_configuration(format!("序列化搜索结果失败：{error}")))?;
    let preview = if candidates.is_empty() {
        format!("没有找到匹配的{kind_text}仓库")
    } else {
        format!("找到 {} 个候选（共 {} 个结果）", candidates.len(), outcome.total_count)
    };
    let output_chars = content.chars().count();
    Ok(ToolExecution {
        preview: truncate_chars(&preview, MAX_PREVIEW_CHARS),
        content,
        is_error: false,
        activated_skill_id: None,
        output_chars,
        output_truncated: outcome.candidates.len() > candidates.len(),
    })
}
