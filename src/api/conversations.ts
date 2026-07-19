import { invoke, isTauri } from "@tauri-apps/api/core";
import type { Conversation, ConversationListItem } from "../types/chat";

/** 读取轻量索引，不加载每个会话的完整消息。 */
export function listStoredConversations() {
  if (!isTauri()) return Promise.resolve<ConversationListItem[]>([]);
  return invoke<ConversationListItem[]>("list_conversations");
}

/** 用户选择会话时再按 ID 加载完整 JSON。 */
export function loadStoredConversation(conversationId: string) {
  return invoke<Conversation>("load_conversation", { conversationId });
}

/** 只在消息或配置进入稳定状态后调用，不用于每个流式文本增量。 */
export function persistConversation(conversation: Conversation) {
  if (!isTauri()) return Promise.resolve<ConversationListItem | null>(null);
  return invoke<ConversationListItem>("save_conversation", { conversation });
}

export function removeStoredConversation(conversationId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("delete_conversation", { conversationId });
}

export function clearStoredConversations() {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("clear_conversations");
}
