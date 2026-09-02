//! 图表导出的 Tauri 命令边界。
//!
//! 前端只能把「用户已经在保存对话框里选好的路径」交过来，所以这里的校验重点不是
//! 路径来源，而是**扩展名与内容必须对得上**：写盘发生在用户看不见的一侧，一旦
//! 把 SVG 文本写进 .png，用户拿到的是一个永远打不开的文件，且没有任何报错。

use std::{fs, path::Path};

use base64::{engine::general_purpose, Engine as _};

/// 单张图表的字节上限。
///
/// 图表是矢量图或它的位图快照，正常在几百 KB 量级。给到 24 MB 是为超大 flowchart
/// 的 2 倍图留余量，同时挡住「把整段 base64 当图表传进来」这类明显失控的载荷。
const MAX_DIAGRAM_BYTES: usize = 24 * 1024 * 1024;

/// 允许的扩展名。与前端保存对话框的 filters 一一对应。
const ALLOWED_EXTENSIONS: [&str; 2] = ["png", "svg"];

/// PNG 的 8 字节文件签名。用来确认 base64 解出来的确实是 PNG。
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// 把图表写到用户选定的路径。
///
/// `data_base64` 一律是 base64：PNG 本来就是二进制，SVG 走同一条路是为了避免前端
/// 按格式分叉出两个命令——文本编码差异（BOM、换行）在 base64 里不会被中间层改写。
#[tauri::command]
pub async fn export_diagram_file(path: String, data_base64: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || write_diagram(&path, &data_base64))
        .await
        .map_err(|error| format!("图表导出任务失败：{error}"))?
}

fn write_diagram(path: &str, data_base64: &str) -> Result<(), String> {
    let target = Path::new(path);
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXTENSIONS.contains(&extension.as_str()) {
        return Err("图表只能保存为 .png 或 .svg。".to_string());
    }

    // 保存对话框给的路径其父目录一定存在；这里仍然检查，因为命令本身是可被直接
    // 调用的边界，不能假设调用方一定走过对话框。
    let parent = target
        .parent()
        .ok_or_else(|| "图表保存路径没有父目录。".to_string())?;
    if !parent.as_os_str().is_empty() && !parent.is_dir() {
        return Err("图表保存目录不存在。".to_string());
    }

    let payload = data_base64.trim();
    if payload.is_empty() {
        return Err("图表内容为空。".to_string());
    }
    let bytes = general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| format!("解析图表内容失败：{error}"))?;
    if bytes.is_empty() {
        return Err("图表内容为空。".to_string());
    }
    if bytes.len() > MAX_DIAGRAM_BYTES {
        return Err(format!(
            "图表不能超过 {} MB。",
            MAX_DIAGRAM_BYTES / 1024 / 1024
        ));
    }

    // 扩展名与内容必须一致，否则用户拿到一个打不开的文件却收不到任何提示。
    match extension.as_str() {
        "png" if !bytes.starts_with(&PNG_SIGNATURE) => {
            return Err("图表内容不是有效的 PNG。".to_string());
        }
        "svg" if !looks_like_svg(&bytes) => {
            return Err("图表内容不是有效的 SVG。".to_string());
        }
        _ => {}
    }

    fs::write(target, bytes).map_err(|error| format!("保存图表失败：{error}"))
}

/// SVG 没有魔数，只能看开头是不是 XML/SVG 标签。
///
/// 刻意只做前缀判断而不做完整解析：这一层的职责是挡住「格式与扩展名不符」，
/// SVG 的安全清洗早在前端 `sanitizeMermaidSvg` 做过了，重复解析没有收益。
fn looks_like_svg(bytes: &[u8]) -> bool {
    // 跳过 UTF-8 BOM 与前导空白，再看首个非空白字符是否为 '<'。
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let head = body
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|start| &body[start..])
        .unwrap_or_default();
    if !head.starts_with(b"<") {
        return false;
    }
    // 允许 XML 声明、注释或 DOCTYPE 开头，所以在前若干字节里找 "<svg"。
    let probe_len = head.len().min(1024);
    head[..probe_len]
        .windows(4)
        .any(|window| window.eq_ignore_ascii_case(b"<svg"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 用例自带目录，避免并行测试互相踩到同名文件。
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mnemora-diagram-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("创建测试目录");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn encode(bytes: &[u8]) -> String {
        general_purpose::STANDARD.encode(bytes)
    }

    fn png_bytes() -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(b"payload");
        bytes
    }

    #[test]
    fn rejects_unexpected_extensions() {
        let directory = TestDirectory::new("extension");
        let path = directory.join("diagram.exe");
        let error = write_diagram(&path.to_string_lossy(), &encode(&png_bytes()))
            .expect_err("非法扩展名必须被拒绝");
        assert!(error.contains(".png"), "{error}");
    }

    #[test]
    fn rejects_content_that_contradicts_the_extension() {
        let directory = TestDirectory::new("mismatch");
        let svg_path = directory.join("diagram.svg");
        let error = write_diagram(&svg_path.to_string_lossy(), &encode(&png_bytes()))
            .expect_err("PNG 字节写进 .svg 必须被拒绝");
        assert!(error.contains("SVG"), "{error}");

        let png_path = directory.join("diagram.png");
        let error = write_diagram(&png_path.to_string_lossy(), &encode(b"<svg></svg>"))
            .expect_err("SVG 文本写进 .png 必须被拒绝");
        assert!(error.contains("PNG"), "{error}");
    }

    #[test]
    fn writes_png_and_svg_payloads() {
        let directory = TestDirectory::new("write");
        let png_path = directory.join("diagram.png");
        write_diagram(&png_path.to_string_lossy(), &encode(&png_bytes())).expect("写 PNG");
        assert_eq!(fs::read(&png_path).expect("读回 PNG"), png_bytes());

        let svg_path = directory.join("diagram.svg");
        let svg = b"<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        write_diagram(&svg_path.to_string_lossy(), &encode(svg)).expect("写 SVG");
        assert_eq!(fs::read(&svg_path).expect("读回 SVG"), svg);
    }

    #[test]
    fn accepts_uppercase_extensions() {
        let directory = TestDirectory::new("uppercase");
        let path = directory.join("diagram.PNG");
        write_diagram(&path.to_string_lossy(), &encode(&png_bytes()))
            .expect("扩展名大小写不应影响判定");
    }

    #[test]
    fn rejects_empty_and_oversized_payloads() {
        let directory = TestDirectory::new("bounds");
        let path = directory.join("diagram.png");
        let error = write_diagram(&path.to_string_lossy(), "   ").expect_err("空载荷必须被拒绝");
        assert!(error.contains("为空"), "{error}");

        let mut oversized = PNG_SIGNATURE.to_vec();
        oversized.resize(MAX_DIAGRAM_BYTES + 1, 0);
        let error = write_diagram(&path.to_string_lossy(), &encode(&oversized))
            .expect_err("超限载荷必须被拒绝");
        assert!(error.contains("MB"), "{error}");
    }

    #[test]
    fn rejects_missing_directory() {
        let directory = TestDirectory::new("missing");
        let path = directory.join("missing").join("diagram.png");
        let error = write_diagram(&path.to_string_lossy(), &encode(&png_bytes()))
            .expect_err("目录不存在必须被拒绝");
        assert!(error.contains("目录"), "{error}");
    }
}
