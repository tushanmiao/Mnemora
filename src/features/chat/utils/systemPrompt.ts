import { DEFAULT_GLOBAL_SYSTEM_PROMPT } from "../../../types/appSettings";
import type { ResponseLanguage } from "../../../types/appSettings";

/** 设置页首次展示的全局提示词默认值；发送时不会再隐式追加第二份。 */
export const DEFAULT_CHAT_SYSTEM_PROMPT = DEFAULT_GLOBAL_SYSTEM_PROMPT;

const RESPONSE_LANGUAGE_PROMPTS: Partial<Record<ResponseLanguage, string>> = {
  zh: "请使用简体中文回答。",
  zhHant: "請使用繁體中文回答。",
  en: "Please answer in English.",
};

export interface SystemPromptComposition {
  globalPrompt?: string;
  conversationPrompt?: string;
  contextSummary?: string;
  responseLanguage?: ResponseLanguage;
}

/** 组合普通 Chat 的系统提示词；空白片段不会产生额外换行。 */
export function composeChatSystemPrompt({
  globalPrompt = "",
  conversationPrompt = "",
  contextSummary = "",
  responseLanguage = "followInput",
}: SystemPromptComposition): string {
  return [
    globalPrompt.trim(),
    conversationPrompt.trim(),
    contextSummary.trim(),
    RESPONSE_LANGUAGE_PROMPTS[responseLanguage] ?? "",
  ].filter(Boolean).join("\n\n");
}
