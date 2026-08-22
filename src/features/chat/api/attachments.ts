import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import type { ChatAttachment, PendingChatAttachment } from "../../../types/attachment";
import { registerResource } from "../../../runtime/resources/ResourceRegistry";

const MAX_PREVIEW_CACHE_ITEMS = 32;
const MAX_PREVIEW_CACHE_BYTES = 8 * 1024 * 1024;

export type AttachmentDisplaySource = {
  kind: "asset" | "data";
  value: string;
  width?: number;
  height?: number;
};
export type RenderableAttachmentSource = {
  src: string;
  width?: number;
  height?: number;
};
type PreviewCacheEntry = {
  source: RenderableAttachmentSource;
  bytes: number;
  registration: ReturnType<typeof registerResource>;
};
type PreviewRequest = {
  requestId: string;
  promise: Promise<RenderableAttachmentSource>;
  consumers: number;
};

export type AttachmentPreviewLoad = {
  promise: Promise<RenderableAttachmentSource>;
  release: () => void;
};

export type AttachmentImageLoad = AttachmentPreviewLoad;

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
  cached.registration.touch();
  return cached.source;
}

function cachePreview(key: string, source: AttachmentDisplaySource) {
  // Pending previews are still Data URLs and must not remain in a global cache.
  if (source.kind === "data") return toRenderableSource(source);
  const renderable = toRenderableSource(source);
  const bytes = renderable.src.length * 2;
  if (bytes > MAX_PREVIEW_CACHE_BYTES) return;
  const previous = previewCache.get(key);
  if (previous) {
    previewCacheBytes -= previous.bytes;
    previous.registration.release();
  }
  previewCache.delete(key);
  const registration = registerResource({
    owner: `attachment-preview:${key}`,
    kind: "cache",
    estimatedBytes: bytes,
    backgroundReleasable: true,
    release: () => removeCachedPreview(key),
  });
  previewCache.set(key, { source: renderable, bytes, registration });
  previewCacheBytes += bytes;
  while (previewCache.size > MAX_PREVIEW_CACHE_ITEMS || previewCacheBytes > MAX_PREVIEW_CACHE_BYTES) {
    const oldest = previewCache.entries().next().value as [string, PreviewCacheEntry] | undefined;
    if (!oldest) break;
    previewCache.delete(oldest[0]);
    previewCacheBytes -= oldest[1].bytes;
    oldest[1].registration.release();
  }
  return renderable;
}

function removeCachedPreview(key: string) {
  const cached = previewCache.get(key);
  if (!cached) return;
  previewCache.delete(key);
  previewCacheBytes = Math.max(0, previewCacheBytes - cached.bytes);
  cached.registration.release();
}

function toRenderableSource(source: AttachmentDisplaySource) {
  return {
    src: source.kind === "asset" ? convertFileSrc(source.value) : source.value,
    width: source.width,
    height: source.height,
  };
}

export function clearAttachmentPreviewCache() {
  for (const key of [...previewCache.keys()]) removeCachedPreview(key);
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
    const promise = invoke<AttachmentDisplaySource>("read_chat_attachment_preview", {
      requestId,
      path,
      previewPath: previewPath ?? null,
      conversationId: conversationId ?? null,
    }).then((source) => {
      const cachedSource = cachePreview(key, source);
      return cachedSource ?? toRenderableSource(source);
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

export function loadChatAttachmentImage(
  conversationId: string,
  path: string,
): AttachmentImageLoad {
  if (!isTauri()) {
    return {
      promise: Promise.reject(new Error("图片查看需要在 Tauri 应用中运行。")),
      release: () => undefined,
    };
  }
  const requestId = crypto.randomUUID();
  let released = false;
  return {
    promise: invoke<AttachmentDisplaySource>("read_chat_attachment_image", {
      requestId,
      conversationId,
      path,
    }).then(toRenderableSource),
    release: () => {
      if (released) return;
      released = true;
      void cancelChatAttachmentTask(requestId).catch(() => undefined);
    },
  };
}

export function openChatAttachment(conversationId: string, path: string) {
  if (!isTauri()) return Promise.reject(new Error("打开附件需要在 Tauri 应用中运行。"));
  return invoke<void>("open_chat_attachment", { conversationId, path });
}
