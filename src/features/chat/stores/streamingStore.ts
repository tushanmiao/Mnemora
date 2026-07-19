import { useCallback, useSyncExternalStore } from "react";

/**
 * 流式文本是高频、短生命周期的界面状态，不直接写入 Conversation。
 * Rust 事件只向这里追加增量；组件最多按 30 FPS 收到一次新快照。
 */

const STREAM_PUBLISH_INTERVAL_MS = 1000 / 30;

export interface StreamingMessageSnapshot {
  content: string;
  revision: number;
}

interface StreamingMessageEntry {
  messageId: string;
  pendingText: string;
  snapshot: StreamingMessageSnapshot;
  listeners: Set<() => void>;
  frameId: number | null;
  lastPublishedAt: number;
}

const entries = new Map<string, StreamingMessageEntry>();

function publishEntry(entry: StreamingMessageEntry, publishedAt: number) {
  if (!entry.pendingText) return;

  entry.snapshot = {
    content: entry.snapshot.content + entry.pendingText,
    revision: entry.snapshot.revision + 1,
  };
  entry.pendingText = "";
  entry.lastPublishedAt = publishedAt;
  entry.listeners.forEach((listener) => listener());
}

function schedulePublish(entry: StreamingMessageEntry) {
  if (entry.frameId !== null) return;

  const publishOnFrame = (now: number) => {
    if (entries.get(entry.messageId) !== entry) return;

    const elapsed = now - entry.lastPublishedAt;
    if (elapsed < STREAM_PUBLISH_INTERVAL_MS) {
      entry.frameId = requestAnimationFrame(publishOnFrame);
      return;
    }

    entry.frameId = null;
    publishEntry(entry, now);
    if (entry.pendingText) schedulePublish(entry);
  };

  entry.frameId = requestAnimationFrame(publishOnFrame);
}

export function startStreamingMessage(messageId: string) {
  const previous = entries.get(messageId);
  if (previous?.frameId !== null && previous?.frameId !== undefined) {
    cancelAnimationFrame(previous.frameId);
  }

  entries.set(messageId, {
    messageId,
    pendingText: "",
    snapshot: { content: "", revision: 0 },
    listeners: new Set(),
    frameId: null,
    lastPublishedAt: performance.now(),
  });
}

export function appendStreamingDelta(messageId: string, delta: string) {
  const entry = entries.get(messageId);
  if (!entry || !delta) return;

  entry.pendingText += delta;
  schedulePublish(entry);
}

/**
 * 取得最终完整文本并移除临时状态。调用方随后只需把终态写回 Conversation 一次。
 */
export function consumeStreamingMessage(messageId: string) {
  const entry = entries.get(messageId);
  if (!entry) return null;

  if (entry.frameId !== null) cancelAnimationFrame(entry.frameId);
  const content = entry.snapshot.content + entry.pendingText;
  entries.delete(messageId);
  return content;
}

function subscribeToStreamingMessage(messageId: string, listener: () => void) {
  const entry = entries.get(messageId);
  if (!entry) return () => undefined;

  entry.listeners.add(listener);
  return () => entry.listeners.delete(listener);
}

function getStreamingMessageSnapshot(messageId: string) {
  return entries.get(messageId)?.snapshot ?? null;
}

/** 只有处于生成状态的助手消息才会真正订阅 store。 */
export function useStreamingMessage(messageId: string, enabled: boolean) {
  const subscribe = useCallback(
    (listener: () => void) => (
      enabled ? subscribeToStreamingMessage(messageId, listener) : () => undefined
    ),
    [enabled, messageId],
  );
  const getSnapshot = useCallback(
    () => (enabled ? getStreamingMessageSnapshot(messageId) : null),
    [enabled, messageId],
  );

  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
