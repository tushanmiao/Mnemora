use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateCheckSource {
    GitHubApi,
    GitHubWeb,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub tag: String,
    pub available: bool,
    pub release_url: String,
    pub release_notes: String,
    pub published_at: String,
    pub source: UpdateCheckSource,
}
