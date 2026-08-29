//! 远端资源包命令：搜索 / 取回预览 / 确认安装。
//!
//! 安装一律复用既有安装器（skills_import / plugins_install / 宠物安装器），
//! 因此现有的哈希校验、路径防护、容量上限与「拒绝可执行 stdio MCP」全部生效。

use std::{fs, path::PathBuf};

use tauri::{async_runtime, State};

use crate::{
    packages::{
        github::{download_zipball, search_repositories, validate_full_name, validate_git_ref},
        staging::stage_download,
        types::{
            RemoteFetchRequest, RemoteInstallRequest, RemotePackageKind, RemotePackagePreview,
            RemoteSearchRequest, RemoteSearchResult,
        },
    },
    plugins::PluginInstallRequest,
    skills::types::{SkillImportKind, SkillImportRequest},
    state::AppState,
};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Package background task failed: {error}")
}

#[tauri::command]
pub async fn packages_search_remote(
    state: State<'_, AppState>,
    request: RemoteSearchRequest,
) -> Result<RemoteSearchResult, String> {
    search_repositories(&state.http, request.kind, &request.query).await
}

/// 下载并解析，但**不安装**。返回的预览用于让用户看清将要装什么。
#[tauri::command]
pub async fn packages_fetch_remote(
    state: State<'_, AppState>,
    request: RemoteFetchRequest,
) -> Result<RemotePackagePreview, String> {
    let (owner, repo) = validate_full_name(&request.full_name)?;
    let git_ref = match request.git_ref.as_deref() {
        Some(value) => validate_git_ref(value)?,
        None => "HEAD".to_string(),
    };

    state.package_staging.sweep();

    let (bytes, commit_sha) = download_zipball(&state.http, &owner, &repo, &git_ref).await?;

    // 已安装 id 用于判断这次是否覆盖；覆盖必须让用户在确认时看到。
    let installed_ids = collect_installed_ids(&state, request.kind).await?;

    let downloads_dir = state.package_downloads_dir.clone();
    fs::create_dir_all(&downloads_dir)
        .map_err(|error| format!("创建下载目录失败：{error}"))?;

    let full_name = format!("{owner}/{repo}");
    let source_url = format!("https://github.com/{full_name}");
    let kind = request.kind;
    let staged = async_runtime::spawn_blocking(move || {
        stage_download(
            &downloads_dir,
            kind,
            &full_name,
            &commit_sha,
            &source_url,
            &bytes,
            &installed_ids,
        )
    })
    .await
    .map_err(join_error)??;

    let preview = staged.preview.clone();
    state
        .package_staging
        .insert(preview.staging_token.clone(), staged.entry);
    Ok(preview)
}

/// 确认安装。token 一次性消费，避免重放。
#[tauri::command]
pub async fn packages_install_remote(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: RemoteInstallRequest,
) -> Result<String, String> {
    let entry = state
        .package_staging
        .take(&request.staging_token)
        .ok_or_else(|| "暂存内容已过期，请重新获取后再安装。".to_string())?;

    // 预览时判定为覆盖，确认时却没带 replaceExisting，说明用户看到的
    // 和即将执行的不一致；宁可让他重新走一遍，也不静默覆盖。
    if entry.replaces_existing && !request.replace_existing {
        let _ = fs::remove_dir_all(&entry.path);
        return Err("该资源包已安装。如需覆盖，请在确认时选择替换。".to_string());
    }

    let result = install_staged(&app, &state, &entry, request.replace_existing).await;
    // 无论成败都清掉暂存物，不在数据目录里留下载残留。
    let _ = fs::remove_dir_all(staging_root_of(&entry.path));
    result
}

/// 暂存根目录是 token 目录；`entry.path` 可能指向被剥掉包装的子目录。
fn staging_root_of(package_path: &PathBuf) -> PathBuf {
    package_path
        .parent()
        .filter(|parent| {
            parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("stg-"))
        })
        .map(|parent| parent.to_path_buf())
        .unwrap_or_else(|| package_path.clone())
}

async fn collect_installed_ids(
    state: &State<'_, AppState>,
    kind: RemotePackageKind,
) -> Result<Vec<String>, String> {
    match kind {
        RemotePackageKind::Plugin => {
            let manager = state.plugin_manager.clone();
            let overview = async_runtime::spawn_blocking(move || manager.list())
                .await
                .map_err(join_error)??;
            Ok(overview.plugins.into_iter().map(|item| item.id).collect())
        }
        RemotePackageKind::Skill => {
            let repository = state.skill_repository.clone();
            let listing = async_runtime::spawn_blocking(move || repository.list())
                .await
                .map_err(join_error)??;
            Ok(listing.skills.into_iter().map(|item| item.id).collect())
        }
        // 宠物没有集中的「已安装 id」查询接口；重名由宠物安装器自己处理。
        RemotePackageKind::Pet => Ok(Vec::new()),
    }
}

async fn install_staged(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    entry: &crate::packages::staging::StagingEntry,
    replace_existing: bool,
) -> Result<String, String> {
    let path = entry.path.clone();
    match entry.kind {
        RemotePackageKind::Plugin => {
            let _guard = state.plugin_operations.lock().await;
            let manager = state.plugin_manager.clone();
            let summary = async_runtime::spawn_blocking(move || {
                manager.install(
                    &path,
                    PluginInstallRequest {
                        kind: SkillImportKind::Directory,
                        replace_existing,
                        // 与本地安装保持同一姿态：签名验证尚未接入可信发布者目录，
                        // 因此这里不假装验证通过，而是由 UI 明确告知用户未验证。
                        allow_unsigned: true,
                    },
                )
            })
            .await
            .map_err(join_error)??;
            Ok(format!(
                "插件“{}”已从 {} 安装（commit {}），当前保持停用。",
                summary.name,
                entry.full_name,
                short_sha(&entry.commit_sha)
            ))
        }
        RemotePackageKind::Skill => {
            let _guard = state.skill_operations.lock().await;
            let repository = state.skill_repository.clone();
            let result = async_runtime::spawn_blocking(move || {
                repository.import(SkillImportRequest {
                    path: path.to_string_lossy().to_string(),
                    kind: SkillImportKind::Directory,
                    replace_existing,
                })
            })
            .await
            .map_err(join_error)??;
            Ok(format!(
                "技能“{}”已从 {} 安装（commit {}）。",
                result.skill.name,
                entry.full_name,
                short_sha(&entry.commit_sha)
            ))
        }
        RemotePackageKind::Pet => {
            let installed =
                crate::commands::pet::install_pet_from_directory(app, state, &entry.path)?;
            Ok(format!(
                "宠物“{}”已从 {} 安装（commit {}）。",
                installed,
                entry.full_name,
                short_sha(&entry.commit_sha)
            ))
        }
    }
}

fn short_sha(value: &str) -> String {
    value.chars().take(7).collect()
}
