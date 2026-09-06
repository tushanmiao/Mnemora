import {
  safeMarkdownImageUrlTransform,
  safeMarkdownUrlTransform,
} from "../../chat/utils/htmlSecurity";

type MarkdownUrlTransform = (value: string, key: string) => string;

export function safeNoteAttachmentPath(value: string): string | null {
  try {
    const path = decodeURIComponent(value);
    return path.startsWith("attachments/") && !/[\\\x00-\x1f?#:]/.test(path)
      && path.split("/").every((part) => part !== ".." && part !== "." && part !== "") ? path : null;
  } catch { return null; }
}

function noteAssetBaseUrl(value: string | null | undefined) {
  if (!value) return null;
  try {
    const url = new URL(value.endsWith("/") ? value : `${value}/`);
    const isTauriAsset = url.protocol === "asset:"
      || (url.protocol === "http:" && url.hostname === "asset.localhost");
    return isTauriAsset ? url : null;
  } catch {
    return null;
  }
}

/**
 * Notes may resolve relative image paths inside their own asset directory. Chat keeps using the
 * stricter shared transform, so enabling note attachments does not widen message rendering.
 */
export function createSafeNoteMarkdownUrlTransform(
  assetBaseUrl: string | null | undefined,
): MarkdownUrlTransform {
  const baseUrl = noteAssetBaseUrl(assetBaseUrl);
  return (value, key) => {
    if (key !== "src") return safeNoteAttachmentPath(value) ? value : safeMarkdownUrlTransform(value);
    const absolute = safeMarkdownImageUrlTransform(value);
    if (absolute) return absolute;
    if (!baseUrl || !value || value.startsWith("/") || value.startsWith("\\")) return "";

    const relative = value.replace(/\\/g, "/");
    const path = relative.split(/[?#]/, 1)[0] ?? "";
    if (path.split("/").some((segment) => segment === "..") || /^[a-zA-Z]:/.test(path)) {
      return "";
    }
    try {
      const resolved = new URL(relative, baseUrl);
      return resolved.href.startsWith(baseUrl.href) ? resolved.href : "";
    } catch {
      return "";
    }
  };
}
