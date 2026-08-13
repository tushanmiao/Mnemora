export type AttachmentCapability = "image" | "document" | "unsupported";
export type AttachmentCapabilityError = "vision" | "tools" | "format";

type AttachmentDescriptor = {
  kind: "image" | "file";
  name: string;
  mimeType: string;
};

/**
 * 这些格式与 Rust Agent 注册表中的读取工具保持一致。选择器只展示可读取的格式，
 * 粘贴和提交仍会经过同一份能力判断，避免只依赖文件扩展名过滤器。
 */
export const IMAGE_ATTACHMENT_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "gif"] as const;
export const DOCUMENT_ATTACHMENT_EXTENSIONS = [
  "pdf", "txt", "md", "csv", "json", "html", "htm", "css", "xml",
  "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "java", "c", "cc", "cpp", "h",
  "hpp", "cs", "go", "rb", "php", "swift", "kt", "kts", "sql", "toml", "yaml", "yml",
  "scss", "less", "sh", "bash", "ps1", "bat", "cmd", "ini", "conf", "env", "log",
  "docx", "xlsx",
] as const;

const IMAGE_MIME_TYPES = new Set(IMAGE_ATTACHMENT_EXTENSIONS.map((extension) => `image/${extension === "jpg" || extension === "jpeg" ? "jpeg" : extension}`));
const DOCUMENT_MIME_TYPES = new Set([
  "application/pdf", "text/plain", "text/markdown", "text/csv", "text/html", "text/css",
  "text/xml", "application/json", "application/xml", "application/javascript",
  "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
]);

function extensionOf(name: string) {
  return name.trim().split(".").pop()?.toLocaleLowerCase("en-US") ?? "";
}

export function classifyAttachment(name: string, mimeType: string, kind?: "image" | "file"): AttachmentCapability {
  const mime = mimeType.trim().toLocaleLowerCase("en-US");
  const extension = extensionOf(name);
  if (kind === "image" || IMAGE_MIME_TYPES.has(mime) || IMAGE_ATTACHMENT_EXTENSIONS.includes(extension as typeof IMAGE_ATTACHMENT_EXTENSIONS[number])) {
    return "image";
  }
  if (DOCUMENT_MIME_TYPES.has(mime) || DOCUMENT_ATTACHMENT_EXTENSIONS.includes(extension as typeof DOCUMENT_ATTACHMENT_EXTENSIONS[number])) {
    return "document";
  }
  return "unsupported";
}

export function attachmentCapabilityError(
  attachment: AttachmentDescriptor,
  supportsVision: boolean | null | undefined,
  supportsTools: boolean | null | undefined,
): AttachmentCapabilityError | null {
  const capability = classifyAttachment(attachment.name, attachment.mimeType, attachment.kind);
  if (capability === "image" && supportsVision === false) return "vision" as const;
  if (capability === "document" && supportsTools !== true) return "tools" as const;
  if (capability === "unsupported") return "format" as const;
  return null;
}

export function allowedAttachmentExtensions(
  supportsVision: boolean | null | undefined,
  supportsTools: boolean | null | undefined,
) {
  return [
    ...(supportsVision === false ? [] : IMAGE_ATTACHMENT_EXTENSIONS),
    ...(supportsTools === true ? DOCUMENT_ATTACHMENT_EXTENSIONS : []),
  ];
}
