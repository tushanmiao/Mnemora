import { invoke, isTauri } from "@tauri-apps/api/core";
import type { ChatAttachment, PendingChatAttachment } from "../../../types/attachment";

const MAX_PREVIEW_CACHE_ITEMS = 32;
const MAX_PREVIEW_CACHE_BYTES = 8 * 1024 * 1024;

type PreviewCacheEntry = { dataUrl: string; bytes: number };
type PreviewRequest = {
  requestId: string;
  promise: Promise<string>;
  consumers: number;
};

export type AttachmentPreviewLoad = {
  promise: Promise<string>;
  release: () => void;
};

const previewCache = new Map<string, PreviewCacheEntry>();
const previewRequests = new Map<string, PreviewRequest>();
let previewCacheBytes = 0;

function previewKey(path: string, conversationId?: string | null, previewPath?: string | null) {
  return `${conversationId ?? "pending"}\u0000${path}\u0000${previewPath ?? ""}`;
}

function readCachedPreview(key: string) {
  const cached = previewCache.get(key);
  if (!cached) return null;
  previewCache.delete(key);
  previewCache.set(key, cached);
  return cached.dataUrl;
}

function cachePreview(key: string, dataUrl: string) {
  const bytes = dataUrl.length * 2;
  if (bytes > MAX_PREVIEW_CACHE_BYTES) return;
  const previous = previewCache.get(key);
  if (previous) previewCacheBytes -= previous.bytes;
  previewCache.delete(key);
  previewCache.set(key, { dataUrl, bytes });
  previewCacheBytes += bytes;
  while (previewCache.size > MAX_PREVIEW_CACHE_ITEMS || previewCacheBytes > MAX_PREVIEW_CACHE_BYTES) {
    const oldest = previewCache.entries().next().value as [string, PreviewCacheEntry] | undefined;
    if (!oldest) break;
    previewCache.delete(oldest[0]);
    previewCacheBytes -= oldest[1].bytes;
  }
}

export function inspectChatAttachments(paths: string[]) {
  if (!isTauri()) return Promise.reject(new Error("附件选择需要在 Tauri 应用中运行。"));
  return invoke<PendingChatAttachment[]>("inspect_chat_attachments", { paths });
}

export function savePastedChatAttachment(
  name: string,
  mimeType: string,
  dataBase64: string,
) {
  if (!isTauri()) return Promise.reject(new Error("剪贴板附件需要在 Tauri 应用中运行。"));
  return invoke<PendingChatAttachment>("save_pasted_chat_attachment", {
    name,
    mimeType,
    dataBase64,
  });
}

export function discardStagedChatAttachment(path: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("discard_staged_chat_attachment", { path });
}

export function importChatAttachments(
  requestId: string,
  conversationId: string,
  paths: string[],
) {
  if (!isTauri()) return Promise.reject(new Error("附件导入需要在 Tauri 应用中运行。"));
  return invoke<ChatAttachment[]>("import_chat_attachments", { requestId, conversationId, paths });
}

export function cancelChatAttachmentTask(requestId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("cancel_chat_attachment_task", { requestId });
}

export function discardImportedChatAttachments(
  conversationId: string,
  attachments: ChatAttachment[],
) {
  if (!isTauri()) return Promise.resolve(0);
  return invoke<number>("discard_imported_chat_attachments", { conversationId, attachments });
}

export function loadChatAttachmentPreview(
  path: string,
  conversationId?: string | null,
  previewPath?: string | null,
): AttachmentPreviewLoad {
  if (!isTauri()) {
    return {
      promise: Promise.reject(new Error("附件预览需要在 Tauri 应用中运行。")),
      release: () => undefined,
    };
  }
  const key = previewKey(path, conversationId, previewPath);
  const cached = readCachedPreview(key);
  if (cached) return { promise: Promise.resolve(cached), release: () => undefined };

  let request = previewRequests.get(key);
  if (!request) {
    const requestId = crypto.randomUUID();
    const promise = invoke<string>("read_chat_attachment_preview", {
      requestId,
      path,
      previewPath: previewPath ?? null,
      conversationId: conversationId ?? null,
    }).then((dataUrl) => {
      cachePreview(key, dataUrl);
      return dataUrl;
    }).finally(() => {
      previewRequests.delete(key);
    });
    request = { requestId, promise, consumers: 0 };
    previewRequests.set(key, request);
  }
  request.consumers += 1;
  let released = false;
  return {
    promise: request.promise,
    release: () => {
      if (released) return;
      released = true;
      request!.consumers = Math.max(0, request!.consumers - 1);
      if (request!.consumers === 0 && previewRequests.get(key) === request) {
        void cancelChatAttachmentTask(request!.requestId).catch(() => undefined);
      }
    },
  };
}

export function openChatAttachment(conversationId: string, path: string) {
  if (!isTauri()) return Promise.reject(new Error("打开附件需要在 Tauri 应用中运行。"));
  return invoke<void>("open_chat_attachment", { conversationId, path });
}
