import type { ChatMessage } from "../../../types/chat";
import type { Conversation } from "../../../types/conversation";
import { completeChat } from "../api/chat";
import { createLibraryNote } from "../../library/api/library";
import type { LibraryNote } from "../../library/types";
import type { SelectedModel } from "../hooks/useChatRuntime";
import { compressionTranscript } from "./contextCompression";

/**
 * 对话 → 笔记的生成工具。
 *
 * 两条路径：
 * - `summarizeConversationToNote`：AI 总结（`noteSummary` 辅助调用，复用上下文压缩的
 *   转写格式与非流式 `completeChat` 通道，不激活技能、不带附件、不启用 Agent 工具）。
 * - `saveMessageAsNote`：单条助手回答直接转存，不调用模型。
 * 对话级原文转存走 Rust 侧 `save_conversation_as_note`，不在此文件。
 */

/** 与 Rust 侧笔记标题上限（500 字符）对齐。 */
const MAX_NOTE_TITLE_CHARS = 500;
/**
 * 总结转写的字符上限。Rust 侧单条消息上限为 1 MiB 字节；
 * 按全中文（UTF-8 每字符 3 字节）估算，30 万字符 ≈ 900 KB，留足余量。
 */
const MAX_TRANSCRIPT_CHARS = 300_000;
/** 截断提示追加在转写末尾，让模型知道输入不完整。 */
const TRANSCRIPT_TRUNCATION_NOTICE = "\n\n（对话过长，以上转写已截断，请只基于给出的内容总结。）";

const NOTE_SUMMARY_SYSTEM_PROMPT = [
  "你是一位专业的学习笔记作者，负责把一段学习对话整理成一篇可以长期复习的深度知识笔记。",
  "",
  "第一行必须是以「# 」开头的简洁标题，概括笔记主题。",
  "",
  "内容要求：",
  "- 完整覆盖对话中出现的全部知识点，宁可详尽也不要遗漏。对话只是线索：把每个知识点扩写成完整、自洽的讲解，而不是摘抄对话原句。",
  "- 按知识的内在逻辑组织层次（## 一级主题、### 子主题），不要按对话先后顺序流水记录。",
  "- 交代前置知识：如果理解某个概念依赖更基础的概念（例如理解 MVCC 需要先理解事务隔离级别），先用小节把前置知识讲清楚，并明确说明两者如何关联、为什么没有前者就无法理解后者。",
  "- 每个核心概念都配具体示例：代码、SQL、数据表格或分步场景推演，展示「输入 → 过程 → 结果」。",
  "- 复杂的流程、状态变化、组件协作关系，用 mermaid 代码块画图讲解（flowchart、sequenceDiagram、stateDiagram 均可），图后附一段文字解读。",
  "- 对话中用户表现出疑惑、追问或理解偏差的地方，设「⚠️ 常见误区」小节或对比表格，针对性拆解正确与错误理解的差异。",
  "- 涉及多个相似概念时（如多种隔离级别、多种锁），用 Markdown 表格逐维度对比。",
  "- 结尾设「知识关联」小节（本主题与哪些上下游知识相连、下一步该学什么）和「自检问题」小节（3-5 个检验理解程度的问题，不给答案）。",
  "",
  "写作纪律：",
  "- 保持与对话事实一致；扩写背景知识时只补充公认的通用知识，不要虚构对话中不存在的具体数据或结论。",
  "- 不要输出「好的」「以下是」之类的过渡语，直接开始笔记正文。",
  "- 使用中文书写；专业术语保留英文原文。",
].join("\n");

/** 可纳入笔记的消息：已完成且有正文（引用、附件在转写函数里单独说明）。 */
function noteworthyMessages(conversation: Conversation): ChatMessage[] {
  return conversation.messages.filter((message) => (
    message.status === "completed"
    && (
      message.content.trim().length > 0
      || (message.attachments?.length ?? 0) > 0
      || (message.literatureReferences?.length ?? 0) > 0
      || (message.noteReferences?.length ?? 0) > 0
    )
  ));
}

/** 标题不能包含控制字符（Rust 侧校验会拒绝），并统一截断到上限。 */
function sanitizeNoteTitle(raw: string, fallback: string): string {
  const cleaned = raw
    .replace(/[\u0000-\u001f\u007f]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  const title = cleaned || fallback.trim() || "未命名笔记";
  return [...title].slice(0, MAX_NOTE_TITLE_CHARS).join("");
}

/**
 * 从模型输出中拆出标题与正文：首个 `# ` 标题行作为笔记标题（正文保留该行，
 * 与笔记工作区「正文含 H1」的惯例一致）；没有标题行时回退到对话标题并补 H1。
 */
export function splitNoteMarkdown(
  markdown: string,
  fallbackTitle: string,
): { title: string; content: string } {
  const text = markdown.trim();
  const headingMatch = /^#\s+(.+)$/m.exec(text);
  if (headingMatch) {
    return {
      title: sanitizeNoteTitle(headingMatch[1], fallbackTitle),
      content: text,
    };
  }
  const title = sanitizeNoteTitle(fallbackTitle, "未命名笔记");
  return { title, content: `# ${title}\n\n${text}` };
}

/** 生成总结输入转写；复用压缩的「### 用户/### 助手」格式并按上限截断。 */
export function noteSummaryTranscript(conversation: Conversation): string {
  const messages = noteworthyMessages(conversation);
  if (messages.length === 0) return "";
  const transcript = compressionTranscript("", messages);
  if (transcript.length <= MAX_TRANSCRIPT_CHARS) return transcript;
  return transcript.slice(0, MAX_TRANSCRIPT_CHARS) + TRANSCRIPT_TRUNCATION_NOTICE;
}

export type NoteSummaryOptions = {
  /** 用户设置的输出上限；深度笔记篇幅大，内部再取不超过 16K 的上限。 */
  maxOutputTokens: number;
  /** 跟随全局思考开关：深度总结开启思考能显著改善知识组织质量。 */
  thinkingEnabled: boolean;
};

/**
 * 用指定模型把对话提炼为学习笔记并写入笔记库。
 * 由调用方决定模型（惯例：对话自己记录的模型，回退全局默认）并做好防重入。
 */
export async function summarizeConversationToNote(
  conversation: Conversation,
  selectedModel: SelectedModel,
  options: NoteSummaryOptions,
): Promise<LibraryNote> {
  const transcript = noteSummaryTranscript(conversation);
  if (!transcript) {
    throw new Error("对话还没有可以总结的消息。");
  }
  const response = await completeChat({
    providerId: selectedModel.provider.id,
    modelId: selectedModel.model.id,
    conversationId: conversation.id,
    messageId: crypto.randomUUID(),
    operation: "noteSummary",
    systemPrompt: NOTE_SUMMARY_SYSTEM_PROMPT,
    messages: [{ role: "user", content: transcript }],
    options: {
      maxOutputTokens: Math.min(16_384, options.maxOutputTokens),
      thinkingEnabled: options.thinkingEnabled,
    },
  });
  const markdown = response.text.trim();
  if (!markdown) {
    throw new Error("模型没有返回可用的总结内容。");
  }
  const { title, content } = splitNoteMarkdown(markdown, conversation.title);
  return createLibraryNote({ itemId: null, title, content });
}

/** 单条回答的笔记草稿：标题取它之前最近的用户提问首行，正文附来源对话。 */
export function messageNoteDraft(
  conversation: Conversation,
  messageId: string,
): { title: string; content: string } | null {
  const index = conversation.messages.findIndex((message) => message.id === messageId);
  if (index < 0) return null;
  const message = conversation.messages[index];
  const body = message.content.trim();
  if (!body) return null;

  const question = conversation.messages
    .slice(0, index)
    .reverse()
    .find((item) => item.role === "user" && item.content.trim().length > 0);
  const questionLine = question?.content.trim().split("\n", 1)[0] ?? "";
  const title = sanitizeNoteTitle(questionLine, conversation.title);

  const sourceTitle = conversation.title.trim() || "未命名对话";
  const content = `# ${title}\n\n${body}\n\n---\n\n> 来源：对话「${sourceTitle}」`;
  return { title, content };
}

/** 把单条助手回答直接转存为笔记；不调用模型。 */
export async function saveMessageAsNote(
  conversation: Conversation,
  messageId: string,
): Promise<LibraryNote> {
  const draft = messageNoteDraft(conversation, messageId);
  if (!draft) {
    throw new Error("这条消息没有可以保存的内容。");
  }
  return createLibraryNote({ itemId: null, title: draft.title, content: draft.content });
}
