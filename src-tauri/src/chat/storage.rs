//! Chat 本地 JSON 仓库。
//!
//! 完整会话分别写入 `conversations/conv_<id>.json`，`index.json` 只保存侧边栏摘要。
//! 写入使用同目录临时文件和备份替换；索引缺失、损坏或文件集合不一致时会扫描重建，
//! 单个损坏会话只会被跳过，不阻塞其他会话。

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::conversation_types::{
    validate_conversation_id, ConversationListItem, StoredConversation,
};

const INDEX_VERSION: u32 = 1;
const INDEX_FILE_NAME: &str = "index.json";
const MAX_CONVERSATION_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationIndex {
    version: u32,
    conversations: Vec<ConversationListItem>,
}

#[derive(Clone)]
pub struct ConversationRepository {
    directory: PathBuf,
}

impl ConversationRepository {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            directory: app_data_dir.join("conversations"),
        }
    }

    pub fn list(&self) -> Result<Vec<ConversationListItem>, String> {
        self.ensure_directory()?;
        let mut index = match self.read_valid_index() {
            Ok(index) if self.index_matches_files(&index)? => index,
            _ => self.rebuild_index()?,
        };
        sort_items(&mut index.conversations);
        Ok(index.conversations)
    }

    pub fn load(&self, conversation_id: &str) -> Result<StoredConversation, String> {
        validate_conversation_id("Conversation ID", conversation_id)?;
        let path = self.conversation_path(conversation_id);
        read_conversation(&path)
    }

    pub fn save(&self, conversation: &StoredConversation) -> Result<ConversationListItem, String> {
        conversation.validate()?;
        self.ensure_directory()?;
        let path = self.conversation_path(&conversation.id);
        write_json_atomic(&path, conversation)?;

        let item = conversation.to_list_item();
        let mut items = self.list()?;
        if let Some(existing) = items.iter_mut().find(|existing| existing.id == item.id) {
            *existing = item.clone();
        } else {
            items.push(item.clone());
        }
        sort_items(&mut items);
        if let Err(error) = self.write_index(items) {
            let _ = fs::remove_file(self.index_path());
            return Err(error);
        }
        Ok(item)
    }

    pub fn delete(&self, conversation_id: &str) -> Result<bool, String> {
        validate_conversation_id("Conversation ID", conversation_id)?;
        self.ensure_directory()?;
        let path = self.conversation_path(conversation_id);
        let existed = path.exists();
        if existed {
            fs::remove_file(&path)
                .map_err(|error| format!("Failed to delete conversation file: {error}"))?;
        }
        let items = self
            .list()?
            .into_iter()
            .filter(|item| item.id != conversation_id)
            .collect();
        if let Err(error) = self.write_index(items) {
            let _ = fs::remove_file(self.index_path());
            return Err(error);
        }
        Ok(existed)
    }

    pub fn clear(&self) -> Result<(), String> {
        self.ensure_directory()?;
        for entry in fs::read_dir(&self.directory)
            .map_err(|error| format!("Failed to read conversations directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("Failed to inspect conversation file: {error}"))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name == INDEX_FILE_NAME
                || (file_name.starts_with("conv_") && file_name.ends_with(".json"))
            {
                fs::remove_file(entry.path())
                    .map_err(|error| format!("Failed to clear conversation file: {error}"))?;
            }
        }
        self.write_index(Vec::new())
    }

    fn ensure_directory(&self) -> Result<(), String> {
        fs::create_dir_all(&self.directory)
            .map_err(|error| format!("Failed to create conversations directory: {error}"))
    }

    fn conversation_path(&self, conversation_id: &str) -> PathBuf {
        self.directory.join(format!("conv_{conversation_id}.json"))
    }

    fn index_path(&self) -> PathBuf {
        self.directory.join(INDEX_FILE_NAME)
    }

    fn read_valid_index(&self) -> Result<ConversationIndex, String> {
        let raw = fs::read(self.index_path())
            .map_err(|error| format!("Failed to read conversation index: {error}"))?;
        let index: ConversationIndex = serde_json::from_slice(&raw)
            .map_err(|error| format!("Failed to parse conversation index: {error}"))?;
        if index.version > INDEX_VERSION {
            return Err("Conversation index version is newer than this app".to_string());
        }
        let mut seen = HashSet::new();
        for item in &index.conversations {
            validate_conversation_id("Conversation ID", &item.id)?;
            if !seen.insert(item.id.as_str()) {
                return Err("Conversation index contains duplicate IDs".to_string());
            }
        }
        Ok(index)
    }

    fn index_matches_files(&self, index: &ConversationIndex) -> Result<bool, String> {
        let indexed = index
            .conversations
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        let files = self.conversation_file_ids()?;
        Ok(indexed.len() == files.len() && files.iter().all(|id| indexed.contains(id.as_str())))
    }

    fn conversation_file_ids(&self) -> Result<Vec<String>, String> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(&self.directory)
            .map_err(|error| format!("Failed to read conversations directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("Failed to inspect conversation file: {error}"))?;
            if !entry
                .file_type()
                .map_err(|error| format!("Failed to inspect conversation file type: {error}"))?
                .is_file()
            {
                continue;
            }
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if let Some(id) = file_name
                .strip_prefix("conv_")
                .and_then(|name| name.strip_suffix(".json"))
            {
                if validate_conversation_id("Conversation ID", id).is_ok() {
                    ids.push(id.to_string());
                }
            }
        }
        Ok(ids)
    }

    fn rebuild_index(&self) -> Result<ConversationIndex, String> {
        let mut conversations = Vec::new();
        for id in self.conversation_file_ids()? {
            let path = self.conversation_path(&id);
            match read_conversation(&path) {
                Ok(conversation) => conversations.push(conversation.to_list_item()),
                Err(error) => {
                    eprintln!("Skipping invalid conversation {id}: {error}");
                    quarantine_invalid_file(&path);
                }
            }
        }
        sort_items(&mut conversations);
        self.write_index(conversations.clone())?;
        Ok(ConversationIndex {
            version: INDEX_VERSION,
            conversations,
        })
    }

    fn write_index(&self, conversations: Vec<ConversationListItem>) -> Result<(), String> {
        write_json_atomic(
            &self.index_path(),
            &ConversationIndex {
                version: INDEX_VERSION,
                conversations,
            },
        )
    }
}

fn quarantine_invalid_file(path: &Path) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let quarantine = path.with_extension(format!("json.corrupt-{timestamp}"));
    if let Err(error) = fs::rename(path, quarantine) {
        eprintln!("Failed to quarantine invalid conversation file: {error}");
    }
}

fn read_conversation(path: &Path) -> Result<StoredConversation, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to inspect conversation file: {error}"))?;
    if metadata.len() > MAX_CONVERSATION_FILE_BYTES {
        return Err("Conversation file is too large".to_string());
    }
    let raw = fs::read(path).map_err(|error| format!("Failed to read conversation: {error}"))?;
    let conversation: StoredConversation = serde_json::from_slice(&raw)
        .map_err(|error| format!("Failed to parse conversation: {error}"))?;
    conversation.validate()?;
    Ok(conversation)
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "JSON path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create JSON directory: {error}"))?;
    let json = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to serialize JSON file: {error}"))?;
    let temporary = path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let backup = path.with_extension("json.bak");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("Failed to create temporary JSON file: {error}"))?;
    file.write_all(&json)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Failed to write JSON file: {error}"))?;
    drop(file);

    if path.exists() {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Failed to remove stale JSON backup: {error}"))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("Failed to back up JSON file: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temporary);
        return Err(format!("Failed to replace JSON file: {error}"));
    }
    let _ = fs::remove_file(backup);
    Ok(())
}

fn sort_items(items: &mut [ConversationListItem]) {
    items.sort_by(|left, right| {
        right
            .pinned
            .cmp(&left.pinned)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::ConversationRepository;
    use crate::chat::conversation_types::{AiPermissionMode, StoredConversation};

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mnemora-conversations-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn conversation(id: &str, updated_at: u64) -> StoredConversation {
        StoredConversation {
            id: id.to_string(),
            title: format!("Conversation {id}"),
            messages: Vec::new(),
            assistant_id: None,
            provider_id: None,
            model_id: None,
            system_prompt: String::new(),
            context_summary: String::new(),
            compressed_until_message_id: None,
            context_compression_count: 0,
            permission_mode: AiPermissionMode::AskSensitive,
            project_id: None,
            collection_id: None,
            pinned: false,
            created_at: updated_at,
            updated_at,
        }
    }

    #[test]
    fn saves_lists_loads_and_deletes_conversations() {
        let directory = test_directory("roundtrip");
        let repository = ConversationRepository::new(directory.clone());
        repository
            .save(&conversation("conversation-1", 10))
            .unwrap();
        repository
            .save(&conversation("conversation-2", 20))
            .unwrap();

        let items = repository.list().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "conversation-2");
        assert_eq!(repository.load("conversation-1").unwrap().updated_at, 10);
        assert!(repository.delete("conversation-1").unwrap());
        assert_eq!(repository.list().unwrap().len(), 1);

        repository.clear().unwrap();
        assert!(repository.list().unwrap().is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rebuild_skips_one_corrupt_conversation_file() {
        let directory = test_directory("corrupt");
        let repository = ConversationRepository::new(directory.clone());
        repository
            .save(&conversation("conversation-valid", 10))
            .unwrap();
        fs::write(
            directory
                .join("conversations")
                .join("conv_conversation-bad.json"),
            b"{not-json",
        )
        .unwrap();

        let items = repository.list().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "conversation-valid");
        let _ = fs::remove_dir_all(directory);
    }
}
