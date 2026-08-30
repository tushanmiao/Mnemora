import {
  safeMarkdownImageUrlTransform,
  safeMarkdownUrlTransform,
} from "../../chat/utils/htmlSecurity";

type MarkdownUrlTransform = (value: string, key: string) => string;

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
    if (key !== "src") return safeMarkdownUrlTransform(value);
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
