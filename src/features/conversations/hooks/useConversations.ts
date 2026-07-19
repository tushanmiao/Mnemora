import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Conversation, ConversationListItem } from "../../../types/conversation";
import {
  clearStoredConversations,
  listStoredConversations,
  loadStoredConversation,
  persistConversation,
  removeStoredConversation,
} from "../api/conversations";

const DEFAULT_CONVERSATION_TITLE = "新对话";
const MAX_LOADED_CONVERSATIONS = 8;
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
    .find((message) => message.content || message.errorMessage);
  return {
    id: conversation.id,
    title: conversation.title,
    preview: lastMessage?.content || lastMessage?.errorMessage || "暂无消息",
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
    return right.updatedAt - left.updatedAt;
  });
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
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(
    STARTS_IN_TAURI ? null : initialConversation.id,
  );
  const requestInFlightRef = useRef(false);
  const selectionVersionRef = useRef(0);
  const conversationSaveChainsRef = useRef(new Map<string, Promise<void>>());

  const currentConversation = useMemo(
    () => conversations.find((item) => item.id === currentConversationId) ?? null,
    [conversations, currentConversationId],
  );

  const cacheConversation = useCallback((conversation: Conversation, updateSummary = true) => {
    const nextCache = [
      conversation,
      ...conversationsRef.current.filter((item) => item.id !== conversation.id),
    ].slice(0, MAX_LOADED_CONVERSATIONS);
    conversationsRef.current = nextCache;
    setConversations(nextCache);
    if (updateSummary) {
      const summary = toConversationListItem(conversation);
      setConversationListItems((current) => sortConversationListItems([
        summary,
        ...current.filter((item) => item.id !== conversation.id),
      ]));
    }
  }, []);

  const saveStableConversation = useCallback((conversation: Conversation) => {
    if (!STARTS_IN_TAURI) return;
    const previous = conversationSaveChainsRef.current.get(conversation.id) ?? Promise.resolve();
    const operation = previous
      .catch(() => undefined)
      .then(() => persistConversation(conversation))
      .then((summary) => {
        if (!summary) return;
        setConversationListItems((current) => sortConversationListItems([
          summary,
          ...current.filter((item) => item.id !== summary.id),
        ]));
      })
      .catch((error) => console.error("保存会话失败", error));
    conversationSaveChainsRef.current.set(conversation.id, operation);
    void operation.finally(() => {
      if (conversationSaveChainsRef.current.get(conversation.id) === operation) {
        conversationSaveChainsRef.current.delete(conversation.id);
      }
    });
  }, []);

  useEffect(() => {
    if (!STARTS_IN_TAURI) return;
    let cancelled = false;
    void (async () => {
      try {
        const items = await listStoredConversations();
        if (cancelled) return;
        setConversationListItems(items);
        if (items.length > 0) {
          const conversation = await loadStoredConversation(items[0].id);
          if (cancelled) return;
          cacheConversation(conversation, false);
          setCurrentConversationId(conversation.id);
        } else {
          const conversation = createConversation();
          cacheConversation(conversation);
          setCurrentConversationId(conversation.id);
          saveStableConversation(conversation);
        }
      } catch (error) {
        if (cancelled) return;
        console.error("加载本地会话失败", error);
        const conversation = createConversation();
        cacheConversation(conversation);
        setCurrentConversationId(conversation.id);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [cacheConversation, saveStableConversation]);

  const createNewConversation = useCallback(() => {
    const conversation = createConversation();
    cacheConversation(conversation);
    setCurrentConversationId(conversation.id);
    onNavigateToChat();
    saveStableConversation(conversation);
  }, [cacheConversation, onNavigateToChat, saveStableConversation]);

  const selectConversation = useCallback((conversationId: string) => {
    setCurrentConversationId(conversationId);
    onNavigateToChat();
    const cached = conversationsRef.current.some((item) => item.id === conversationId);
    if (cached || !STARTS_IN_TAURI) return;
    const version = ++selectionVersionRef.current;
    void loadStoredConversation(conversationId)
      .then((conversation) => {
        if (version === selectionVersionRef.current) cacheConversation(conversation, false);
      })
      .catch((error) => console.error("加载会话失败", error));
  }, [cacheConversation, onNavigateToChat]);

  const deleteConversation = useCallback((conversationId: string) => {
    if (requestInFlightRef.current && currentConversationId === conversationId) return;
    const deletedIndex = conversationListItems.findIndex((item) => item.id === conversationId);
    const remainingItems = conversationListItems.filter((item) => item.id !== conversationId);
    setConversationListItems(remainingItems);
    const nextCache = conversationsRef.current.filter((item) => item.id !== conversationId);
    conversationsRef.current = nextCache;
    setConversations(nextCache);
    if (currentConversationId === conversationId) {
      const next = remainingItems[deletedIndex] ?? remainingItems[deletedIndex - 1] ?? null;
      if (next) selectConversation(next.id);
      else setCurrentConversationId(null);
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
  }, [conversationListItems, currentConversationId, selectConversation]);

  const clearConversations = useCallback(() => {
    if (requestInFlightRef.current) return;
    conversationsRef.current = [];
    setConversations([]);
    setConversationListItems([]);
    setCurrentConversationId(null);
    if (!STARTS_IN_TAURI) return;
    const pendingWrites = [...conversationSaveChainsRef.current.values()];
    void Promise.allSettled(pendingWrites)
      .then(() => clearStoredConversations())
      .catch((error) => console.error("清空会话失败", error));
  }, []);

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
    currentConversation,
    currentConversationId,
    cacheConversation,
    saveStableConversation,
    createNewConversation,
    selectConversation,
    deleteConversation,
    clearConversations,
    updateCurrentConversation,
  };
}
