//! 手动笔记同步编排。
//!
//! 服务逐篇读取笔记、文献和批注，完成一篇后立即释放正文；映射只在任务开始时读取、
//! 结束时写回。同一时间只允许一个任务，由 `AppState::sync_operations` 负责串行化。

use reqwest::Client;
use zeroize::Zeroizing;

use crate::library::LibraryRepository;

use super::{
    mapping::{content_hash, now_millis, SyncMapping, SyncMappingRepository},
    markdown::render_document,
    notion, obsidian,
    secrets::SyncSecretStore,
    types::{SyncItemResult, SyncRequest, SyncResult, SyncSettings, SyncTarget},
};

const MAX_RESULT_ITEMS: usize = 100;

pub async fn run(
    http: Client,
    library_repository: LibraryRepository,
    mapping_repository: SyncMappingRepository,
    secret_store: SyncSecretStore,
    settings: SyncSettings,
    request: SyncRequest,
) -> Result<SyncResult, String> {
    if !settings.enabled {
        return Err("请先在同步设置中启用笔记同步。".to_string());
    }
    let target = settings.target;
    validate_target_settings(&settings)?;

    let notion_token = if target == SyncTarget::Notion {
        let store = secret_store;
        Some(Zeroizing::new(
            tokio::task::spawn_blocking(move || store.get_notion_token())
                .await
                .map_err(join_error)??
                .ok_or_else(|| "请先保存 Notion Integration Token。".to_string())?,
        ))
    } else {
        None
    };

    let note_ids = if let Some(note_id) = request.note_id {
        vec![note_id]
    } else {
        let repository = library_repository.clone();
        tokio::task::spawn_blocking(move || repository.list_note_ids())
            .await
            .map_err(join_error)??
    };
    let repository = mapping_repository.clone();
    let mut mappings = tokio::task::spawn_blocking(move || repository.load_store())
        .await
        .map_err(join_error)??;

    let mut result = SyncResult {
        target,
        attempted: note_ids.len(),
        succeeded: 0,
        skipped: 0,
        failed: 0,
        items: Vec::with_capacity(note_ids.len().min(MAX_RESULT_ITEMS)),
    };
    let mut mappings_changed = false;

    for note_id in note_ids {
        let repository = library_repository.clone();
        let include_annotations = settings.include_annotations;
        let note_id_to_load = note_id.clone();
        let loaded = tokio::task::spawn_blocking(move || {
            let note = repository.get_note(&note_id_to_load)?;
            let item = repository.get_item(&note.item_id)?;
            let annotations = if include_annotations {
                repository.list_annotations(&note.item_id)?
            } else {
                Vec::new()
            };
            Ok::<_, String>((note, item, annotations))
        })
        .await
        .map_err(join_error)?;

        let (note, item, annotations) = match loaded {
            Ok(value) => value,
            Err(error) => {
                result.failed += 1;
                push_item(
                    &mut result,
                    SyncItemResult {
                        note_id,
                        title: String::new(),
                        status: "failed".to_string(),
                        message: error,
                    },
                );
                continue;
            }
        };
        let document = render_document(
            &note,
            &item,
            &annotations,
            settings.include_metadata,
            settings.include_annotations,
        );
        let hash = content_hash(&document.markdown);
        let existing = mappings.get(target, &document.note_id).cloned();
        if existing
            .as_ref()
            .is_some_and(|mapping| mapping.content_hash == hash)
        {
            result.skipped += 1;
            push_item(
                &mut result,
                SyncItemResult {
                    note_id: document.note_id,
                    title: document.title,
                    status: "skipped".to_string(),
                    message: "内容没有变化。".to_string(),
                },
            );
            continue;
        }

        let remote_id = match target {
            SyncTarget::Obsidian => {
                let obsidian_settings = settings.obsidian.clone();
                let mapped_path = existing.map(|mapping| mapping.remote_id);
                let document = document;
                tokio::task::spawn_blocking(move || {
                    obsidian::sync_document(&obsidian_settings, &document, mapped_path.as_deref())
                })
                .await
                .map_err(join_error)?
            }
            SyncTarget::Notion => {
                notion::sync_document(
                    &http,
                    &settings.notion,
                    notion_token
                        .as_deref()
                        .ok_or_else(|| "Notion Integration Token 不可用。".to_string())?,
                    &document,
                    existing.as_ref().map(|mapping| mapping.remote_id.as_str()),
                )
                .await
            }
        };

        match remote_id {
            Ok(remote_id) => {
                mappings.insert(SyncMapping {
                    target,
                    note_id: note.id.clone(),
                    remote_id,
                    content_hash: hash,
                    synced_at: now_millis(),
                });
                mappings_changed = true;
                result.succeeded += 1;
                push_item(
                    &mut result,
                    SyncItemResult {
                        note_id: note.id,
                        title: note.title,
                        status: "succeeded".to_string(),
                        message: "同步完成。".to_string(),
                    },
                );
            }
            Err(error) => {
                result.failed += 1;
                push_item(
                    &mut result,
                    SyncItemResult {
                        note_id: note.id,
                        title: note.title,
                        status: "failed".to_string(),
                        message: error,
                    },
                );
            }
        }
    }

    if mappings_changed {
        tokio::task::spawn_blocking(move || mapping_repository.save_store(&mappings))
            .await
            .map_err(join_error)??;
    }
    Ok(result)
}

fn validate_target_settings(settings: &SyncSettings) -> Result<(), String> {
    match settings.target {
        SyncTarget::Obsidian if settings.obsidian.vault_path.trim().is_empty() => {
            Err("请先选择 Obsidian Vault。".to_string())
        }
        SyncTarget::Notion if settings.notion.parent_page_id.trim().is_empty() => {
            Err("请先填写 Notion 父页面 ID。".to_string())
        }
        _ => Ok(()),
    }
}

fn push_item(result: &mut SyncResult, item: SyncItemResult) {
    if result.items.len() < MAX_RESULT_ITEMS {
        result.items.push(item);
    }
}

fn join_error(error: tokio::task::JoinError) -> String {
    format!("同步后台任务失败：{error}")
}
