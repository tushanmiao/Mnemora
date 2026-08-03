import { useCallback, useEffect, useRef, useState } from "react";
import {
  loadStoredConversation,
  saveStoredConversationAsNote,
} from "../../features/conversations/api/conversations";
import {
  saveMessageAsNote,
  summarizeConversationToNote,
} from "../../features/chat/utils/noteGeneration";
import type { Conversation } from "../../types/conversation";
import type { AppSettings } from "../../types/appSettings";
import type { ModelSettings } from "../../types/modelSettings";
import { resolveConversationModel } from "../../types/modelSettings";

type NoteFeedback = {
  kind: "progress" | "success" | "error";
  text: string;
};

function noteErrorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return String(error);
}

export function useNoteActions({
  currentConversation,
  currentConversationId,
  modelSettings,
  appSettings,
}: {
  currentConversation: Conversation | null;
  currentConversationId: string | null;
  modelSettings: ModelSettings;
  appSettings: AppSettings;
}) {
  const [feedback, setFeedback] = useState<NoteFeedback | null>(null);
  const feedbackTimerRef = useRef<number | null>(null);
  const summaryBusyRef = useRef(false);

  const showFeedback = useCallback((kind: NoteFeedback["kind"], text: string) => {
    if (feedbackTimerRef.current !== null) window.clearTimeout(feedbackTimerRef.current);
    feedbackTimerRef.current = null;
    setFeedback({ kind, text });
    if (kind !== "progress") {
      feedbackTimerRef.current = window.setTimeout(() => setFeedback(null), 4_000);
    }
  }, []);

  useEffect(() => () => {
    if (feedbackTimerRef.current !== null) window.clearTimeout(feedbackTimerRef.current);
  }, []);

  const saveConversationAsNote = useCallback((conversationId: string) => {
    void saveStoredConversationAsNote(conversationId)
      .then((note) => showFeedback("success", `已保存为笔记「${note.title}」`))
      .catch((error) => showFeedback("error", `保存笔记失败：${noteErrorText(error)}`));
  }, [showFeedback]);

  const summarizeConversationAsNote = useCallback(async (conversationId: string) => {
    if (summaryBusyRef.current) {
      showFeedback("error", "已有一个总结任务正在进行，请稍候再试。");
      return;
    }
    summaryBusyRef.current = true;
    showFeedback("progress", "正在用模型总结对话…");
    try {
      const conversation = conversationId === currentConversationId && currentConversation
        ? currentConversation
        : await loadStoredConversation(conversationId);
      const model = resolveConversationModel(
        modelSettings,
        conversation.providerId,
        conversation.modelId,
      );
      if (!model) {
        showFeedback("error", "请先在设置中配置可用的默认模型。");
        return;
      }
      const note = await summarizeConversationToNote(conversation, model, {
        maxOutputTokens: appSettings.maxOutputTokens,
        thinkingEnabled: appSettings.thinkingEnabled,
      });
      showFeedback("success", `已生成总结笔记「${note.title}」`);
    } catch (error) {
      showFeedback("error", `总结失败：${noteErrorText(error)}`);
    } finally {
      summaryBusyRef.current = false;
    }
  }, [
    appSettings.maxOutputTokens,
    appSettings.thinkingEnabled,
    currentConversation,
    currentConversationId,
    modelSettings,
    showFeedback,
  ]);

  const saveMessage = useCallback(async (messageId: string) => {
    if (!currentConversation) return false;
    try {
      const note = await saveMessageAsNote(currentConversation, messageId);
      showFeedback("success", `已保存为笔记「${note.title}」`);
      return true;
    } catch (error) {
      showFeedback("error", `保存笔记失败：${noteErrorText(error)}`);
      return false;
    }
  }, [currentConversation, showFeedback]);

  return {
    feedback,
    saveConversationAsNote,
    summarizeConversationAsNote,
    saveMessage,
  };
}
