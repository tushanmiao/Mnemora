use serde::{Deserialize, Serialize};

/// 可从 GitHub 安装的资源包类型。宠物走各自的安装器，但发现流程共用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemotePackageKind {
    Skill,
    Plugin,
    Pet,
}

/// 搜索结果里的单个候选仓库。
///
/// 只承载「让人做判断」所需的信号：星数、最近更新、归档状态、许可证。
/// 这些都来自 GitHub 搜索 API 本身，不做任何额外抓取或推断。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCandidate {
    /// owner/repo
    pub full_name: String,
    pub owner: String,
    pub description: String,
    pub html_url: String,
    pub stars: u64,
    /// ISO8601；仓库最近一次 push
    pub pushed_at: String,
    pub archived: bool,
    pub license: Option<String>,
    pub default_branch: String,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSearchResult {
    pub candidates: Vec<RemoteCandidate>,
    /// 命中总数可能远大于返回条数，用于提示用户缩小关键词。
    pub total_count: u64,
    pub truncated: bool,
}

/// 下载并解析后、安装**之前**呈现给用户的清单。
///
/// 这是本设计的关键一环：用户在看到真实清单内容和权限声明之后
/// 才决定是否安装，而不是凭仓库名字决定。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePackagePreview {
    /// 服务端持有的暂存句柄；确认安装时回传，前端不接触真实路径。
    pub staging_token: String,
    pub kind: RemotePackageKind,
    pub full_name: String,
    /// 实际下载到的 commit，写入来源记录用于审计。
    pub commit_sha: String,
    pub source_url: String,
    /// 仓库中实际选中的包目录。空字符串表示仓库根目录。
    pub package_path: String,
    /// 清单里声明的身份信息
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub publisher: String,
    /// 插件专有：声明贡献的 Skill 数与远程 MCP 服务器
    pub skill_count: usize,
    pub mcp_server_ids: Vec<String>,
    pub network_domains: Vec<String>,
    pub secrets: Vec<String>,
    /// 这个包是否已经安装过（同 id）
    pub replaces_existing: bool,
    /// 解析过程中发现的、需要用户注意但不阻断安装的问题
    pub warnings: Vec<String>,
    pub total_bytes: u64,
    pub file_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSearchRequest {
    pub kind: RemotePackageKind,
    pub query: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFetchRequest {
    pub kind: RemotePackageKind,
    /// owner/repo，由用户从候选中指定；不接受任意 URL。
    pub full_name: String,
    /// 可选 ref（分支或 tag）；缺省用默认分支。
    #[serde(default)]
    pub git_ref: Option<String>,
    /// 可选的仓库内目录。GitHub tree/blob URL 会解析为这个字段。
    #[serde(default)]
    pub package_path: Option<String>,
    /// 从名称搜索进入时用于在多 Skill 仓库里挑选匹配目录。
    #[serde(default)]
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInstallRequest {
    pub staging_token: String,
    /// 必须与预览时的 replacesExisting 一致才允许覆盖，避免过期确认。
    #[serde(default)]
    pub replace_existing: bool,
}
