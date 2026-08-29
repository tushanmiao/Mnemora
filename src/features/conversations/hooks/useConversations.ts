import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Conversation, ConversationListItem } from "../../../types/conversation";
import {
  CONVERSATION_PAGE_SIZE,
  clearStoredConversations,
  listStoredConversations,
  loadStoredConversation,
  persistConversation,
  renameStoredConversation,
  removeStoredConversation,
} from "../api/conversations";
import { trimConversationCache } from "../utils/conversationCache";

const DEFAULT_CONVERSATION_TITLE = "新对话";
const STARTS_IN_TAURI = isTauri();

function createConversation(): Conversation {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    title: DEFAULT_CONVERSATION_TITLE,
    messages: [],
    assistantId: null,
    providerId: null,
    modelId: null,
    thinkingEnabled: null,
    reasoningEffort: null,
    systemPrompt: "",
    contextSummary: "",
    compressedUntilMessageId: null,
    contextCompressionCount: 0,
    enabledSkillIds: [],
    linkedLibraryItemIds: [],
    permissionMode: "askSensitive",
    projectId: null,
    collectionId: null,
    pinned: false,
    createdAt: now,
    updatedAt: now,
  };
}

function toConversationListItem(conversation: Conversation): ConversationListItem {
  const lastMessage = [...conversation.messages]
    .reverse()
    .find((message) => (
      message.content
      || message.errorMessage
      || (message.attachments?.length ?? 0) > 0
      || (message.literatureReferences?.length ?? 0) > 0
      || (message.noteReferences?.length ?? 0) > 0
    ));
  const attachmentPreview = lastMessage?.attachments?.length
    ? `附件：${lastMessage.attachments.map((attachment) => attachment.name).join("、")}`
    : "";
  return {
    id: conversation.id,
    title: conversation.title,
    preview: lastMessage?.content
      || lastMessage?.errorMessage
      || attachmentPreview
      || lastMessage?.literatureReferences?.[0]?.title
      || lastMessage?.noteReferences?.[0]?.noteTitle
      || "暂无消息",
    messageCount: conversation.messages.length,
    assistantId: conversation.assistantId,
    providerId: conversation.providerId,
    modelId: conversation.modelId,
    projectId: conversation.projectId,
    collectionId: conversation.collectionId,
    sourceKind: conversation.sourceKind ?? null,
    pinned: conversation.pinned,
    createdAt: conversation.createdAt,
    updatedAt: conversation.updatedAt,
  };
}

function sortConversationListItems(items: ConversationListItem[]) {
  return [...items].sort((left, right) => {
    if (left.pinned !== right.pinned) return left.pinned ? -1 : 1;
    if (left.updatedAt !== right.updatedAt) return right.updatedAt - left.updatedAt;
    return left.id.localeCompare(right.id);
  });
}

function mergeConversationListItems(
  current: ConversationListItem[],
  incoming: ConversationListItem[],
) {
  const byId = new Map(current.map((item) => [item.id, item]));
  for (const item of incoming) byId.set(item.id, item);
  return sortConversationListItems([...byId.values()]);
}

const initialConversation = createConversation();

type PendingConversationSave = {
  latest: Conversation;
  requestedVersion: number;
  persistedVersion: number;
  promise: Promise<void>;
};

export function useConversations(onNavigateToChat: () => void) {
  const [conversations, setConversations] = useState<Conversation[]>(() => (
    STARTS_IN_TAURI ? [] : [initialConversation]
  ));
  const conversationsRef = useRef<Conversation[]>(STARTS_IN_TAURI ? [] : [initialConversation]);
  const [conversationListItems, setConversationListItems] = useState<ConversationListItem[]>(() => (
    STARTS_IN_TAURI ? [] : [toConversationListItem(initialConversation)]
  ));
  const conversationListItemsRef = useRef<ConversationListItem[]>(
    STARTS_IN_TAURI ? [] : [toConversationListItem(initialConversation)],
  );
  const [conversationListLoading, setConversationListLoading] = useState(STARTS_IN_TAURI);
  const [conversationListError, setConversationListError] = useState("");
  const [conversationListHasMore, setConversationListHasMore] = useState(false);
  const [conversationListTotal, setConversationListTotal] = useState(
    STARTS_IN_TAURI ? 0 : 1,
  );
  const conversationListHasMoreRef = useRef(false);
  const conversationListTotalRef = useRef(STARTS_IN_TAURI ? 0 : 1);
  const nextConversationOffsetRef = useRef(0);
  const conversationPageRequestRef = useRef(false);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(
    STARTS_IN_TAURI ? null : initialConversation.id,
  );
  const [currentConversationLoading, setCurrentConversationLoading] = useState(false);
  const currentConversationIdRef = useRef<string | null>(
    STARTS_IN_TAURI ? null : initialConversation.id,
  );
  const protectedConversationIdsRef = useRef(new Set<string>());
  const requestInFlightRef = useRef(false);
  const selectionVersionRef = useRef(0);
  const activeConversationLoadRef = useRef<{ id: string; promise: Promise<boolean> } | null>(null);
  const pendingConversationSavesRef = useRef(new Map<string, PendingConversationSave>());

  const currentConversation = useMemo(
    () => conversations.find((item) => item.id === currentConversationId) ?? null,
    [conversations, currentConversationId],
  );

  const replaceConversationListItems = useCallback((items: ConversationListItem[]) => {
    conversationListItemsRef.current = items;
    setConversationListItems(items);
  }, []);

  const updateConversationListTotal = useCallback((total: number) => {
    conversationListTotalRef.current = Math.max(0, total);
    setConversationListTotal(conversationListTotalRef.current);
  }, []);

  const updateConversationListHasMore = useCallback((hasMore: boolean) => {
    conversationListHasMoreRef.current = hasMore;
    setConversationListHasMore(hasMore);
  }, []);

  const upsertConversationListItem = useCallback((item: ConversationListItem) => {
    const existed = conversationListItemsRef.current.some((current) => current.id === item.id);
    replaceConversationListItems(mergeConversationListItems(
      conversationListItemsRef.current,
      [item],
    ));
    if (!existed) updateConversationListTotal(conversationListTotalRef.current + 1);
  }, [replaceConversationListItems, updateConversationListTotal]);

  const cacheConversation = useCallback((conversation: Conversation, updateSummary = true) => {
    const nextCache = trimConversationCache(
      [conversation, ...conversationsRef.current],
      {
        currentConversationId: currentConversationIdRef.current,
        protectedConversationIds: protectedConversationIdsRef.current,
      },
    );
    conversationsRef.current = nextCache;
    setConversations(nextCache);
    if (updateSummary) {
      upsertConversationListItem(toConversationListItem(conversation));
    }
  }, [upsertConversationListItem]);

  const protectConversation = useCallback((conversationId: string) => {
    protectedConversationIdsRef.current.add(conversationId);
  }, []);

  const releaseConversation = useCallback((conversationId: string) => {
    protectedConversationIdsRef.current.delete(conversationId);
    const nextCache = trimConversationCache(conversationsRef.current, {
      currentConversationId: currentConversationIdRef.current,
      protectedConversationIds: protectedConversationIdsRef.current,
    });
    if (
      nextCache.length === conversationsRef.current.length
      && nextCache.every((conversation, index) => conversation === conversationsRef.current[index])
    ) return;
    conversationsRef.current = nextCache;
    setConversations(nextCache);
  }, []);

  const ensureCurrentConversationLoaded = useCallback(() => {
    const conversationId = currentConversationIdRef.current;
    if (!STARTS_IN_TAURI || !conversationId) return Promise.resolve(false);
    if (conversationsRef.current.some((conversation) => conversation.id === conversationId)) {
      setCurrentConversationLoading(false);
      return Promise.resolve(true);
    }
    const activeLoad = activeConversationLoadRef.current;
    if (activeLoad?.id === conversationId) return activeLoad.promise;

    const version = ++selectionVersionRef.current;
    setCurrentConversationLoading(true);
    const pendingWrite = pendingConversationSavesRef.current.get(conversationId)?.promise;
    const promise = (pendingWrite ?? Promise.resolve())
      .then(() => loadStoredConversation(conversationId))
      .then((conversation) => {
        if (
          version !== selectionVersionRef.current
          || currentConversationIdRef.current !== conversationId
        ) return false;
        cacheConversation(conversation, false);
        setCurrentConversationLoading(false);
        return true;
      })
      .catch((error) => {
        if (
          version === selectionVersionRef.current
          && currentConversationIdRef.current === conversationId
        ) {
          setCurrentConversationLoading(false);
          console.error("恢复会话失败", error);
        }
        return false;
      });
    activeConversationLoadRef.current = { id: conversationId, promise };
    void promise.finally(() => {
      if (activeConversationLoadRef.current?.promise === promise) {
        activeConversationLoadRef.current = null;
      }
    });
    return promise;
  }, [cacheConversation]);

  /** 离开 Chat/AI 面板后释放当前消息正文；持久化索引和当前 ID 仍保留。 */
  const releaseCurrentConversation = useCallback(() => {
    const conversationId = currentConversationIdRef.current;
    if (
      !STARTS_IN_TAURI
      || !conversationId
      || requestInFlightRef.current
      || protectedConversationIdsRef.current.has(conversationId)
    ) return false;
    const current = conversationsRef.current.find((conversation) => conversation.id === conversationId);
    // 空白会话可能还没有落盘，保留它可以避免返回时读取竞态。
    if (!current || current.messages.length === 0) return false;
    selectionVersionRef.current += 1;
    activeConversationLoadRef.current = null;
    conversationsRef.current = conversationsRef.current.filter(
      (conversation) => conversation.id !== conversationId,
    );
    setConversations(conversationsRef.current);
    setCurrentConversationLoading(false);
    return true;
  }, []);

  const saveStableConversation = useCallback((conversation: Conversation) => {
    if (!STARTS_IN_TAURI) return;
    const pending = pendingConversationSavesRef.current.get(conversation.id);
    if (pending) {
      pending.latest = conversation;
      pending.requestedVersion += 1;
      return;
    }
    const entry = {
      latest: conversation,
      requestedVersion: 1,
      persistedVersion: 0,
      promise: Promise.resolve(),
    } satisfies PendingConversationSave;
    entry.promise = (async () => {
      while (entry.persistedVersion < entry.requestedVersion) {
        const target = entry.latest;
        const targetVersion = entry.requestedVersion;
        try {
          const summary = await persistConversation(target);
          if (summary) upsertConversationListItem(summary);
        } catch (error) {
          console.error("保存会话失败", error);
        }
        entry.persistedVersion = targetVersion;
      }
    })().finally(() => {
      if (pendingConversationSavesRef.current.get(conversation.id) === entry) {
        pendingConversationSavesRef.current.delete(conversation.id);
      }
    });
    pendingConversationSavesRef.current.set(conversation.id, entry);
  }, [upsertConversationListItem]);

  const loadMoreConversations = useCallback(() => {
    if (
      !STARTS_IN_TAURI
      || conversationPageRequestRef.current
      || !conversationListHasMoreRef.current
    ) return;

    conversationPageRequestRef.current = true;
    setConversationListLoading(true);
    setConversationListError("");
    const requestedOffset = nextConversationOffsetRef.current;
    void listStoredConversations(requestedOffset, CONVERSATION_PAGE_SIZE)
      .then((page) => {
        replaceConversationListItems(mergeConversationListItems(
          conversationListItemsRef.current,
          page.items,
        ));
        nextConversationOffsetRef.current = page.offset + page.items.length;
        updateConversationListTotal(page.total);
        updateConversationListHasMore(page.hasMore);
      })
      .catch((error) => {
        console.error("加载更多会话失败", error);
        setConversationListError("加载更多会话失败");
      })
      .finally(() => {
        conversationPageRequestRef.current = false;
        setConversationListLoading(false);
      });
  }, [
    replaceConversationListItems,
    updateConversationListHasMore,
    updateConversationListTotal,
  ]);

  useEffect(() => {
    if (!STARTS_IN_TAURI) return;
    let cancelled = false;
    void (async () => {
      try {
        const page = await listStoredConversations(0, CONVERSATION_PAGE_SIZE);
        if (cancelled) return;
        replaceConversationListItems(page.items);
        nextConversationOffsetRef.current = page.offset + page.items.length;
        updateConversationListTotal(page.total);
        updateConversationListHasMore(page.hasMore);
        setConversationListError("");
        if (page.items.length > 0) {
          const conversation = await loadStoredConversation(page.items[0].id);
          if (cancelled) return;
          currentConversationIdRef.current = conversation.id;
          cacheConversation(conversation, false);
          setCurrentConversationId(conversation.id);
        } else {
          const conversation = createConversation();
          currentConversationIdRef.current = conversation.id;
          cacheConversation(conversation);
          setCurrentConversationId(conversation.id);
          saveStableConversation(conversation);
        }
      } catch (error) {
        if (cancelled) return;
        console.error("加载本地会话失败", error);
        setConversationListError("加载本地会话失败");
        nextConversationOffsetRef.current = 0;
        updateConversationListHasMore(true);
        const conversation = createConversation();
        currentConversationIdRef.current = conversation.id;
        cacheConversation(conversation);
        setCurrentConversationId(conversation.id);
      } finally {
        if (!cancelled) setConversationListLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    cacheConversation,
    replaceConversationListItems,
    saveStableConversation,
    updateConversationListHasMore,
    updateConversationListTotal,
  ]);

  const createNewConversation = useCallback(() => {
    const conversation = createConversation();
    selectionVersionRef.current += 1;
    activeConversationLoadRef.current = null;
    setCurrentConversationLoading(false);
    currentConversationIdRef.current = conversation.id;
    cacheConversation(conversation);
    setCurrentConversationId(conversation.id);
    onNavigateToChat();
    saveStableConversation(conversation);
  }, [cacheConversation, onNavigateToChat, saveStableConversation]);

  const selectConversation = useCallback((conversationId: string) => {
    selectionVersionRef.current += 1;
    activeConversationLoadRef.current = null;
    currentConversationIdRef.current = conversationId;
    setCurrentConversationId(conversationId);
    onNavigateToChat();
    const cached = conversationsRef.current.find((item) => item.id === conversationId);
    if (cached) {
      cacheConversation(cached, false);
      setCurrentConversationLoading(false);
      return;
    }
    void ensureCurrentConversationLoaded();
  }, [cacheConversation, ensureCurrentConversationLoaded, onNavigateToChat]);

  const deleteConversation = useCallback((conversationId: string) => {
    if (
      protectedConversationIdsRef.current.has(conversationId)
      || (requestInFlightRef.current && currentConversationId === conversationId)
    ) return;
    const deletedIndex = conversationListItems.findIndex((item) => item.id === conversationId);
    const remainingItems = conversationListItems.filter((item) => item.id !== conversationId);
    replaceConversationListItems(remainingItems);
    if (deletedIndex >= 0) {
      updateConversationListTotal(conversationListTotalRef.current - 1);
      nextConversationOffsetRef.current = Math.max(0, nextConversationOffsetRef.current - 1);
      updateConversationListHasMore(
        remainingItems.length < conversationListTotalRef.current,
      );
    }
    const nextCache = conversationsRef.current.filter((item) => item.id !== conversationId);
    conversationsRef.current = nextCache;
    setConversations(nextCache);
    if (currentConversationId === conversationId) {
      const next = remainingItems[deletedIndex] ?? remainingItems[deletedIndex - 1] ?? null;
      if (next) selectConversation(next.id);
      else {
        currentConversationIdRef.current = null;
        setCurrentConversationId(null);
        setCurrentConversationLoading(false);
      }
    }
    if (!STARTS_IN_TAURI) return;
    const previous = pendingConversationSavesRef.current.get(conversationId)?.promise ?? Promise.resolve();
    const operation = previous
      .catch(() => undefined)
      .then(() => removeStoredConversation(conversationId))
      .then(() => undefined)
      .catch((error) => console.error("删除会话失败", error));
    void operation;
  }, [
    conversationListItems,
    currentConversationId,
    replaceConversationListItems,
    selectConversation,
    updateConversationListHasMore,
    updateConversationListTotal,
  ]);

  const renameConversation = useCallback(async (conversationId: string, nextTitle: string) => {
    const title = nextTitle.trim();
    if (!title || title.length > 500) return false;
    const pendingWrite = pendingConversationSavesRef.current.get(conversationId)?.promise;
    if (pendingWrite) await pendingWrite;
    const cached = conversationsRef.current.find((item) => item.id === conversationId);
    const now = Date.now();
    const summary = STARTS_IN_TAURI
      ? await renameStoredConversation(conversationId, title)
      : cached
        ? toConversationListItem({ ...cached, title, updatedAt: now })
        : null;
    const nextCache = conversationsRef.current.map((conversation) => (
      conversation.id === conversationId
        ? { ...conversation, title, updatedAt: summary?.updatedAt ?? now }
        : conversation
    ));
    conversationsRef.current = nextCache;
    setConversations(nextCache);
    if (summary) {
      upsertConversationListItem(summary);
    } else {
      const current = conversationListItemsRef.current.find((item) => item.id === conversationId);
      if (current) upsertConversationListItem({ ...current, title, updatedAt: now });
    }
    return true;
  }, [upsertConversationListItem]);

  const clearConversations = useCallback(() => {
    if (requestInFlightRef.current) return;
    selectionVersionRef.current += 1;
    activeConversationLoadRef.current = null;
    conversationsRef.current = [];
    currentConversationIdRef.current = null;
    protectedConversationIdsRef.current.clear();
    setConversations([]);
    replaceConversationListItems([]);
    nextConversationOffsetRef.current = 0;
    updateConversationListTotal(0);
    updateConversationListHasMore(false);
    setConversationListError("");
    setCurrentConversationId(null);
    setCurrentConversationLoading(false);
    if (!STARTS_IN_TAURI) return;
    const pendingWrites = [...pendingConversationSavesRef.current.values()]
      .map((entry) => entry.promise);
    void Promise.allSettled(pendingWrites)
      .then(() => clearStoredConversations())
      .catch((error) => console.error("清空会话失败", error));
  }, [
    replaceConversationListItems,
    updateConversationListHasMore,
    updateConversationListTotal,
  ]);

  /** Slash `/clear` 使用后端优先删除，失败时不提前移除界面数据。 */
  const deleteCurrentConversationPermanently = useCallback(async () => {
    const conversationId = currentConversationIdRef.current;
    if (
      !conversationId
      || requestInFlightRef.current
      || protectedConversationIdsRef.current.has(conversationId)
    ) return false;

    selectionVersionRef.current += 1;
    activeConversationLoadRef.current = null;
    const pendingWrite = pendingConversationSavesRef.current.get(conversationId)?.promise;
    if (pendingWrite) await pendingWrite;
    if (STARTS_IN_TAURI) {
      const removed = await removeStoredConversation(conversationId);
      if (!removed) throw new Error("当前对话已经不存在，未执行清除。");
    }

    const remainingItems = conversationListItemsRef.current.filter(
      (item) => item.id !== conversationId,
    );
    replaceConversationListItems(remainingItems);
    updateConversationListTotal(conversationListTotalRef.current - 1);
    nextConversationOffsetRef.current = Math.max(0, nextConversationOffsetRef.current - 1);
    updateConversationListHasMore(remainingItems.length < conversationListTotalRef.current);
    conversationsRef.current = conversationsRef.current.filter((item) => item.id !== conversationId);
    setConversations(conversationsRef.current);
    currentConversationIdRef.current = null;
    setCurrentConversationId(null);
    setCurrentConversationLoading(false);
    return true;
  }, [
    replaceConversationListItems,
    updateConversationListHasMore,
    updateConversationListTotal,
  ]);

  const updateCurrentConversation = useCallback((
    update: (conversation: Conversation) => Conversation,
  ) => {
    const current = conversationsRef.current.find((item) => item.id === currentConversationId);
    if (!current) return;
    const next = update(current);
    cacheConversation(next);
    saveStableConversation(next);
  }, [cacheConversation, currentConversationId, saveStableConversation]);

  return {
    conversationsRef,
    requestInFlightRef,
    conversationListItems,
    conversationListLoading,
    conversationListError,
    conversationListHasMore,
    conversationListTotal,
    loadMoreConversations,
    currentConversation,
    currentConversationId,
    cacheConversation,
    saveStableConversation,
    protectConversation,
    releaseConversation,
    ensureCurrentConversationLoaded,
    releaseCurrentConversation,
    currentConversationLoading,
    createNewConversation,
    selectConversation,
    deleteConversation,
    renameConversation,
    clearConversations,
    deleteCurrentConversationPermanently,
    updateCurrentConversation,
  };
}
