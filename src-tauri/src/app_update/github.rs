use std::{cmp::Ordering, time::Duration};

use reqwest::{header, Client, Response};
use serde::Deserialize;

use super::types::{UpdateCheckResult, UpdateCheckSource};

const REPOSITORY: &str = "tushanmiao/Mnemora";
const RELEASES_URL: &str = "https://github.com/tushanmiao/Mnemora/releases";
const API_LATEST_URL: &str = "https://api.github.com/repos/tushanmiao/Mnemora/releases/latest";
const WEB_LATEST_URL: &str = "https://github.com/tushanmiao/Mnemora/releases/latest";
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_API_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_RELEASE_NOTES_CHARS: usize = 20_000;

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<PrereleasePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrereleasePart {
    Number(u64),
    Text(String),
}

pub async fn check_latest_release(client: &Client) -> Result<UpdateCheckResult, String> {
    let current = env!("CARGO_PKG_VERSION");
    if let Ok(Ok(result)) = tokio::time::timeout(CHECK_TIMEOUT, check_api(client, current)).await {
        return Ok(result);
    }
    match tokio::time::timeout(CHECK_TIMEOUT, check_web_redirect(client, current)).await {
        Ok(result) => result.map_err(|error| {
            format!("无法检查 GitHub 更新：{error}。请前往 {RELEASES_URL} 手动查看。")
        }),
        Err(_) => Err(format!(
            "无法检查 GitHub 更新：请求超时。请前往 {RELEASES_URL} 手动查看。"
        )),
    }
}

async fn check_api(client: &Client, current: &str) -> Result<UpdateCheckResult, String> {
    let response = send_with_timeout(
        client
            .get(API_LATEST_URL)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28"),
    )
    .await?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub API 返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = read_bounded(response, MAX_API_RESPONSE_BYTES).await?;
    let release: GitHubRelease = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 GitHub Release 失败：{error}"))?;
    if release.draft || release.prerelease {
        return Err("GitHub latest 返回的不是稳定版本。".to_string());
    }
    build_result(
        current,
        &release.tag_name,
        &release.html_url,
        &release.body,
        &release.published_at,
        UpdateCheckSource::GitHubApi,
    )
}

async fn check_web_redirect(client: &Client, current: &str) -> Result<UpdateCheckResult, String> {
    let response = send_with_timeout(
        client
            .get(WEB_LATEST_URL)
            .header(header::ACCEPT, "text/html"),
    )
    .await?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub 页面返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let final_url = response.url();
    if final_url.scheme() != "https" || final_url.host_str() != Some("github.com") {
        return Err("GitHub 更新页面跳转到了不可信地址。".to_string());
    }
    let expected_prefix = format!("/{REPOSITORY}/releases/tag/");
    let tag = final_url
        .path()
        .strip_prefix(&expected_prefix)
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or_else(|| "GitHub 更新页面没有返回有效版本标签。".to_string())?;
    build_result(
        current,
        tag,
        final_url.as_str(),
        "",
        "",
        UpdateCheckSource::GitHubWeb,
    )
}

async fn send_with_timeout(builder: reqwest::RequestBuilder) -> Result<Response, String> {
    builder
        .send()
        .await
        .map_err(|error| format!("网络请求失败：{error}"))
}

async fn read_bounded(mut response: Response, max_bytes: usize) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err("GitHub Release 响应过大。".to_string());
    }
    let mut output = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("读取 GitHub Release 失败：{error}"))?
    {
        if output.len().saturating_add(chunk.len()) > max_bytes {
            return Err("GitHub Release 响应过大。".to_string());
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn build_result(
    current: &str,
    tag: &str,
    release_url: &str,
    release_notes: &str,
    published_at: &str,
    source: UpdateCheckSource,
) -> Result<UpdateCheckResult, String> {
    let current_version = Version::parse(current)?;
    let latest_version_text = tag.trim().strip_prefix('v').unwrap_or(tag.trim());
    let latest_version = Version::parse(latest_version_text)?;
    if latest_version.has_prerelease() {
        return Err("最新 Release 不是稳定版本。".to_string());
    }
    if !is_trusted_release_url(release_url, tag) {
        return Err("GitHub Release 地址无效。".to_string());
    }
    Ok(UpdateCheckResult {
        current_version: current.to_string(),
        latest_version: latest_version_text.to_string(),
        tag: tag.to_string(),
        available: latest_version.cmp(&current_version) == Ordering::Greater,
        release_url: release_url.to_string(),
        release_notes: release_notes
            .chars()
            .take(MAX_RELEASE_NOTES_CHARS)
            .collect(),
        published_at: published_at.to_string(),
        source,
    })
}

fn is_trusted_release_url(value: &str, tag: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.path() == format!("/{REPOSITORY}/releases/tag/{tag}")
}

impl Version {
    fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
        let value = value.split_once('+').map_or(value, |(main, _)| main);
        let (core, prerelease) = value
            .split_once('-')
            .map_or((value, None), |(core, prerelease)| (core, Some(prerelease)));
        let parts = core.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(format!("版本号格式无效：{value}"));
        }
        let parse_number = |part: &str| {
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(format!("版本号格式无效：{value}"));
            }
            part.parse::<u64>()
                .map_err(|_| format!("版本号超出范围：{value}"))
        };
        let prerelease = prerelease
            .map(|value| {
                if value.is_empty() {
                    return Err(format!("版本号格式无效：{value}"));
                }
                value
                    .split('.')
                    .map(|part| {
                        if part.is_empty()
                            || !part
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                        {
                            return Err("预发布版本号格式无效。".to_string());
                        }
                        if part.bytes().all(|byte| byte.is_ascii_digit()) {
                            if part.len() > 1 && part.starts_with('0') {
                                return Err("预发布数字标识不能包含前导零。".to_string());
                            }
                            Ok(PrereleasePart::Number(
                                part.parse::<u64>()
                                    .map_err(|_| "预发布版本号超出范围。".to_string())?,
                            ))
                        } else {
                            Ok(PrereleasePart::Text(part.to_ascii_lowercase()))
                        }
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            major: parse_number(parts[0])?,
            minor: parse_number(parts[1])?,
            patch: parse_number(parts[2])?,
            prerelease,
        })
    }

    fn has_prerelease(&self) -> bool {
        !self.prerelease.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| compare_prerelease(&self.prerelease, &other.prerelease))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prerelease(left: &[PrereleasePart], right: &[PrereleasePart]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for (left, right) in left.iter().zip(right) {
        let ordering = match (left, right) {
            (PrereleasePart::Number(left), PrereleasePart::Number(right)) => left.cmp(right),
            (PrereleasePart::Number(_), PrereleasePart::Text(_)) => Ordering::Less,
            (PrereleasePart::Text(_), PrereleasePart::Number(_)) => Ordering::Greater,
            (PrereleasePart::Text(left), PrereleasePart::Text(right)) => left.cmp(right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{build_result, is_trusted_release_url, UpdateCheckSource, Version};

    #[test]
    fn compares_semver_and_prerelease_correctly() {
        assert_eq!(
            Version::parse("1.2.4")
                .unwrap()
                .cmp(&Version::parse("1.2.3").unwrap()),
            Ordering::Greater
        );
        assert_eq!(
            Version::parse("1.0.0-rc.1")
                .unwrap()
                .cmp(&Version::parse("1.0.0").unwrap()),
            Ordering::Less
        );
        assert_eq!(
            Version::parse("1.0.0-beta.11")
                .unwrap()
                .cmp(&Version::parse("1.0.0-beta.2").unwrap()),
            Ordering::Greater
        );
    }

    #[test]
    fn rejects_invalid_versions() {
        for value in ["", "1", "1.2", "1.2.3.4", "01.2.3", "1.2.x", "1.2.3-"] {
            assert!(Version::parse(value).is_err(), "value={value}");
        }
    }

    #[test]
    fn accepts_only_the_fixed_repository_release_url() {
        assert!(is_trusted_release_url(
            "https://github.com/tushanmiao/Mnemora/releases/tag/v0.1.5",
            "v0.1.5"
        ));
        assert!(!is_trusted_release_url(
            "https://example.com/tushanmiao/Mnemora/releases/tag/v0.1.5",
            "v0.1.5"
        ));
    }

    #[test]
    fn builds_bounded_stable_update_result() {
        let result = build_result(
            "0.1.4",
            "v0.1.5",
            "https://github.com/tushanmiao/Mnemora/releases/tag/v0.1.5",
            "更新说明",
            "2026-08-01T00:00:00Z",
            UpdateCheckSource::GitHubApi,
        )
        .unwrap();
        assert!(result.available);
        assert_eq!(result.latest_version, "0.1.5");
    }
}
