import { useCallback, useSyncExternalStore } from "react";

/**
 * 流式文本是高频、短生命周期的界面状态，不直接写入 Conversation。
 * Rust 事件只向这里追加增量；组件最多按 30 FPS 收到一次新快照。
 */

const STREAM_PUBLISH_INTERVAL_MS = 1000 / 30;

export interface StreamingMessageSnapshot {
  content: string;
  reasoning: string;
  revision: number;
}

interface StreamingMessageEntry {
  messageId: string;
  pendingText: string;
  pendingReasoning: string;
  snapshot: StreamingMessageSnapshot;
  listeners: Set<() => void>;
  frameId: number | null;
  lastPublishedAt: number;
}

const entries = new Map<string, StreamingMessageEntry>();

function cancelScheduledPublish(entry: StreamingMessageEntry) {
  if (entry.frameId === null) return;
  cancelAnimationFrame(entry.frameId);
  entry.frameId = null;
}

function disposeEntry(entry: StreamingMessageEntry, notifyListeners: boolean) {
  cancelScheduledPublish(entry);
  if (entries.get(entry.messageId) === entry) entries.delete(entry.messageId);
  entry.pendingText = "";
  entry.pendingReasoning = "";
  if (notifyListeners) entry.listeners.forEach((listener) => listener());
  entry.listeners.clear();
}

function publishEntry(entry: StreamingMessageEntry, publishedAt: number) {
  if (!entry.pendingText && !entry.pendingReasoning) return;

  entry.snapshot = {
    content: entry.snapshot.content + entry.pendingText,
    reasoning: entry.snapshot.reasoning + entry.pendingReasoning,
    revision: entry.snapshot.revision + 1,
  };
  entry.pendingText = "";
  entry.pendingReasoning = "";
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
    if (entry.pendingText || entry.pendingReasoning) schedulePublish(entry);
  };

  entry.frameId = requestAnimationFrame(publishOnFrame);
}

export function startStreamingMessage(messageId: string) {
  const previous = entries.get(messageId);
  if (previous) cancelScheduledPublish(previous);

  const nextEntry: StreamingMessageEntry = {
    messageId,
    pendingText: "",
    pendingReasoning: "",
    snapshot: { content: "", reasoning: "", revision: 0 },
    // 相同消息重新开始时沿用订阅集合，避免组件仍订阅已经废弃的 entry。
    listeners: previous?.listeners ?? new Set(),
    frameId: null,
    lastPublishedAt: performance.now(),
  };
  entries.set(messageId, nextEntry);
  nextEntry.listeners.forEach((listener) => listener());
}

export function appendStreamingDelta(messageId: string, delta: string) {
  const entry = entries.get(messageId);
  if (!entry || !delta) return;

  entry.pendingText += delta;
  schedulePublish(entry);
}

export function appendStreamingReasoningDelta(messageId: string, delta: string) {
  const entry = entries.get(messageId);
  if (!entry || !delta) return;

  entry.pendingReasoning += delta;
  schedulePublish(entry);
}

/**
 * 取得最终完整文本并移除临时状态。调用方随后只需把终态写回 Conversation 一次。
 */
export function consumeStreamingMessage(messageId: string) {
  const entry = entries.get(messageId);
  if (!entry) return null;

  const content = entry.snapshot.content + entry.pendingText;
  const reasoning = entry.snapshot.reasoning + entry.pendingReasoning;
  disposeEntry(entry, false);
  return { content, reasoning };
}

/** 丢弃一条流式消息，不把临时文本写入 Conversation。 */
export function discardStreamingMessage(messageId: string) {
  const entry = entries.get(messageId);
  if (entry) disposeEntry(entry, true);
}

/** Chat Runtime 卸载时释放全部 RAF、订阅和临时字符串。 */
export function resetAllStreamingMessages() {
  for (const entry of [...entries.values()]) disposeEntry(entry, true);
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
