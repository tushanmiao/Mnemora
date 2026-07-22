import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Conversation, ConversationListItem } from "../../../types/conversation";
import {
  CONVERSATION_PAGE_SIZE,
  clearStoredConversations,
  listStoredConversations,
  loadStoredConversation,
  persistConversation,
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
    systemPrompt: "",
    contextSummary: "",
    compressedUntilMessageId: null,
    contextCompressionCount: 0,
    enabledSkillIds: [],
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
      message.content || message.errorMessage || (message.attachments?.length ?? 0) > 0
    ));
  const attachmentPreview = lastMessage?.attachments?.length
    ? `附件：${lastMessage.attachments.map((attachment) => attachment.name).join("、")}`
    : "";
  return {
    id: conversation.id,
    title: conversation.title,
    preview: lastMessage?.content || lastMessage?.errorMessage || attachmentPreview || "暂无消息",
    messageCount: conversation.messages.length,
    assistantId: conversation.assistantId,
    providerId: conversation.providerId,
    modelId: conversation.modelId,
    projectId: conversation.projectId,
    collectionId: conversation.collectionId,
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
  const currentConversationIdRef = useRef<string | null>(
    STARTS_IN_TAURI ? null : initialConversation.id,
  );
  const protectedConversationIdsRef = useRef(new Set<string>());
  const requestInFlightRef = useRef(false);
  const selectionVersionRef = useRef(0);
  const conversationSaveChainsRef = useRef(new Map<string, Promise<void>>());

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

  const saveStableConversation = useCallback((conversation: Conversation) => {
    if (!STARTS_IN_TAURI) return;
    const previous = conversationSaveChainsRef.current.get(conversation.id) ?? Promise.resolve();
    const operation = previous
      .catch(() => undefined)
      .then(() => persistConversation(conversation))
      .then((summary) => {
        if (!summary) return;
        upsertConversationListItem(summary);
      })
      .catch((error) => console.error("保存会话失败", error));
    conversationSaveChainsRef.current.set(conversation.id, operation);
    void operation.finally(() => {
      if (conversationSaveChainsRef.current.get(conversation.id) === operation) {
        conversationSaveChainsRef.current.delete(conversation.id);
      }
    });
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
    currentConversationIdRef.current = conversation.id;
    cacheConversation(conversation);
    setCurrentConversationId(conversation.id);
    onNavigateToChat();
    saveStableConversation(conversation);
  }, [cacheConversation, onNavigateToChat, saveStableConversation]);

  const selectConversation = useCallback((conversationId: string) => {
    currentConversationIdRef.current = conversationId;
    setCurrentConversationId(conversationId);
    onNavigateToChat();
    const cached = conversationsRef.current.find((item) => item.id === conversationId);
    if (cached) {
      cacheConversation(cached, false);
      return;
    }
    if (!STARTS_IN_TAURI) return;
    const version = ++selectionVersionRef.current;
    void loadStoredConversation(conversationId)
      .then((conversation) => {
        if (version === selectionVersionRef.current) cacheConversation(conversation, false);
      })
      .catch((error) => console.error("加载会话失败", error));
  }, [cacheConversation, onNavigateToChat]);

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
      }
    }
    if (!STARTS_IN_TAURI) return;
    const previous = conversationSaveChainsRef.current.get(conversationId) ?? Promise.resolve();
    const operation = previous
      .catch(() => undefined)
      .then(() => removeStoredConversation(conversationId))
      .then(() => undefined)
      .catch((error) => console.error("删除会话失败", error));
    conversationSaveChainsRef.current.set(conversationId, operation);
    void operation.finally(() => {
      if (conversationSaveChainsRef.current.get(conversationId) === operation) {
        conversationSaveChainsRef.current.delete(conversationId);
      }
    });
  }, [
    conversationListItems,
    currentConversationId,
    replaceConversationListItems,
    selectConversation,
    updateConversationListHasMore,
    updateConversationListTotal,
  ]);

  const clearConversations = useCallback(() => {
    if (requestInFlightRef.current) return;
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
    if (!STARTS_IN_TAURI) return;
    const pendingWrites = [...conversationSaveChainsRef.current.values()];
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

    const pendingWrite = conversationSaveChainsRef.current.get(conversationId);
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
    createNewConversation,
    selectConversation,
    deleteConversation,
    clearConversations,
    deleteCurrentConversationPermanently,
    updateCurrentConversation,
  };
}
