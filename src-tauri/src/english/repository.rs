use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use brotli::Decompressor;
use reqwest::StatusCode;
use serde_json::Value;

use super::types::{
    EnglishDerivedWord, EnglishDictionaryStatus, EnglishExamExample, EnglishGroupSummary,
    EnglishIndexEntry, EnglishIndexFile, EnglishSearchResult, EnglishWordEntry, EnglishWordSummary,
    ENGLISH_SOURCE_NAME, ENGLISH_SOURCE_URL,
};

const INDEX_FILE: &str = "index.json";
const ENTRIES_FILE: &str = "entries.jsonl";
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct EnglishRepository {
    root: PathBuf,
    cached_index: Arc<RwLock<Option<Arc<EnglishIndexFile>>>>,
}

impl EnglishRepository {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            root: app_data_dir.join("english").join("dictionary"),
            cached_index: Arc::new(RwLock::new(None)),
        }
    }

    pub fn status(&self) -> Result<EnglishDictionaryStatus, String> {
        let index_path = self.root.join(INDEX_FILE);
        if !index_path.is_file() {
            return Ok(EnglishDictionaryStatus {
                installed: false,
                source_name: ENGLISH_SOURCE_NAME.to_string(),
                source_url: ENGLISH_SOURCE_URL.to_string(),
                word_count: 0,
                downloaded_at: None,
                data_size_bytes: 0,
            });
        }
        let index = self.load_index()?;
        let data_size_bytes = fs::metadata(&index_path)
            .map(|meta| meta.len())
            .unwrap_or(0)
            + fs::metadata(self.root.join(ENTRIES_FILE))
                .map(|meta| meta.len())
                .unwrap_or(0);
        Ok(EnglishDictionaryStatus {
            installed: true,
            source_name: index.source_name.clone(),
            source_url: index.source_url.clone(),
            word_count: index.word_count,
            downloaded_at: Some(index.downloaded_at),
            data_size_bytes,
        })
    }

    pub fn search(
        &self,
        query: &str,
        group_id: Option<u32>,
        limit: usize,
    ) -> Result<EnglishSearchResult, String> {
        let index = self.load_index()?;
        let needle = query.trim().as_bytes();
        let limit = limit.clamp(1, MAX_SEARCH_RESULTS);
        let mut total = 0usize;
        let mut items = Vec::with_capacity(limit);
        for entry in &index.entries {
            let matches = group_id.is_none_or(|id| entry.group_id == id)
                && (needle.is_empty() || contains_ascii_case_insensitive(&entry.word, needle));
            if !matches {
                continue;
            }
            total += 1;
            if items.len() < limit {
                items.push(index_entry_summary(entry, &index.groups));
            }
        }
        Ok(EnglishSearchResult {
            items,
            total,
            groups: index.groups.clone(),
        })
    }

    pub fn get_entry(&self, id: u32) -> Result<EnglishWordEntry, String> {
        let index = self.load_index()?;
        let summary = index
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| "English word not found".to_string())?;
        let mut file = File::open(self.root.join(ENTRIES_FILE))
            .map_err(|error| format!("Open English dictionary entries failed: {error}"))?;
        file.seek(SeekFrom::Start(summary.file_offset))
            .map_err(|error| format!("Seek English dictionary entry failed: {error}"))?;
        let mut line = String::new();
        BufReader::new(file)
            .read_line(&mut line)
            .map_err(|error| format!("Read English dictionary entry failed: {error}"))?;
        serde_json::from_str(line.trim())
            .map_err(|error| format!("Parse English word failed: {error}"))
    }

    pub fn delete(&self) -> Result<(), String> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)
                .map_err(|error| format!("Delete English dictionary failed: {error}"))?;
        }
        self.clear_cache();
        Ok(())
    }

    pub fn install_payload(&self, payload: Vec<u8>) -> Result<EnglishDictionaryStatus, String> {
        if payload.len() > MAX_SOURCE_BYTES {
            return Err("English dictionary source is too large".to_string());
        }
        let source_data = decode_source_html(payload)?;
        let temp_root = self.root.with_extension("download");
        if temp_root.exists() {
            fs::remove_dir_all(&temp_root)
                .map_err(|error| format!("Clear English temporary directory failed: {error}"))?;
        }
        fs::create_dir_all(&temp_root)
            .map_err(|error| format!("Create English dictionary directory failed: {error}"))?;
        let entries_path = temp_root.join(ENTRIES_FILE);
        let mut entries_file = File::create(&entries_path)
            .map_err(|error| format!("Create English entries failed: {error}"))?;
        let mut summaries = Vec::new();
        let mut groups = Vec::new();
        let mut next_id = 0u32;
        let group_values = source_data
            .get("g")
            .and_then(Value::as_array)
            .ok_or_else(|| "English dictionary groups are missing".to_string())?;
        let pools = source_data
            .get("p")
            .ok_or_else(|| "English dictionary pools are missing".to_string())?;
        let exam_pools = source_data.get("d").unwrap_or(&Value::Null);
        for (group_id, group) in group_values.iter().enumerate() {
            let group_name =
                string_value(group.get("n")).unwrap_or_else(|| format!("Group {}", group_id + 1));
            let words = group
                .get("ws")
                .and_then(Value::as_array)
                .ok_or_else(|| "English dictionary group words are missing".to_string())?;
            groups.push(EnglishGroupSummary {
                id: group_id as u32,
                name: group_name.clone(),
                count: words.len(),
            });
            for word in words {
                let entry = normalize_word(
                    next_id,
                    group_id as u32,
                    &group_name,
                    word,
                    pools,
                    exam_pools,
                )?;
                let summary = EnglishIndexEntry {
                    id: entry.id,
                    word: entry.word.clone(),
                    group_id: entry.group_id,
                    pronunciation: entry.pronunciation.clone(),
                    occurrence: entry.occurrence,
                    file_offset: entries_file
                        .stream_position()
                        .map_err(|error| format!("Read English entries offset failed: {error}"))?,
                };
                let line = serde_json::to_vec(&entry)
                    .map_err(|error| format!("Serialize English word failed: {error}"))?;
                entries_file
                    .write_all(&line)
                    .and_then(|_| entries_file.write_all(b"\n"))
                    .map_err(|error| format!("Write English word failed: {error}"))?;
                summaries.push(summary);
                next_id = next_id.saturating_add(1);
            }
        }
        entries_file
            .flush()
            .map_err(|error| format!("Flush English entries failed: {error}"))?;
        // 解码阶段的完整 JSON 只用于安装，落盘前尽早释放它，避免常驻占用。
        drop(source_data);
        let downloaded_at = now_millis();
        let index = EnglishIndexFile {
            source_name: ENGLISH_SOURCE_NAME.to_string(),
            source_url: ENGLISH_SOURCE_URL.to_string(),
            downloaded_at,
            word_count: summaries.len(),
            groups,
            entries: summaries,
        };
        let index_bytes = serde_json::to_vec(&index)
            .map_err(|error| format!("Serialize English index failed: {error}"))?;
        fs::write(temp_root.join(INDEX_FILE), index_bytes)
            .map_err(|error| format!("Write English index failed: {error}"))?;
        if self.root.exists() {
            fs::remove_dir_all(&self.root)
                .map_err(|error| format!("Replace English dictionary failed: {error}"))?;
        }
        fs::rename(&temp_root, &self.root)
            .map_err(|error| format!("Install English dictionary failed: {error}"))?;
        self.set_cache(index);
        self.status()
    }

    fn load_index(&self) -> Result<Arc<EnglishIndexFile>, String> {
        if let Some(index) = self
            .cached_index
            .read()
            .map_err(|_| "English index lock unavailable".to_string())?
            .clone()
        {
            return Ok(index);
        }
        let bytes = fs::read(self.root.join(INDEX_FILE))
            .map_err(|error| format!("Read English dictionary index failed: {error}"))?;
        let index: EnglishIndexFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Parse English dictionary index failed: {error}"))?;
        let index = Arc::new(index);
        if let Ok(mut cache) = self.cached_index.write() {
            *cache = Some(index.clone());
        }
        Ok(index)
    }

    fn set_cache(&self, index: EnglishIndexFile) {
        if let Ok(mut cache) = self.cached_index.write() {
            *cache = Some(Arc::new(index));
        }
    }

    /// 释放英语页面使用的内存索引；词库文件仍保留在磁盘中。
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cached_index.write() {
            *cache = None;
        }
    }
}

fn index_entry_summary(
    entry: &EnglishIndexEntry,
    groups: &[EnglishGroupSummary],
) -> EnglishWordSummary {
    EnglishWordSummary {
        id: entry.id,
        word: entry.word.clone(),
        group_id: entry.group_id,
        group_name: groups
            .get(entry.group_id as usize)
            .map(|group| group.name.clone())
            .unwrap_or_default(),
        pronunciation: entry.pronunciation.clone(),
        occurrence: entry.occurrence,
    }
}

fn contains_ascii_case_insensitive(value: &str, needle: &[u8]) -> bool {
    value
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

fn indexed_json(pools: &Value, key: &str, value: Option<&Value>) -> Value {
    let Some(index) = value.and_then(Value::as_u64) else {
        return value.cloned().unwrap_or(Value::Null);
    };
    pools
        .get(key)
        .and_then(Value::as_array)
        .and_then(|items| items.get(index as usize))
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(Value::Null)
}

fn normalize_word(
    id: u32,
    group_id: u32,
    group_name: &str,
    word: &Value,
    pools: &Value,
    exam_pools: &Value,
) -> Result<EnglishWordEntry, String> {
    let word_text = string_value(word.get("w")).unwrap_or_default();
    let root = indexed_json(pools, "r", word.get("rt"));
    let root_affixes = if root.is_object() {
        let mut parts = Vec::new();
        for key in ["root", "mnemonic"] {
            if let Some(value) = root
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                parts.push(value.to_string());
            }
        }
        if let Some(value) = root
            .get("prefix")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            parts.push(format!("前缀：{value}"));
        }
        if let Some(value) = root
            .get("suffix")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            parts.push(format!("后缀：{value}"));
        }
        parts.join(" ")
    } else {
        String::new()
    };
    let derived_words = word
        .get("dv")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    indexed_json(pools, "v", Some(item))
                        .as_object()
                        .map(|obj| EnglishDerivedWord {
                            word: obj
                                .get("word")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            definition: obj
                                .get("definition")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            part_of_speech: obj
                                .get("part_of_speech")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            word_formation: obj
                                .get("word_formation")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    let exam_examples = normalize_exam_examples(word.get("dt"), exam_pools, pools);
    Ok(EnglishWordEntry {
        id,
        word: word_text,
        group_id,
        group_name: group_name.to_string(),
        pronunciation: string_value(word.get("p")).unwrap_or_default(),
        translation: string_value(word.get("t")).unwrap_or_default(),
        example: string_value(word.get("e")).unwrap_or_default(),
        example_translation: string_value(word.get("ec")).unwrap_or_default(),
        british_audio: audio_url(string_value(word.get("ay"))),
        american_audio: audio_url(string_value(word.get("am"))),
        mnemonic: string_value(word.get("ax")).unwrap_or_default(),
        root_affixes,
        english_definition: string_value(word.get("ed")).unwrap_or_default(),
        derived_words,
        occurrence: word
            .get("oc")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
        exam_examples,
    })
}

fn audio_url(value: Option<String>) -> String {
    value
        .map(|value| {
            if value.starts_with("http") {
                value
            } else {
                format!("https://oss.ors.de5.net/{value}")
            }
        })
        .unwrap_or_default()
}

fn normalize_exam_examples(
    dt: Option<&Value>,
    exam_pools: &Value,
    pools: &Value,
) -> Vec<EnglishExamExample> {
    let mut result = Vec::new();
    let Some(map) = dt.and_then(Value::as_object) else {
        return result;
    };
    for values in map.values().filter_map(Value::as_array) {
        for tuple in values.iter().filter_map(Value::as_array) {
            let sentence = tuple
                .get(0)
                .and_then(Value::as_u64)
                .and_then(|i| {
                    pools
                        .get("s")
                        .and_then(Value::as_array)
                        .and_then(|v| v.get(i as usize))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default();
            let source = tuple
                .get(1)
                .and_then(Value::as_u64)
                .and_then(|i| {
                    exam_pools
                        .get("n")
                        .and_then(Value::as_array)
                        .and_then(|v| v.get(i as usize))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default();
            let section = tuple
                .get(2)
                .and_then(Value::as_u64)
                .and_then(|i| {
                    exam_pools
                        .get("p")
                        .and_then(Value::as_array)
                        .and_then(|v| v.get(i as usize))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default();
            let source_kind = tuple
                .get(3)
                .and_then(Value::as_u64)
                .and_then(|i| {
                    exam_pools
                        .get("y")
                        .and_then(Value::as_array)
                        .and_then(|v| v.get(i as usize))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default();
            if !sentence.is_empty() {
                result.push(EnglishExamExample {
                    sentence: sentence.to_string(),
                    source: source.to_string(),
                    section: section.to_string(),
                    source_kind: source_kind.to_string(),
                });
            }
        }
    }
    result
}

fn decode_source_html(payload: Vec<u8>) -> Result<Value, String> {
    let html = String::from_utf8(payload)
        .map_err(|error| format!("English dictionary source is not UTF-8: {error}"))?;
    let marker = r#"<script type="application/json" id="asp-data">"#;
    let start = html
        .find(marker)
        .ok_or_else(|| "English dictionary payload was not found".to_string())?
        + marker.len();
    let end = html[start..]
        .find("</script>")
        .map(|offset| start + offset)
        .ok_or_else(|| "English dictionary payload is incomplete".to_string())?;
    let encoded = html[start..end].trim();
    let alphabet = build_alphabet();
    let mut decoded = Vec::new();
    for line in encoded.lines() {
        let mut bytes = Vec::with_capacity(line.len() * 4 / 5 + 4);
        let chars: Vec<u8> = line.as_bytes().to_vec();
        for chunk in chars.chunks(5) {
            let count = chunk.len();
            let mut value = 0u64;
            for byte in chunk {
                value = value * 85
                    + *alphabet
                        .get(byte)
                        .ok_or_else(|| "Invalid English dictionary encoding".to_string())?
                        as u64;
            }
            for _ in count..5 {
                value = value * 85 + 84;
            }
            let output_len = count * 4 / 5;
            for index in 0..output_len {
                bytes.push(((value / 256u64.pow((3 - index) as u32)) % 256) as u8);
            }
        }
        let mut decompressor = Decompressor::new(Cursor::new(bytes), 4096);
        let mut part = Vec::new();
        decompressor
            .read_to_end(&mut part)
            .map_err(|error| format!("Brotli decode English dictionary failed: {error}"))?;
        if decoded.len().saturating_add(part.len()) > MAX_DECODED_BYTES {
            return Err("English dictionary decoded data is too large".to_string());
        }
        decoded.extend_from_slice(&part);
    }
    serde_json::from_slice(&decoded)
        .map_err(|error| format!("Parse English dictionary data failed: {error}"))
}

fn build_alphabet() -> HashMap<u8, u8> {
    let mut alphabet = HashMap::new();
    let mut index = 0u8;
    for code in 33u8..=126u8 {
        if matches!(code, b'"' | b'\'' | b'<') {
            continue;
        }
        alphabet.insert(code, index);
        index = index.saturating_add(1);
        if index >= 85 {
            break;
        }
    }
    alphabet
}

pub async fn download_source(client: &reqwest::Client) -> Result<Vec<u8>, String> {
    let response = client
        .get(ENGLISH_SOURCE_URL)
        .send()
        .await
        .map_err(|error| format!("Download English dictionary failed: {error}"))?;
    if response.status() != StatusCode::OK {
        return Err(format!(
            "English dictionary returned HTTP {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Read English dictionary failed: {error}"))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("English dictionary download exceeds the safety limit".to_string());
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use brotli::CompressorWriter;
    use serde_json::json;

    use super::{build_alphabet, decode_source_html};

    fn encode_base85(bytes: &[u8]) -> String {
        let alphabet = build_alphabet();
        let mut symbols = [0u8; 85];
        for (symbol, index) in alphabet {
            symbols[index as usize] = symbol;
        }

        let mut encoded = String::new();
        for chunk in bytes.chunks(4) {
            let mut value = 0u32;
            for index in 0..4 {
                value = (value << 8) | u32::from(chunk.get(index).copied().unwrap_or(0));
            }
            let mut digits = [0u8; 5];
            for index in (0..5).rev() {
                digits[index] = symbols[(value % 85) as usize];
                value /= 85;
            }
            let count = if chunk.len() == 4 { 5 } else { chunk.len() + 1 };
            encoded.push_str(std::str::from_utf8(&digits[..count]).expect("ASCII Base85"));
        }
        encoded
    }

    #[test]
    fn decodes_embedded_base85_brotli_payload() {
        let expected = json!({"g": [{"n": "test", "ws": []}], "d": {}, "p": {}});
        let source = serde_json::to_vec(&expected).unwrap();
        let mut compressed = Vec::new();
        {
            let mut writer = CompressorWriter::new(&mut compressed, 4096, 5, 22);
            writer.write_all(&source).unwrap();
        }
        let html = format!(
            "<html><script type=\"application/json\" id=\"asp-data\">{}</script></html>",
            encode_base85(&compressed)
        );

        assert_eq!(decode_source_html(html.into_bytes()).unwrap(), expected);
    }

    #[test]
    fn rejects_page_without_dictionary_payload() {
        let error = decode_source_html(b"<html></html>".to_vec()).unwrap_err();
        assert!(error.contains("payload was not found"));
    }

    #[test]
    fn matches_words_without_allocating_lowercase_copies() {
        assert!(super::contains_ascii_case_insensitive("Deposit", b"pos"));
        assert!(super::contains_ascii_case_insensitive("Deposit", b"DEPO"));
        assert!(!super::contains_ascii_case_insensitive("Deposit", b"desk"));
    }
}
