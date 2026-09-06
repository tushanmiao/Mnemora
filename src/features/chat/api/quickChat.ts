import { invoke, isTauri } from "@tauri-apps/api/core";

/** Opens the standalone quick-chat window without sharing UI state with main. */
export async function openQuickChat() {
  if (!isTauri()) {
    throw new Error("快速聊天窗口需要在 Tauri 应用中运行。");
  }
  await invoke("quick_chat_open");
}
