//! 统一的聊天附件格式能力注册表。
//!
//! 上传层、普通 Agent 和深度笔记都应以这里的分类为准，避免出现“已经允许上传，
//! 但不同读取链路对白名单理解不一致”的漂移。这里只描述安全读取能力，不执行用户
//! 代码，也不把未知二进制文件当作文本猜测。

use std::path::Path;

use super::conversation_types::StoredChatAttachment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentReadKind {
    Text,
    Pdf,
    Docx,
    Xlsx,
    Image,
    Unsupported,
}

const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "rst",
    "csv",
    "tsv",
    "json",
    "jsonl",
    "ndjson",
    "ipynb",
    "xml",
    "svg",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "mts",
    "cts",
    "vue",
    "svelte",
    "astro",
    "rs",
    "py",
    "pyi",
    "pyw",
    "java",
    "c",
    "cc",
    "cpp",
    "cxx",
    "h",
    "hh",
    "hpp",
    "hxx",
    "cs",
    "go",
    "rb",
    "php",
    "swift",
    "kt",
    "kts",
    "dart",
    "scala",
    "sc",
    "lua",
    "r",
    "m",
    "mm",
    "fs",
    "fsx",
    "vb",
    "vbs",
    "groovy",
    "clj",
    "cljs",
    "ex",
    "exs",
    "erl",
    "hrl",
    "sql",
    "graphql",
    "gql",
    "proto",
    "prisma",
    "toml",
    "yaml",
    "yml",
    "ini",
    "cfg",
    "conf",
    "config",
    "properties",
    "env",
    "editorconfig",
    "gitignore",
    "gitattributes",
    "dockerignore",
    "npmrc",
    "lock",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "psm1",
    "psd1",
    "bat",
    "cmd",
    "cmake",
    "gradle",
    "tf",
    "tfvars",
    "hcl",
    "nix",
    "log",
    "tex",
    "bib",
    "adoc",
    "dockerfile",
    "makefile",
];

const TEXT_FILE_NAMES: &[&str] = &[
    "dockerfile",
    "containerfile",
    "makefile",
    "gnumakefile",
    "cmakelists.txt",
    "jenkinsfile",
    "procfile",
    "vagrantfile",
    "gemfile",
    "rakefile",
    "justfile",
    "license",
    "readme",
    ".env",
    ".editorconfig",
    ".gitignore",
    ".gitattributes",
    ".dockerignore",
    ".npmrc",
];

const SENSITIVE_TEXT_FILE_NAMES: &[&str] = &[
    ".env",
    ".npmrc",
    ".pypirc",
    ".netrc",
    "credentials",
    "credentials.json",
    "secrets.json",
];

pub fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn normalized_name(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .to_ascii_lowercase()
}

pub fn is_text_name(name: &str) -> bool {
    let extension = extension(name);
    TEXT_EXTENSIONS.contains(&extension.as_str())
        || TEXT_FILE_NAMES.contains(&normalized_name(name).as_str())
}

pub fn is_text_mime(mime_type: &str) -> bool {
    mime_type.starts_with("text/")
        || matches!(
            mime_type,
            "application/json"
                | "application/ld+json"
                | "application/x-ndjson"
                | "application/xml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/sql"
                | "application/graphql"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
        )
}

pub fn is_text_attachment(attachment: &StoredChatAttachment) -> bool {
    is_text_mime(&attachment.mime_type) || is_text_name(&attachment.name)
}

pub fn code_language(name: &str) -> Option<&'static str> {
    let normalized = normalized_name(name);
    let language = match extension(name).as_str() {
        "py" | "pyi" | "pyw" => "Python",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "astro" => "Astro",
        "rs" => "Rust",
        "java" => "Java",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "C++",
        "cs" => "C#",
        "go" => "Go",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "kt" | "kts" => "Kotlin",
        "dart" => "Dart",
        "scala" | "sc" => "Scala",
        "lua" => "Lua",
        "r" => "R",
        "m" | "mm" => "Objective-C",
        "fs" | "fsx" => "F#",
        "vb" | "vbs" => "Visual Basic",
        "groovy" | "gradle" => "Groovy",
        "clj" | "cljs" => "Clojure",
        "ex" | "exs" => "Elixir",
        "erl" | "hrl" => "Erlang",
        "sh" | "bash" | "zsh" | "fish" => "Shell",
        "ps1" | "psm1" | "psd1" => "PowerShell",
        "bat" | "cmd" => "Windows Batch",
        "sql" => "SQL",
        "graphql" | "gql" => "GraphQL",
        "proto" => "Protocol Buffers",
        "prisma" => "Prisma Schema",
        "tf" | "tfvars" | "hcl" => "HCL/Terraform",
        "nix" => "Nix",
        "cmake" => "CMake",
        "tex" => "TeX",
        "ipynb" => "Jupyter Notebook JSON",
        _ => match normalized.as_str() {
            "dockerfile" | "containerfile" => "Dockerfile",
            "makefile" | "gnumakefile" => "Makefile",
            "cmakelists.txt" => "CMake",
            "jenkinsfile" => "Jenkins Pipeline",
            "rakefile" | "gemfile" => "Ruby",
            _ => return None,
        },
    };
    Some(language)
}

pub fn is_sensitive_text_name(name: &str) -> bool {
    let name = normalized_name(name);
    SENSITIVE_TEXT_FILE_NAMES.contains(&name.as_str())
        || name.starts_with(".env.")
        || extension(&name) == "env"
        || name
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| matches!(token, "secret" | "secrets" | "credential" | "credentials"))
}

pub fn is_pdf_attachment(attachment: &StoredChatAttachment) -> bool {
    attachment.mime_type == "application/pdf" || extension(&attachment.name) == "pdf"
}

pub fn is_docx_attachment(attachment: &StoredChatAttachment) -> bool {
    attachment.mime_type
        == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        || extension(&attachment.name) == "docx"
}

pub fn is_xlsx_attachment(attachment: &StoredChatAttachment) -> bool {
    attachment.mime_type == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        || extension(&attachment.name) == "xlsx"
}

pub fn deep_note_read_kind(attachment: &StoredChatAttachment) -> AttachmentReadKind {
    if attachment.kind == "image" {
        AttachmentReadKind::Image
    } else if is_text_attachment(attachment) && !is_sensitive_text_name(&attachment.name) {
        AttachmentReadKind::Text
    } else if is_pdf_attachment(attachment) {
        AttachmentReadKind::Pdf
    } else if is_docx_attachment(attachment) {
        AttachmentReadKind::Docx
    } else if is_xlsx_attachment(attachment) {
        AttachmentReadKind::Xlsx
    } else {
        AttachmentReadKind::Unsupported
    }
}

pub fn is_supported_deep_note_attachment(attachment: &StoredChatAttachment) -> bool {
    deep_note_read_kind(attachment) != AttachmentReadKind::Unsupported
}

pub fn mime_type_for_name(name: &str) -> &'static str {
    match extension(name).as_str() {
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" | "jsonl" | "ndjson" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "jsx" | "mjs" | "cjs" => "application/javascript",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        _ if is_text_name(name) => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        code_language, deep_note_read_kind, is_sensitive_text_name, is_text_name,
        mime_type_for_name, AttachmentReadKind,
    };
    use crate::chat::conversation_types::StoredChatAttachment;

    #[test]
    fn recognizes_code_config_and_extensionless_text_files() {
        for name in [
            "main.py",
            "component.vue",
            "schema.graphql",
            "events.proto",
            "analysis.ipynb",
            "diagram.svg",
            ".env",
            "Dockerfile",
            "Makefile",
            "CMakeLists.txt",
        ] {
            assert!(is_text_name(name), "expected {name} to be text-readable");
        }
    }

    #[test]
    fn rejects_binary_and_container_names() {
        for name in [
            "archive.zip",
            "slides.pptx",
            "database.sqlite",
            "program.exe",
        ] {
            assert!(!is_text_name(name), "expected {name} to stay unsupported");
        }
    }

    #[test]
    fn assigns_useful_mime_types_without_claiming_unknown_binary_support() {
        assert_eq!(mime_type_for_name("main.py"), "text/plain");
        assert_eq!(mime_type_for_name("Dockerfile"), "text/plain");
        assert_eq!(
            mime_type_for_name("archive.zip"),
            "application/octet-stream"
        );
    }

    #[test]
    fn identifies_code_language_without_executing_the_file() {
        assert_eq!(code_language("main.py"), Some("Python"));
        assert_eq!(code_language("Dockerfile"), Some("Dockerfile"));
        assert_eq!(code_language("notes.md"), None);
    }

    #[test]
    fn keeps_secret_bearing_text_out_of_automatic_deep_note_recon() {
        assert!(is_sensitive_text_name(".env.production"));
        assert!(is_sensitive_text_name("production.env"));
        assert!(is_sensitive_text_name("client-secret.yaml"));
        assert!(!is_sensitive_text_name("secretary-notes.md"));
        let attachment = StoredChatAttachment {
            id: "attachment-1".to_string(),
            kind: "file".to_string(),
            name: ".env".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 10,
            path: "attachment-1.env".to_string(),
            preview_path: None,
            width: None,
            height: None,
        };
        assert_eq!(
            deep_note_read_kind(&attachment),
            AttachmentReadKind::Unsupported
        );
    }
}
