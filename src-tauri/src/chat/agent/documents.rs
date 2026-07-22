//! Office 附件的有界、按需解析。
//!
//! 本模块不建立缓存，也不启动外部进程。每次工具调用只打开当前会话的安全副本，
//! 校验压缩包预算，读取用户指定的小范围内容，然后立即释放解析器和文件句柄。

use std::{
    fs::{self, File},
    io::BufReader,
    path::Path,
};

use calamine::{open_workbook_auto, Data, Reader as CalamineReader};
use quick_xml::{escape::unescape, events::Event, Reader as XmlReader};
use serde_json::Value;
use zip::ZipArchive;

use crate::ai::error::ModelError;

use super::types::ToolExecution;

const MAX_OFFICE_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_OFFICE_ZIP_ENTRIES: usize = 4_096;
const MAX_OFFICE_XML_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_OFFICE_XML_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_DOCX_BLOCKS_PER_CALL: usize = 200;
pub(super) const MAX_XLSX_ROWS_PER_CALL: usize = 200;
const MAX_XLSX_COLUMNS_PER_ROW: usize = 100;
const MAX_PREVIEW_CHARS: usize = 2_000;

pub(super) fn read_docx_blocks(
    path: &Path,
    attachment_id: &str,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    preflight_office_zip(path)?;
    let start = positive_argument(arguments, "startBlock", 1)? as usize;
    let end = positive_argument(
        arguments,
        "endBlock",
        start.saturating_add(MAX_DOCX_BLOCKS_PER_CALL - 1) as u64,
    )? as usize;
    if end < start || end.saturating_sub(start) >= MAX_DOCX_BLOCKS_PER_CALL {
        return Err(ModelError::invalid_configuration(format!(
            "DOCX 单次最多读取 {MAX_DOCX_BLOCKS_PER_CALL} 个内容块。"
        )));
    }

    let file = File::open(path)
        .map_err(|error| ModelError::invalid_configuration(format!("打开 DOCX 失败：{error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| ModelError::invalid_configuration(format!("DOCX 格式无效：{error}")))?;
    let document = archive.by_name("word/document.xml").map_err(|_| {
        ModelError::invalid_configuration("DOCX 缺少 word/document.xml。")
    })?;
    if document.size() > MAX_OFFICE_XML_ENTRY_BYTES {
        return Err(ModelError::invalid_configuration(
            "DOCX 主文档 XML 超过 16 MB 解析上限。",
        ));
    }

    let mut reader = XmlReader::from_reader(BufReader::new(document));
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut paragraph = String::new();
    let mut cell_paragraphs = Vec::new();
    let mut row_cells = Vec::new();
    let mut in_text = false;
    let mut cell_depth = 0usize;
    let mut row_depth = 0usize;
    let mut block_index = 0usize;
    let mut selected = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                b"t" => in_text = true,
                b"tc" => cell_depth = cell_depth.saturating_add(1),
                b"tr" => row_depth = row_depth.saturating_add(1),
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.local_name().as_ref() {
                b"tab" => paragraph.push('\t'),
                b"br" | b"cr" => paragraph.push('\n'),
                _ => {}
            },
            Ok(Event::Text(event)) if in_text => {
                let decoded = event.decode().map_err(|error| {
                    ModelError::invalid_configuration(format!("DOCX 文本编码无效：{error}"))
                })?;
                let decoded = unescape(&decoded).map_err(|error| {
                    ModelError::invalid_configuration(format!("DOCX 文本实体无效：{error}"))
                })?;
                paragraph.push_str(&decoded);
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    let value = normalize_office_text(std::mem::take(&mut paragraph));
                    if cell_depth > 0 {
                        if !value.is_empty() {
                            cell_paragraphs.push(value);
                        }
                    } else if row_depth == 0
                        && push_docx_block(
                            value,
                            start,
                            end,
                            &mut block_index,
                            &mut selected,
                        )
                    {
                        break;
                    }
                }
                b"tc" => {
                    cell_depth = cell_depth.saturating_sub(1);
                    row_cells.push(cell_paragraphs.join(" / "));
                    cell_paragraphs.clear();
                }
                b"tr" => {
                    row_depth = row_depth.saturating_sub(1);
                    let value = row_cells
                        .drain(..)
                        .map(|value| if value.is_empty() { "[空]".to_string() } else { value })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    if push_docx_block(value, start, end, &mut block_index, &mut selected) {
                        break;
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ModelError::invalid_configuration(format!(
                    "DOCX XML 解析失败：{error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    let content = if selected.is_empty() {
        format!("DOCX 中没有第 {start} 到 {end} 个可读取内容块。")
    } else {
        selected
            .into_iter()
            .map(|(index, value)| format!("[DOCX:{attachment_id}#block={index}]\n{value}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    Ok(tool_execution(content))
}

pub(super) fn read_xlsx_rows(
    path: &Path,
    attachment_id: &str,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    preflight_office_zip(path)?;
    let start = positive_argument(arguments, "startRow", 1)? as usize;
    let end = positive_argument(
        arguments,
        "endRow",
        start.saturating_add(MAX_XLSX_ROWS_PER_CALL - 1) as u64,
    )? as usize;
    if end < start || end.saturating_sub(start) >= MAX_XLSX_ROWS_PER_CALL {
        return Err(ModelError::invalid_configuration(format!(
            "XLSX 单次最多读取 {MAX_XLSX_ROWS_PER_CALL} 行。"
        )));
    }

    let mut workbook = open_workbook_auto(path)
        .map_err(|error| ModelError::invalid_configuration(format!("XLSX 解析失败：{error}")))?;
    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(ModelError::invalid_configuration("XLSX 不包含工作表。"));
    }
    let requested_sheet = arguments
        .get("sheetName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let sheet_name = requested_sheet.unwrap_or(&sheet_names[0]);
    if !sheet_names.iter().any(|name| name == sheet_name) {
        return Err(ModelError::invalid_configuration(format!(
            "找不到工作表“{sheet_name}”。可用工作表：{}",
            sheet_names.join("、")
        )));
    }
    let range = workbook.worksheet_range(sheet_name).map_err(|error| {
        ModelError::invalid_configuration(format!("读取工作表“{sheet_name}”失败：{error}"))
    })?;
    let (range_start_row, range_start_column) = range
        .start()
        .map(|(row, column)| (row as usize + 1, column as usize))
        .unwrap_or((1, 0));
    let reference_sheet = sanitize_reference_component(sheet_name);
    let mut rows = Vec::new();
    for (offset, row) in range.rows().enumerate() {
        let row_number = range_start_row + offset;
        if row_number < start {
            continue;
        }
        if row_number > end {
            break;
        }
        let mut cells = row
            .iter()
            .take(MAX_XLSX_COLUMNS_PER_ROW)
            .enumerate()
            .filter_map(|(column_offset, value)| {
                let value = data_to_text(value);
                (!value.is_empty()).then(|| {
                    format!(
                        "{}={value}",
                        spreadsheet_column_name(range_start_column + column_offset)
                    )
                })
            })
            .collect::<Vec<_>>();
        if row.len() > MAX_XLSX_COLUMNS_PER_ROW {
            cells.push(format!("[仅显示前 {MAX_XLSX_COLUMNS_PER_ROW} 列]"));
        }
        let row_text = if cells.is_empty() {
            "[空行]".to_string()
        } else {
            cells.join(" | ")
        };
        rows.push(format!(
            "[XLSX:{attachment_id}#sheet={reference_sheet}#row={row_number}]\n{row_text}"
        ));
    }
    let sheet_catalog = sheet_names.join("、");
    let content = if rows.is_empty() {
        format!(
            "工作表“{sheet_name}”中没有第 {start} 到 {end} 行。可用工作表：{sheet_catalog}"
        )
    } else {
        format!(
            "可用工作表：{sheet_catalog}\n当前工作表：{sheet_name}\n\n{}",
            rows.join("\n\n")
        )
    };
    Ok(tool_execution(content))
}

fn preflight_office_zip(path: &Path) -> Result<(), ModelError> {
    let metadata = fs::metadata(path).map_err(|error| {
        ModelError::invalid_configuration(format!("读取 Office 附件失败：{error}"))
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_OFFICE_FILE_BYTES {
        return Err(ModelError::invalid_configuration(
            "Office 附件必须是 1 字节到 10 MB 的普通文件。",
        ));
    }
    let file = File::open(path).map_err(|error| {
        ModelError::invalid_configuration(format!("打开 Office 附件失败：{error}"))
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| ModelError::invalid_configuration(format!("Office 文件不是有效 ZIP：{error}")))?;
    if archive.len() > MAX_OFFICE_ZIP_ENTRIES {
        return Err(ModelError::invalid_configuration(
            "Office 文件内部条目超过 4096 个。",
        ));
    }
    let mut xml_total = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            ModelError::invalid_configuration(format!("读取 Office ZIP 条目失败：{error}"))
        })?;
        let name = entry.name().to_ascii_lowercase();
        if name.ends_with(".xml") || name.ends_with(".rels") {
            if entry.size() > MAX_OFFICE_XML_ENTRY_BYTES {
                return Err(ModelError::invalid_configuration(
                    "Office 文件中的单个 XML 条目超过 16 MB。",
                ));
            }
            xml_total = xml_total.saturating_add(entry.size());
            if xml_total > MAX_OFFICE_XML_TOTAL_BYTES {
                return Err(ModelError::invalid_configuration(
                    "Office 文件的 XML 解压总量超过 64 MB。",
                ));
            }
        }
    }
    Ok(())
}

fn push_docx_block(
    value: String,
    start: usize,
    end: usize,
    block_index: &mut usize,
    selected: &mut Vec<(usize, String)>,
) -> bool {
    if value.is_empty() {
        return false;
    }
    *block_index = block_index.saturating_add(1);
    if *block_index >= start && *block_index <= end {
        selected.push((*block_index, value));
    }
    *block_index >= end
}

fn positive_argument(arguments: &Value, key: &str, default: u64) -> Result<u64, ModelError> {
    match arguments.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                ModelError::invalid_configuration(format!("工具参数 {key} 必须是正整数。"))
            }),
    }
}

fn normalize_office_text(value: String) -> String {
    value
        .replace('\r', "")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn data_to_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        _ => value
            .to_string()
            .replace(['\r', '\n', '\t'], " ")
            .trim()
            .to_string(),
    }
}

fn spreadsheet_column_name(mut column: usize) -> String {
    let mut name = String::new();
    column += 1;
    while column > 0 {
        let remainder = (column - 1) % 26;
        name.insert(0, (b'A' + remainder as u8) as char);
        column = (column - 1) / 26;
    }
    name
}

fn sanitize_reference_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if matches!(character, ']' | '#' | '\r' | '\n') {
                '_'
            } else {
                character
            }
        })
        .collect()
}

fn tool_execution(content: String) -> ToolExecution {
    ToolExecution {
        preview: content.chars().take(MAX_PREVIEW_CHARS).collect(),
        content,
        is_error: false,
        activated_skill_id: None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
        path::{Path, PathBuf},
    };

    use serde_json::json;
    use uuid::Uuid;
    use zip::{write::SimpleFileOptions, ZipWriter};

    use super::{read_docx_blocks, read_xlsx_rows};

    #[test]
    fn reads_docx_paragraphs_and_table_rows_with_block_citations() {
        let path = temp_path("docx");
        write_zip(
            &path,
            &[(
                "word/document.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>第一段</w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>姓名</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>张三</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#,
            )],
        );

        let result = read_docx_blocks(
            &path,
            "attachment-docx",
            &json!({ "startBlock": 1, "endBlock": 2 }),
        )
        .unwrap();
        assert!(result
            .content
            .contains("[DOCX:attachment-docx#block=1]\n第一段"));
        assert!(result.content.contains("姓名 | 张三"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_selected_xlsx_rows_and_lists_sheets() {
        let path = temp_path("xlsx");
        write_minimal_xlsx(&path);

        let result = read_xlsx_rows(
            &path,
            "attachment-xlsx",
            &json!({ "sheetName": "数据", "startRow": 1, "endRow": 2 }),
        )
        .unwrap();
        assert!(result.content.contains("可用工作表：数据"));
        assert!(result
            .content
            .contains("[XLSX:attachment-xlsx#sheet=数据#row=1]"));
        assert!(result.content.contains("A=名称 | B=数量"));
        assert!(result.content.contains("A=苹果 | B=3"));
        fs::remove_file(path).unwrap();
    }

    fn temp_path(extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mnemora-office-{}.{}", Uuid::new_v4(), extension))
    }

    fn write_minimal_xlsx(path: &Path) {
        write_zip(
            path,
            &[
                (
                    "[Content_Types].xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
                ),
                (
                    "_rels/.rels",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
                ),
                (
                    "xl/workbook.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="数据" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#,
                ),
                (
                    "xl/_rels/workbook.xml.rels",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#,
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>名称</t></is></c><c r="B1" t="inlineStr"><is><t>数量</t></is></c></row>
<row r="2"><c r="A2" t="inlineStr"><is><t>苹果</t></is></c><c r="B2"><v>3</v></c></row>
</sheetData></worksheet>"#,
                ),
            ],
        );
    }

    fn write_zip(path: &Path, files: &[(&str, &str)]) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, content) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
}
