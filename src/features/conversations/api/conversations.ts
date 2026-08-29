import { invoke, isTauri } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  Conversation,
  ConversationListItem,
  ConversationListPage,
} from "../../../types/conversation";
import type { LibraryNote } from "../../library/types";

export const CONVERSATION_PAGE_SIZE = 50;

/** 读取轻量索引，不加载每个会话的完整消息。 */
export function listStoredConversations(offset = 0, limit = CONVERSATION_PAGE_SIZE) {
  if (!isTauri()) {
    return Promise.resolve<ConversationListPage>({ items: [], offset, total: 0, hasMore: false });
  }
  return invoke<ConversationListPage>("list_conversations", { offset, limit });
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

export function renameStoredConversation(conversationId: string, title: string) {
  if (!isTauri()) return Promise.resolve<ConversationListItem | null>(null);
  return invoke<ConversationListItem>("rename_conversation", { conversationId, title });
}

export function removeStoredConversation(conversationId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("delete_conversation", { conversationId });
}

export function clearStoredConversations() {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("clear_conversations");
}

export async function exportStoredConversation(
  conversationId: string,
  title: string,
  format: "markdown" | "json",
) {
  if (!isTauri()) throw new Error("会话导出需要在 Tauri 应用中运行。");
  const extension = format === "markdown" ? "md" : "json";
  const safeTitle = title.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-").slice(0, 80) || "mnemora-conversation";
  const path = format === "markdown"
    ? await open({
        title: "选择 Markdown 会话包的保存位置",
        directory: true,
        multiple: false,
      })
    : await save({
        title: "导出会话为 JSON",
        defaultPath: `${safeTitle}.${extension}`,
        filters: [{ name: "JSON", extensions: [extension] }],
      });
  if (typeof path !== "string") return false;
  await invoke<string>("export_conversation", { conversationId, path, format });
  return true;
}

/**
 * 把整个对话原文转存为笔记库中的独立笔记。
 * Rust 侧按 ID 加载并复用导出 Markdown 的渲染逻辑，完整消息不经过 IPC 往返。
 */
export function saveStoredConversationAsNote(conversationId: string) {
  if (!isTauri()) {
    return Promise.reject(new Error("保存笔记需要在 Tauri 应用中运行。"));
  }
  return invoke<LibraryNote>("save_conversation_as_note", { conversationId });
}
