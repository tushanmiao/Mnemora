import type { Conversation } from "../../../types/conversation";
import type { SelectedModel } from "../hooks/useChatRuntime";
import { completeChat } from "../api/chat";
import { createLibraryNoteWithSources } from "../../library/api/library";
import type { LibraryNote, NoteSourceCreate } from "../../library/types";
import {
  deepNoteAnalysisTranscript,
  noteSummaryTranscript,
  summarizeConversationToNote,
} from "../utils/noteGeneration";
import { assembleDeepNote, sectionTail, type DraftedSection } from "./assemble";
import { parseDeepNoteOutline, type DeepNoteOutline } from "./outlineSchema";
import {
  ANALYST_SYSTEM_PROMPT,
  STRICT_JSON_RETRY_SUFFIX,
  analystUserPrompt,
  sectionSystemPrompt,
  sectionUserPrompt,
} from "./stagePrompts";

export type DeepNoteProgress =
  | { phase: "analyzing"; message: string }
  | { phase: "drafting"; current: number; total: number; message: string }
  | { phase: "assembling" | "persisting"; message: string };

export interface DeepNoteRunOptions {
  maxOutputTokens: number;
  thinkingEnabled: boolean;
  retryAttempts: number;
  signal?: AbortSignal;
  onProgress?: (progress: DeepNoteProgress) => void;
}

export interface DeepNotePrepared {
  conversation: Conversation;
  model: SelectedModel;
  transcript: string;
  outline: DeepNoteOutline;
  options: DeepNoteRunOptions;
  degradedNote?: LibraryNote;
}

export interface DeepNoteResult {
  note: LibraryNote;
  warnings: string[];
  degraded: boolean;
}

function throwIfCancelled(signal?: AbortSignal) {
  if (signal?.aborted) throw new DOMException("深度笔记生成已取消。", "AbortError");
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException
    ? error.name === "AbortError"
    : error instanceof Error && error.name === "AbortError";
}

function validMessageIds(conversation: Conversation): Set<string> {
  return new Set(conversation.messages.map((message) => message.id));
}

async function callAnalyst(
  conversation: Conversation,
  model: SelectedModel,
  transcript: string,
  options: DeepNoteRunOptions,
  adjustment: string,
  strict = false,
): Promise<DeepNoteOutline> {
  throwIfCancelled(options.signal);
  const response = await completeChat({
    providerId: model.provider.id,
    modelId: model.model.id,
    conversationId: conversation.id,
    messageId: crypto.randomUUID(),
    operation: "deepNote",
    systemPrompt: strict ? `${ANALYST_SYSTEM_PROMPT}\n\n${STRICT_JSON_RETRY_SUFFIX}` : ANALYST_SYSTEM_PROMPT,
    messages: [{ role: "user", content: analystUserPrompt(transcript, adjustment) }],
    options: {
      maxOutputTokens: Math.min(8_192, options.maxOutputTokens),
      thinkingEnabled: options.thinkingEnabled,
    },
  });
  throwIfCancelled(options.signal);
  return parseDeepNoteOutline(response.text, validMessageIds(conversation));
}

export async function prepareDeepNote(
  conversation: Conversation,
  model: SelectedModel,
  options: DeepNoteRunOptions,
  adjustment = "",
): Promise<DeepNotePrepared> {
  const transcript = noteSummaryTranscript(conversation);
  if (!transcript) throw new Error("对话还没有可以生成深度笔记的消息。");
  const analysisTranscript = deepNoteAnalysisTranscript(conversation);
  options.onProgress?.({ phase: "analyzing", message: adjustment ? "正在按补充要求调整提纲…" : "正在分析知识结构…" });
  try {
    const outline = await callAnalyst(conversation, model, analysisTranscript, options, adjustment);
    return { conversation, model, transcript, outline, options };
  } catch (error) {
    if (isAbortError(error) || options.signal?.aborted) throw error;
    try {
      const outline = await callAnalyst(conversation, model, analysisTranscript, options, adjustment, true);
      return { conversation, model, transcript, outline, options };
    } catch (retryError) {
      if (isAbortError(retryError) || options.signal?.aborted) throw retryError;
      throwIfCancelled(options.signal);
      const degradedNote = await summarizeConversationToNote(conversation, model, options);
      return {
        conversation,
        model,
        transcript,
        outline: { title: degradedNote.title, summary: "", weakPoints: [], sections: [] },
        options,
        degradedNote,
      };
    }
  }
}

async function draftSection(
  prepared: DeepNotePrepared,
  outline: DeepNoteOutline,
  index: number,
  previousTail: string,
): Promise<DraftedSection> {
  const section = outline.sections[index];
  const attempts = Math.max(1, prepared.options.retryAttempts);
  let lastError: unknown;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    throwIfCancelled(prepared.options.signal);
    try {
      const response = await completeChat({
        providerId: prepared.model.provider.id,
        modelId: prepared.model.model.id,
        conversationId: prepared.conversation.id,
        messageId: crypto.randomUUID(),
        operation: "deepNote",
        systemPrompt: sectionSystemPrompt(),
        messages: [{
          role: "user",
          content: sectionUserPrompt({ outline, section, transcript: prepared.transcript, previousTail }),
        }],
        options: {
          maxOutputTokens: Math.min(16_384, prepared.options.maxOutputTokens),
          thinkingEnabled: prepared.options.thinkingEnabled,
        },
      });
      throwIfCancelled(prepared.options.signal);
      const markdown = response.text.trim();
      if (!markdown) throw new Error("模型返回了空章节。");
      return { section, markdown };
    } catch (error) {
      if (isAbortError(error) || prepared.options.signal?.aborted) throw error;
      lastError = error;
    }
  }
  return {
    section,
    markdown: `## ${section.heading}\n\n> [本章生成失败，可稍后重试]\n\n> 错误：${lastError instanceof Error ? lastError.message : String(lastError)}`,
    failed: true,
  };
}

function noteSources(conversationId: string, outline: DeepNoteOutline): NoteSourceCreate[] {
  return outline.sections.flatMap((section) => {
    const conversationSources = section.sourceMessageIds.length > 0
      ? section.sourceMessageIds.map((messageId): NoteSourceCreate => ({
          sectionId: section.id,
          origin: "conversation",
          conversationId,
          messageId,
        }))
      : [{ sectionId: section.id, origin: "conversation", conversationId } satisfies NoteSourceCreate];
    return section.needsSupplement
      ? [...conversationSources, { sectionId: section.id, origin: "aiSupplement" } satisfies NoteSourceCreate]
      : conversationSources;
  });
}

export async function generateDeepNote(
  prepared: DeepNotePrepared,
  outline: DeepNoteOutline,
): Promise<DeepNoteResult> {
  if (prepared.degradedNote) return { note: prepared.degradedNote, warnings: ["分析师提纲解析失败，已降级为简版总结。"], degraded: true };
  if (outline.sections.length === 0) throw new Error("请至少保留一个章节。");
  const drafts: DraftedSection[] = [];
  let previousTail = "";
  let cancelled = false;
  for (let index = 0; index < outline.sections.length; index += 1) {
    if (prepared.options.signal?.aborted) {
      cancelled = true;
      break;
    }
    prepared.options.onProgress?.({
      phase: "drafting",
      current: index + 1,
      total: outline.sections.length,
      message: `正在扩写 ${index + 1}/${outline.sections.length}：${outline.sections[index].heading}`,
    });
    try {
      const draft = await draftSection(prepared, outline, index, previousTail);
      drafts.push(draft);
      previousTail = sectionTail(draft.markdown);
    } catch (error) {
      if (!isAbortError(error) && !prepared.options.signal?.aborted) throw error;
      cancelled = true;
      break;
    }
  }
  if (prepared.options.signal?.aborted) cancelled = true;
  if (drafts.length === 0) throw new DOMException("深度笔记生成已取消。", "AbortError");
  prepared.options.onProgress?.({ phase: "assembling", message: "正在组装与检查笔记…" });
  const effectiveOutline = { ...outline, sections: outline.sections.slice(0, drafts.length) };
  const assembled = assembleDeepNote(effectiveOutline, drafts, cancelled);
  prepared.options.onProgress?.({ phase: "persisting", message: cancelled ? "正在保存已完成章节为草稿…" : "正在保存笔记与来源…" });
  const note = await createLibraryNoteWithSources(
    { itemId: null, title: assembled.title, content: assembled.content },
    noteSources(prepared.conversation.id, effectiveOutline),
  );
  return { note, warnings: assembled.warnings, degraded: false };
}
