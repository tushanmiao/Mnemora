//! Obsidian Vault 文件适配器。只在同步时创建目录并原子写入 Markdown。

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{mapping::replace_file, markdown::SyncDocument, types::ObsidianSettings};

pub fn sync_document(
    settings: &ObsidianSettings,
    document: &SyncDocument,
    mapped_relative_path: Option<&str>,
) -> Result<String, String> {
    let vault = validate_vault(&settings.vault_path)?;
    let target_directory = if settings.directory.is_empty() {
        vault.clone()
    } else {
        vault.join(&settings.directory)
    };
    fs::create_dir_all(&target_directory)
        .map_err(|error| format!("创建 Obsidian 同步目录失败：{error}"))?;
    let target_directory = target_directory
        .canonicalize()
        .map_err(|error| format!("读取 Obsidian 同步目录失败：{error}"))?;
    if !target_directory.starts_with(&vault) {
        return Err("Obsidian 同步目录超出了 Vault。".to_string());
    }

    let relative = mapped_relative_path
        .and_then(validate_mapped_path)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let file_name = format!(
                "{}--{}.md",
                safe_file_name(&document.title),
                short_id(&document.note_id)
            );
            if settings.directory.is_empty() {
                PathBuf::from(file_name)
            } else {
                PathBuf::from(&settings.directory).join(file_name)
            }
        });
    let target = vault.join(&relative);
    let target_parent = target
        .parent()
        .ok_or_else(|| "Obsidian 同步文件路径无效。".to_string())?;
    fs::create_dir_all(target_parent)
        .map_err(|error| format!("创建 Obsidian 笔记目录失败：{error}"))?;
    let canonical_parent = target_parent
        .canonicalize()
        .map_err(|error| format!("读取 Obsidian 笔记目录失败：{error}"))?;
    if !canonical_parent.starts_with(&vault) {
        return Err("Obsidian 同步文件超出了 Vault。".to_string());
    }
    let temporary = target.with_extension(format!("md.tmp-{}", std::process::id()));
    fs::write(&temporary, document.markdown.as_bytes())
        .map_err(|error| format!("写入 Obsidian 临时笔记失败：{error}"))?;
    replace_file(&temporary, &target, "Obsidian 笔记")?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn validate_vault(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty() {
        return Err("请先选择 Obsidian Vault。".to_string());
    }
    let path = Path::new(value);
    if !path.is_dir() {
        return Err("Obsidian Vault 不存在或不是文件夹。".to_string());
    }
    path.canonicalize()
        .map_err(|error| format!("读取 Obsidian Vault 失败：{error}"))
}

fn validate_mapped_path(value: &str) -> Option<&str> {
    let path = Path::new(value);
    (!path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md")))
    .then_some(value)
}

fn safe_file_name(value: &str) -> String {
    let mut result = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    result = result.trim().trim_end_matches(['.', ' ']).to_string();
    if result.is_empty() {
        "Mnemora 笔记".to_string()
    } else {
        result.chars().take(120).collect()
    }
}

fn short_id(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::safe_file_name;

    #[test]
    fn file_names_remove_windows_reserved_characters() {
        assert_eq!(safe_file_name("A:B/C?"), "A_B_C_");
    }
}
