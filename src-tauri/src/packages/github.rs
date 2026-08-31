//! GitHub 搜索与仓库快照下载。
//!
//! 边界原则：
//!   * 只接受 `owner/repo` 形式的标识，**不接受任意 URL**。下载地址由本模块
//!     自行拼装，因此调用方（包括模型）无法把请求指向别的主机。
//!   * 每个响应都限流限时；zipball 有独立的更严格上限。
//!   * 重定向后的最终 URL 仍要校验主机，防止被引到第三方。

use std::time::Duration;

use reqwest::{header, Client, Response};
use serde::Deserialize;

use super::types::{RemoteCandidate, RemotePackageKind, RemoteSearchResult};

const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_SEARCH_RESPONSE_BYTES: usize = 512 * 1024;
/// 资源包是文本与少量图片；32MB 足够，同时挡住误指向大仓库。
const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const SEARCH_PAGE_SIZE: u32 = 20;
const MAX_DESCRIPTION_CHARS: usize = 400;

/// GitHub 允许的下载主机。zipball 会 302 到 codeload。
const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &["api.github.com", "codeload.github.com", "github.com"];

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    total_count: u64,
    #[serde(default)]
    incomplete_results: bool,
    #[serde(default)]
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    full_name: String,
    #[serde(default)]
    description: Option<String>,
    html_url: String,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    pushed_at: String,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    license: Option<License>,
    #[serde(default)]
    default_branch: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    owner: Option<Owner>,
}

#[derive(Debug, Deserialize)]
struct License {
    #[serde(default)]
    spdx_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Owner {
    #[serde(default)]
    login: String,
}

/// 校验 `owner/repo`：两段、非空、只含 GitHub 允许的字符。
///
/// 这道校验同时防住路径穿越（`../`）和把标识当 URL 用的情况。
pub fn validate_full_name(value: &str) -> Result<(String, String), String> {
    let trimmed = value.trim();
    let mut parts = trimmed.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err("仓库标识必须是 owner/repo 形式。".to_string());
    }
    let valid = |segment: &str| {
        !segment.is_empty()
            && segment.len() <= 100
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if !valid(owner) || !valid(repo) {
        return Err(format!("仓库标识无效：{trimmed}"));
    }
    Ok((owner.to_string(), repo.to_string()))
}

/// git ref 只允许分支/标签常见字符，且不能以 `-` 开头（避免被当作参数）。
pub fn validate_git_ref(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 200 || trimmed.starts_with('-') {
        return Err(format!("git ref 无效：{trimmed}"));
    }
    if trimmed.contains("..") || trimmed.starts_with('/') || trimmed.ends_with('/') {
        return Err(format!("git ref 无效：{trimmed}"));
    }
    if !trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(format!("git ref 无效：{trimmed}"));
    }
    Ok(trimmed.to_string())
}

/// 搜索关键词构造。
///
/// 早期实现强制追加 `topic:mnemora-*`，结果只有专门为 Mnemora 打过标签的
/// 仓库才能出现，标准 Agent Skill 和 Codex 插件几乎全部被排除。这里改成
/// GitHub 仓库名称、描述和 README 搜索；包格式是否兼容由下载后的解析器确认。
fn build_query(_kind: RemotePackageKind, query: &str) -> Result<String, String> {
    let cleaned = query.trim();
    if cleaned.is_empty() {
        return Err("请提供搜索关键词。".to_string());
    }
    if cleaned.len() > 200 {
        return Err("搜索关键词过长。".to_string());
    }
    // 只允许温和的查询字符，避免调用方注入 GitHub 搜索限定符改变语义
    // （例如 user: / repo: 把搜索指向特定私有目标）。
    if !cleaned
        .chars()
        .all(|ch| ch.is_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.' | '+' | '#'))
    {
        return Err("搜索关键词只能包含字母、数字、空格和 - _ . + # 符号。".to_string());
    }
    Ok(format!("{cleaned} in:name,description,readme"))
}

pub async fn search_repositories(
    client: &Client,
    kind: RemotePackageKind,
    query: &str,
) -> Result<RemoteSearchResult, String> {
    let q = build_query(kind, query)?;
    let url = reqwest::Url::parse_with_params(
        "https://api.github.com/search/repositories",
        &[
            ("q", q.as_str()),
            ("sort", "stars"),
            ("order", "desc"),
            ("per_page", &SEARCH_PAGE_SIZE.to_string()),
        ],
    )
    .map_err(|error| format!("构造搜索请求失败：{error}"))?;

    let response = tokio::time::timeout(
        SEARCH_TIMEOUT,
        client
            .get(url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send(),
    )
    .await
    .map_err(|_| {
        "GitHub 搜索请求超时。请检查“设置 → 关于 → 网络代理”，或直接粘贴 GitHub 仓库/目录地址。"
            .to_string()
    })?
    .map_err(|error| format!("GitHub 搜索请求失败：{error}"))?;

    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err("GitHub 搜索接口触发速率限制，请稍后重试。".to_string());
    }
    if !status.is_success() {
        return Err(format!("GitHub 搜索返回 HTTP {}", status.as_u16()));
    }

    let bytes = read_bounded(response, MAX_SEARCH_RESPONSE_BYTES, "GitHub 搜索").await?;
    let parsed: SearchResponse = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 GitHub 搜索结果失败：{error}"))?;

    let candidates = parsed
        .items
        .into_iter()
        .filter_map(|item| {
            // 名称不合法的条目直接丢弃，而不是留到下载阶段才失败。
            let (owner, _) = validate_full_name(&item.full_name).ok()?;
            Some(RemoteCandidate {
                owner: item.owner.map(|value| value.login).unwrap_or(owner),
                full_name: item.full_name,
                description: item
                    .description
                    .unwrap_or_default()
                    .chars()
                    .take(MAX_DESCRIPTION_CHARS)
                    .collect(),
                html_url: item.html_url,
                stars: item.stargazers_count,
                pushed_at: item.pushed_at,
                archived: item.archived,
                license: item.license.and_then(|value| value.spdx_id),
                default_branch: if item.default_branch.is_empty() {
                    "main".to_string()
                } else {
                    item.default_branch
                },
                topics: item.topics,
            })
        })
        .collect::<Vec<_>>();

    Ok(RemoteSearchResult {
        total_count: parsed.total_count,
        truncated: parsed.incomplete_results || parsed.total_count > candidates.len() as u64,
        candidates,
    })
}

/// 下载仓库快照 zipball。返回 (字节, 解析出的 commit sha)。
///
/// commit sha 从 `content-disposition` 或最终 URL 推断；拿不到时返回 ref 本身，
/// 调用方会把它写进来源记录，缺失不阻断安装但会降低可追溯性。
pub async fn download_zipball(
    client: &Client,
    owner: &str,
    repo: &str,
    git_ref: &str,
) -> Result<(Vec<u8>, String), String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/zipball/{git_ref}");
    let response = tokio::time::timeout(
        DOWNLOAD_TIMEOUT,
        client
            .get(&url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send(),
    )
    .await
    .map_err(|_| "下载仓库快照超时。".to_string())?
    .map_err(|error| format!("下载仓库快照失败：{error}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "下载仓库快照返回 HTTP {}（仓库或 ref 可能不存在）",
            response.status().as_u16()
        ));
    }

    // 跟随重定向后必须仍在 GitHub 自己的主机上。
    let final_url = response.url().clone();
    if final_url.scheme() != "https"
        || !final_url
            .host_str()
            .is_some_and(|host| ALLOWED_DOWNLOAD_HOSTS.contains(&host))
    {
        return Err(format!(
            "仓库快照被重定向到不可信地址：{}",
            final_url.host_str().unwrap_or("<unknown>")
        ));
    }

    let sha = extract_commit_sha(&response).unwrap_or_else(|| git_ref.to_string());
    let bytes = read_bounded(response, MAX_ARCHIVE_BYTES, "仓库快照").await?;
    Ok((bytes, sha))
}

/// zipball 的顶层目录形如 `owner-repo-<sha>`，content-disposition 里也带同样后缀。
fn extract_commit_sha(response: &Response) -> Option<String> {
    let value = response
        .headers()
        .get(header::CONTENT_DISPOSITION)?
        .to_str()
        .ok()?;
    let stem = value.rsplit_once(".zip").map(|(head, _)| head)?;
    let sha = stem.rsplit('-').next()?;
    let looks_like_sha =
        sha.len() >= 7 && sha.len() <= 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit());
    looks_like_sha.then(|| sha.to_string())
}

async fn read_bounded(
    mut response: Response,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!(
            "{label}响应过大（超过 {} MB）。",
            max_bytes / 1024 / 1024
        ));
    }
    let mut output = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("读取{label}失败：{error}"))?
    {
        if output.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!(
                "{label}响应过大（超过 {} MB）。",
                max_bytes / 1024 / 1024
            ));
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_owner_repo() {
        assert_eq!(
            validate_full_name("tushanmiao/Mnemora").unwrap(),
            ("tushanmiao".to_string(), "Mnemora".to_string())
        );
        assert_eq!(
            validate_full_name("  a-b_c.d/e.f  ").unwrap(),
            ("a-b_c.d".to_string(), "e.f".to_string())
        );
    }

    #[test]
    fn rejects_urls_traversal_and_extra_segments() {
        for value in [
            "https://github.com/a/b",
            "../../etc/passwd",
            "a/b/c",
            "a",
            "",
            "/b",
            "a/",
            "a b/c",
            "a/..",
            "a/.",
        ] {
            assert!(validate_full_name(value).is_err(), "value={value}");
        }
    }

    #[test]
    fn rejects_search_qualifier_injection() {
        // 冒号会被拒，调用方无法追加 user:/repo: 之类限定符改变搜索语义。
        assert!(build_query(RemotePackageKind::Skill, "weather user:someone").is_err());
        assert!(build_query(RemotePackageKind::Skill, "a\"b").is_err());
        assert!(build_query(RemotePackageKind::Skill, "").is_err());
        let ok = build_query(RemotePackageKind::Plugin, "note helper").unwrap();
        assert_eq!(ok, "note helper in:name,description,readme");
    }

    #[test]
    fn searches_standard_repositories_without_mnemora_topics() {
        for kind in [
            RemotePackageKind::Skill,
            RemotePackageKind::Plugin,
            RemotePackageKind::Pet,
        ] {
            let query = build_query(kind, "question-framing").unwrap();
            assert_eq!(query, "question-framing in:name,description,readme");
            assert!(!query.contains("mnemora-"));
        }
    }

    #[test]
    fn rejects_dangerous_git_refs() {
        for value in ["", "-x", "a..b", "/main", "main/", "a b", "a;b"] {
            assert!(validate_git_ref(value).is_err(), "value={value}");
        }
        assert_eq!(validate_git_ref("main").unwrap(), "main");
        assert_eq!(
            validate_git_ref("release/v1.2.0").unwrap(),
            "release/v1.2.0"
        );
    }
}
