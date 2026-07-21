import { invoke, isTauri } from "@tauri-apps/api/core";

const MAX_HTML_PREVIEW_BYTES = 1024 * 1024;

function htmlSizeInBytes(html: string) {
  return new TextEncoder().encode(html).byteLength;
}

export async function openHtmlPreview(html: string) {
  if (!isTauri()) throw new Error("HTML 预览仅在桌面应用中可用");
  if (!html.trim()) throw new Error("HTML 预览内容不能为空");
  if (htmlSizeInBytes(html) > MAX_HTML_PREVIEW_BYTES) {
    throw new Error("HTML 预览内容不能超过 1 MB");
  }
  await invoke("html_preview_open", { html });
}

export async function loadHtmlPreview(token: string) {
  return invoke<string>("html_preview_get", { token });
}

