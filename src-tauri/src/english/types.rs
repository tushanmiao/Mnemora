use serde::{Deserialize, Serialize};

pub const ENGLISH_SOURCE_URL: &str = "https://isdc.pages.dev/";
pub const ENGLISH_SOURCE_NAME: &str = "雅思词典（isdc.pages.dev）";
pub const ENGLISH_BACKUP_URL: &str = "https://raw.githubusercontent.com/tushanmiao/Mnemora/main/src-tauri/resources/english/isdc-asp-data.txt";
pub const ENGLISH_BACKUP_RESOURCE: &str = "english/isdc-asp-data.txt";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishDictionaryStatus {
    pub installed: bool,
    pub source_name: String,
    pub source_url: String,
    pub word_count: usize,
    pub downloaded_at: Option<u64>,
    pub data_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishDownloadProgress {
    pub phase: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub indexed_words: usize,
    pub total_words: usize,
    pub progress: Option<u8>,
    pub finished: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishGroupSummary {
    pub id: u32,
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishWordSummary {
    pub id: u32,
    pub word: String,
    pub group_id: u32,
    pub group_name: String,
    pub pronunciation: String,
    pub occurrence: Option<u32>,
}

/// Rust 内部使用的索引条目。文件偏移只用于读取 JSONL，不通过 IPC 暴露给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnglishIndexEntry {
    pub id: u32,
    pub word: String,
    pub group_id: u32,
    pub pronunciation: String,
    pub occurrence: Option<u32>,
    pub file_offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishDerivedWord {
    pub word: String,
    pub definition: String,
    pub part_of_speech: String,
    pub word_formation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishExamExample {
    pub sentence: String,
    pub source: String,
    pub section: String,
    pub source_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishWordEntry {
    pub id: u32,
    pub word: String,
    pub group_id: u32,
    pub group_name: String,
    pub pronunciation: String,
    pub translation: String,
    pub example: String,
    pub example_translation: String,
    pub british_audio: String,
    pub american_audio: String,
    pub mnemonic: String,
    pub root_affixes: String,
    pub english_definition: String,
    pub derived_words: Vec<EnglishDerivedWord>,
    pub occurrence: Option<u32>,
    pub exam_examples: Vec<EnglishExamExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnglishSearchResult {
    pub items: Vec<EnglishWordSummary>,
    pub total: usize,
    pub groups: Vec<EnglishGroupSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnglishIndexFile {
    pub source_name: String,
    pub source_url: String,
    pub downloaded_at: u64,
    pub word_count: usize,
    pub groups: Vec<EnglishGroupSummary>,
    pub entries: Vec<EnglishIndexEntry>,
}
