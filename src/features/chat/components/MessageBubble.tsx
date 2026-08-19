import { memo, useEffect, useRef, useState, type FormEvent } from "react";
import {
  AlertCircle,
  Bot,
  BookOpenText,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  FileText,
  LoaderCircle,
  NotebookPen,
  Pencil,
  Quote,
  RefreshCcw,
  Trash2,
  UserRound,
  X,
} from "lucide-react";
import type { ChatMessage, LiteratureReference } from "../../../types/chat";
import { MarkdownMessage } from "./MarkdownMessage";
import { ChatAttachments } from "./ChatAttachments";
import { useStreamingMessage } from "../stores/streamingStore";
import { useI18n } from "../../../i18n/I18nProvider";
import { AgentWorkflow } from "../agent/components/AgentWorkflow";
import { agentWorkflowNeedsAttention, hasAgentActivity } from "../agent/projections/workflowProjection";
import "../styles/message-bubble.css";

const LONG_USER_MESSAGE_CHARACTERS = 420;
const LONG_USER_MESSAGE_LINES = 8;
/** 引用片段的长度上限；超长选区截断，避免把整篇回答塞回上下文。 */
const MAX_QUOTE_CHARACTERS = 2_000;
const MESSAGE_TIME_FORMATTERS = {
  zh: new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }),
  en: new Intl.DateTimeFormat("en-US", { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }),
} as const;

type MessageBubbleProps = {
  message: ChatMessage;
  uiState: MessageBubbleUiState;
  actionsDisabled?: boolean;
  canRegenerate?: boolean;
  onUiStateChange: (messageId: string, patch: Partial<MessageBubbleUiState>) => void;
  onEdit: (messageId: string, content: string) => void;
  onRegenerate: (messageId: string) => void;
  onDelete: (messageId: string) => void;
  /** 选中助手回答的部分文本后点击"引用提问"时回调；不传则不显示引用入口。 */
  onQuote?: (text: string) => void;
  /** 把这条助手回答保存为笔记；返回是否成功。不传则不显示保存入口。 */
  onSaveAsNote?: (messageId: string) => Promise<boolean>;
  onLiteratureReferenceOpen?: (reference: LiteratureReference) => void;
  citationReferences?: readonly LiteratureReference[];
};

export type MessageBubbleUiState = {
  workflowOpen: boolean;
  workflowInteracted: boolean;
  userExpanded: boolean;
  editing: boolean;
  editDraft: string;
};

function formatMessageTime(timestamp: number, language: "zh" | "en") {
  return MESSAGE_TIME_FORMATTERS[language].format(timestamp);
}

async function copySelectedText(text: string) {
  if (navigator.clipboard) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) throw new Error("复制失败");
}

export const MessageBubble = memo(function MessageBubble({
  message,
  uiState,
  actionsDisabled = false,
  canRegenerate = false,
  onUiStateChange,
  onEdit,
  onRegenerate,
  onDelete,
  onQuote,
  onSaveAsNote,
  onLiteratureReferenceOpen,
  citationReferences = [],
}: MessageBubbleProps) {
  const { language, t } = useI18n();
  const isAssistant = message.role === "assistant";
  const canStream = isAssistant && (
    message.status === "pending" || message.status === "streaming"
  );
  const streamingSnapshot = useStreamingMessage(message.id, canStream);
  const displayedContent = streamingSnapshot?.content ?? message.content;
  const displayedReasoning = streamingSnapshot?.reasoning ?? message.reasoning ?? "";
  const isStreaming = message.status === "streaming" || streamingSnapshot !== null;
  const hasContent = displayedContent.trim().length > 0;
  const attachments = message.attachments ?? [];
  const literatureReferences = message.literatureReferences ?? [];
  const noteReferences = message.noteReferences ?? [];
  const hasAttachments = attachments.length > 0;
  const hasLiteratureReferences = (message.literatureReferences?.length ?? 0) > 0;
  const hasNoteReferences = noteReferences.length > 0;
  const hasReasoning = displayedReasoning.trim().length > 0;
  const showAgentActivity = hasAgentActivity(message, displayedReasoning);
  const isStopped = message.status === "stopped";
  const isError = message.status === "error";
  const showFooter = !isStreaming && message.status !== "pending";
  const isLongUserMessage = !isAssistant && (
    Array.from(displayedContent).length > LONG_USER_MESSAGE_CHARACTERS
    || displayedContent.split(/\r?\n/).length > LONG_USER_MESSAGE_LINES
  );
  const { workflowOpen, workflowInteracted, userExpanded, editing } = uiState;
  const editDraft = uiState.editDraft;
  const [copied, setCopied] = useState(false);
  const [noteState, setNoteState] = useState<"idle" | "saving" | "saved">("idle");
  const copyResetTimerRef = useRef<number | null>(null);
  const noteResetTimerRef = useRef<number | null>(null);
  const quoteHostRef = useRef<HTMLDivElement | null>(null);
  const [quoteAnchor, setQuoteAnchor] = useState<{
    left: number;
    top: number;
    text: string;
  } | null>(null);
  const [referencesOpen, setReferencesOpen] = useState(false);

  // 选中助手回答的部分文本后，在选区附近显示轻量操作条。
  const handleQuoteMouseUp = () => {
    if (!onQuote) return;
    const host = quoteHostRef.current;
    const selection = window.getSelection();
    if (!host || !selection || selection.isCollapsed || selection.rangeCount === 0) {
      setQuoteAnchor(null);
      return;
    }
    const { anchorNode, focusNode } = selection;
    if (!anchorNode || !focusNode || !host.contains(anchorNode) || !host.contains(focusNode)) {
      setQuoteAnchor(null);
      return;
    }
    const text = selection.toString().trim();
    if (!text) {
      setQuoteAnchor(null);
      return;
    }
    const range = selection.getRangeAt(0).getBoundingClientRect();
    const hostRect = host.getBoundingClientRect();
    setQuoteAnchor({
      left: Math.max(24, Math.min(range.left - hostRect.left + range.width / 2, hostRect.width - 24)),
      top: range.bottom - hostRect.top + 6,
      text,
    });
  };

  useEffect(() => {
    if (!quoteAnchor) return;
    // 点击气泡外任意处收起浮动按钮；按钮自身通过 stopPropagation 幸免。
    const hide = () => setQuoteAnchor(null);
    document.addEventListener("mousedown", hide);
    return () => document.removeEventListener("mousedown", hide);
  }, [quoteAnchor]);

  useEffect(() => {
    if (!isAssistant || workflowInteracted) return;
    const shouldOpen = agentWorkflowNeedsAttention(message, isStreaming, displayedReasoning);
    if (workflowOpen !== shouldOpen) {
      onUiStateChange(message.id, { workflowOpen: shouldOpen });
    }
  }, [
    isAssistant,
    isStreaming,
    displayedReasoning,
    message.id,
    message.status,
    onUiStateChange,
    workflowInteracted,
    workflowOpen,
  ]);

  useEffect(() => () => {
    if (copyResetTimerRef.current !== null) window.clearTimeout(copyResetTimerRef.current);
    if (noteResetTimerRef.current !== null) window.clearTimeout(noteResetTimerRef.current);
  }, []);

  const usageParts = compactUsageParts(message.usage, language);
  const usageTitle = detailedUsageTitle(message.usage, language);

  const copyContent = async () => {
    if (!hasContent || !navigator.clipboard) return;
    try {
      await navigator.clipboard.writeText(displayedContent);
      setCopied(true);
      if (copyResetTimerRef.current !== null) window.clearTimeout(copyResetTimerRef.current);
      copyResetTimerRef.current = window.setTimeout(() => setCopied(false), 1_500);
    } catch {
      setCopied(false);
    }
  };

  const beginEditing = () => {
    onUiStateChange(message.id, { editing: true, editDraft: message.content });
  };

  // 转存瞬间完成，用图标短暂反馈成功；失败提示由 App 层统一处理。
  const saveAsNote = async () => {
    if (!onSaveAsNote || noteState === "saving") return;
    setNoteState("saving");
    const saved = await onSaveAsNote(message.id);
    if (!saved) {
      setNoteState("idle");
      return;
    }
    setNoteState("saved");
    if (noteResetTimerRef.current !== null) window.clearTimeout(noteResetTimerRef.current);
    noteResetTimerRef.current = window.setTimeout(() => setNoteState("idle"), 1_500);
  };

  const submitEdit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const content = editDraft.trim();
    if ((!content && !hasAttachments && !hasLiteratureReferences && !hasNoteReferences) || actionsDisabled) return;
    onUiStateChange(message.id, {
      editing: false,
      editDraft: content,
      userExpanded: false,
    });
    if (content !== message.content.trim()) onEdit(message.id, content);
  };

  return (
    <article
      id={`message-${message.id}`}
      data-message-id={message.id}
      className={`message-row message-row-${message.role}`}
    >
      <div className={`message-avatar message-avatar-${message.role}`} aria-hidden="true">
        {isAssistant ? <Bot size={18} /> : <UserRound size={17} />}
      </div>
      <div className={`message-column message-column-${message.role}`}>
        <div className={`message-bubble message-bubble-${message.role}${editing ? " message-bubble-editing" : ""}`}>
          {editing ? (
            <form className="message-edit-form" onSubmit={submitEdit}>
              <textarea
                className="message-edit-textarea"
                aria-label={t("chat.edit")}
                autoFocus
                rows={Math.min(10, Math.max(4, editDraft.split(/\r?\n/).length))}
                value={editDraft}
                onChange={(event) => onUiStateChange(message.id, { editDraft: event.target.value })}
              />
              <div className="message-edit-actions">
                <button
                  className="icon-button"
                  type="button"
                  title={t("chat.cancelEdit")}
                  aria-label={t("chat.cancelEdit")}
                  onClick={() => onUiStateChange(message.id, {
                    editing: false,
                    editDraft: message.content,
                  })}
                >
                  <X size={16} />
                </button>
                <button
                  className="icon-button message-edit-save"
                  type="submit"
                  title={t("chat.saveEdit")}
                  aria-label={t("chat.saveEdit")}
                  disabled={(!editDraft.trim() && !hasAttachments && !hasLiteratureReferences && !hasNoteReferences) || actionsDisabled}
                >
                  <Check size={16} />
                </button>
              </div>
            </form>
          ) : (
            <>
              {isAssistant && showAgentActivity ? (
                <AgentWorkflow
                  message={message}
                  reasoning={displayedReasoning}
                  streaming={isStreaming}
                  open={workflowOpen}
                  onOpenChange={(open) => onUiStateChange(message.id, {
                    workflowOpen: open,
                    workflowInteracted: true,
                  })}
                />
              ) : null}
              {hasAttachments ? (
                <ChatAttachments
                  attachments={attachments}
                  conversationId={message.conversationId}
                  variant="message"
                />
              ) : null}
              {hasLiteratureReferences ? (
                <section className={`message-literature-references${referencesOpen ? " message-literature-references-open" : ""}`} aria-label={t("chat.literatureReferences")}>
                  <button className="message-literature-references-toggle" type="button" aria-expanded={referencesOpen} onClick={() => setReferencesOpen((value) => !value)}>
                    <BookOpenText size={13} />
                    <span>{t("chat.literatureReferences")}</span>
                    <small>{literatureReferences.length}</small>
                  </button>
                  {referencesOpen ? literatureReferences.map((reference) => (
                    <button
                      className="message-literature-reference-item"
                      type="button"
                      title={t("chat.openLiteraturePage", { title: reference.title, page: reference.pageIndex + 1 })}
                      disabled={!onLiteratureReferenceOpen}
                      key={reference.id}
                      onClick={() => onLiteratureReferenceOpen?.(reference)}
                    >
                      <BookOpenText size={13} />
                      <span>{reference.title}</span>
                      <small>{t("chat.pageNumber", { page: reference.pageIndex + 1 })}</small>
                    </button>
                  )) : null}
                </section>
              ) : null}
              {hasNoteReferences ? (
                <section className="message-note-references" aria-label="消息引用的笔记">
                  {noteReferences.map((reference) => (
                    <div className="message-note-reference-item" key={reference.id}>
                      <FileText size={13} />
                      <span>
                        <strong>{reference.noteTitle}</strong>
                        <small>{reference.startLine ? `第 ${reference.startLine}${reference.endLine && reference.endLine !== reference.startLine ? `-${reference.endLine}` : ""} 行` : "Markdown 选区"}</small>
                      </span>
                    </div>
                  ))}
                </section>
              ) : null}
              {hasContent ? (
                isAssistant ? (
                  <div
                    className="message-quote-host"
                    ref={quoteHostRef}
                    onMouseUp={handleQuoteMouseUp}
                  >
                    <MarkdownMessage
                      content={displayedContent}
                      streaming={isStreaming}
                      messageId={message.id}
                      literatureReferences={isAssistant ? citationReferences : literatureReferences}
                      onLiteratureReferenceOpen={onLiteratureReferenceOpen}
                    />
                    {quoteAnchor ? (
                      <div
                        className="message-quote-fab"
                        style={{ left: quoteAnchor.left, top: quoteAnchor.top }}
                        onMouseDown={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                        }}
                      >
                        <button
                          type="button"
                          onClick={() => {
                            void copySelectedText(quoteAnchor.text).then(() => {
                              setQuoteAnchor(null);
                              window.getSelection()?.removeAllRanges();
                            });
                          }}
                        >
                          <Copy size={13} />
                          <span>{t("common.copy")}</span>
                        </button>
                        <button
                          type="button"
                          onClick={() => {
                            const text = quoteAnchor.text.length > MAX_QUOTE_CHARACTERS
                              ? `${quoteAnchor.text.slice(0, MAX_QUOTE_CHARACTERS)}…`
                              : quoteAnchor.text;
                            onQuote?.(text);
                            setQuoteAnchor(null);
                            window.getSelection()?.removeAllRanges();
                          }}
                        >
                          <Quote size={13} />
                          <span>{t("chat.quote")}</span>
                        </button>
                      </div>
                    ) : null}
                  </div>
                ) : (
                  <>
                    <div
                      className={`message-user-content${isLongUserMessage && !userExpanded ? " is-collapsed" : ""}`}
                    >
                      <p className="message-plain-text">{displayedContent}</p>
                    </div>
                    {isLongUserMessage ? (
                      <button
                        className="message-user-expand"
                        type="button"
                        aria-expanded={userExpanded}
                        onClick={() => onUiStateChange(message.id, { userExpanded: !userExpanded })}
                      >
                        <span>{userExpanded ? t("chat.collapseContent") : t("chat.expandAll")}</span>
                        {userExpanded ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
                      </button>
                    ) : null}
                  </>
                )
              ) : null}
              {isStreaming ? (
                <p className="message-streaming" role="status">
                  <LoaderCircle className="message-spin" size={14} />
                  <span>{hasContent ? t("chat.responding") : hasReasoning ? t("chat.thinking") : t("chat.waiting")}</span>
                </p>
              ) : null}
              {isStopped ? <p className="message-stopped">{t("chat.stopped")}</p> : null}
              {isError ? (
                <p className="message-error" role="alert">
                  <AlertCircle size={16} />
                  <span>{message.errorMessage ?? t("chat.error")}</span>
                </p>
              ) : null}
            </>
          )}
        </div>

        {showFooter && !editing ? (
          <footer className={`message-footer message-footer-${message.role}`}>
            <time dateTime={new Date(message.updatedAt).toISOString()}>
              {formatMessageTime(message.updatedAt, language)}
            </time>
            <div className="message-actions" aria-label={t("chat.messageActions")}>
              <button
                className="message-action"
                type="button"
                title={copied ? t("chat.copied") : t("common.copy")}
                aria-label={copied ? t("chat.copied") : t("chat.copyMessage")}
                disabled={!hasContent}
                onClick={() => void copyContent()}
              >
                {copied ? <Check size={15} /> : <Copy size={15} />}
              </button>
              {isAssistant && onSaveAsNote ? (
                <button
                  className="message-action"
                  type="button"
                  title={noteState === "saved" ? t("chat.savedAsNote") : t("chat.saveAsNote")}
                  aria-label={noteState === "saved" ? t("chat.savedAsNote") : t("chat.saveAsNote")}
                  disabled={actionsDisabled || !hasContent || noteState === "saving"}
                  onClick={() => void saveAsNote()}
                >
                  {noteState === "saved" ? <Check size={15} /> : <NotebookPen size={15} />}
                </button>
              ) : null}
              <button
                className="message-action"
                type="button"
                title={isAssistant ? t("chat.editAnswer") : t("chat.editAndResend")}
                aria-label={isAssistant ? t("chat.editAnswer") : t("chat.editAndResend")}
                disabled={actionsDisabled || (!message.content.trim() && !hasAttachments && !hasLiteratureReferences && !hasNoteReferences)}
                onClick={beginEditing}
              >
                <Pencil size={15} />
              </button>
              {isAssistant ? (
                <button
                  className="message-action"
                  type="button"
                  title={t("chat.regenerate")}
                  aria-label={t("chat.regenerate")}
                  disabled={actionsDisabled || !canRegenerate}
                  onClick={() => onRegenerate(message.id)}
                >
                  <RefreshCcw size={15} />
                </button>
              ) : null}
              <button
                className="message-action message-action-danger"
                type="button"
                title={t("common.delete")}
                aria-label={t("chat.deleteMessage")}
                disabled={actionsDisabled}
                onClick={() => onDelete(message.id)}
              >
                <Trash2 size={15} />
              </button>
            </div>
            {usageParts.length > 0 ? (
              <div className="message-usage" title={usageTitle}>
                {usageParts.map((part) => <span key={part}>{part}</span>)}
              </div>
            ) : null}
          </footer>
        ) : null}
      </div>
    </article>
  );
});

function formatCompactDuration(value: number) {
  return value < 1_000 ? `${Math.round(value)} ms` : `${(value / 1_000).toFixed(1)} s`;
}

function compactUsageParts(usage: ChatMessage["usage"] | undefined, language: "zh" | "en") {
  if (!usage) return [];
  const cacheRate = usage.inputTokens
    ? (usage.cacheReadTokens ?? 0) / usage.inputTokens
    : null;
  return [
    usage.totalTokens !== undefined ? formatCompactNumber(usage.totalTokens, language) : null,
    usage.inputTokens !== undefined ? `↑ ${formatCompactNumber(usage.inputTokens, language)}` : null,
    usage.outputTokens !== undefined ? `↓ ${formatCompactNumber(usage.outputTokens, language)}` : null,
    cacheRate !== null && usage.cacheReadTokens ? `${language === "en" ? "Cache" : "缓存"} ${(cacheRate * 100).toFixed(0)}%` : null,
    usage.costUsd !== undefined ? `$${usage.costUsd.toFixed(usage.costUsd < 0.01 ? 4 : 3)}` : null,
    usage.timeToFirstTokenMs !== undefined ? `${language === "en" ? "TTFT" : "首字"} ${formatCompactDuration(usage.timeToFirstTokenMs)}` : null,
    usage.outputTokensPerSecond !== undefined ? `${usage.outputTokensPerSecond.toFixed(1)} tok/s` : null,
    (usage.callCount ?? 1) > 1 ? `${usage.callCount} ${language === "en" ? "calls" : "轮"}` : null,
  ].filter((part): part is string => Boolean(part));
}

function detailedUsageTitle(usage: ChatMessage["usage"] | undefined, language: "zh" | "en") {
  if (!usage) return "";
  if (language === "en") {
    return [
      `Non-cached input: ${usage.nonCachedInputTokens ?? "-"}`,
      `Cache read: ${usage.cacheReadTokens ?? "-"}`,
      `Cache write: ${usage.cacheWriteTokens ?? "-"}`,
      `Output: ${usage.outputTokens ?? "-"}`,
      `Reasoning: ${usage.reasoningTokens ?? "-"}`,
      `Model calls: ${usage.callCount ?? 1}`,
      `Total duration: ${usage.totalDurationMs !== undefined ? formatCompactDuration(usage.totalDurationMs) : "-"}`,
      `Usage source: ${usage.usageSource ?? "missing"}`,
      `Cost source: ${usage.costSource ?? "missing"}`,
    ].join("\n");
  }
  return [
    `普通输入：${usage.nonCachedInputTokens ?? "-"}`,
    `缓存读取：${usage.cacheReadTokens ?? "-"}`,
    `缓存创建：${usage.cacheWriteTokens ?? "-"}`,
    `输出：${usage.outputTokens ?? "-"}`,
    `思考：${usage.reasoningTokens ?? "-"}`,
    `模型调用：${usage.callCount ?? 1} 次`,
    `总耗时：${usage.totalDurationMs !== undefined ? formatCompactDuration(usage.totalDurationMs) : "-"}`,
    `Usage 来源：${usage.usageSource ?? "missing"}`,
    `成本来源：${usage.costSource ?? "missing"}`,
  ].join("\n");
}

function formatCompactNumber(value: number, language: "zh" | "en") {
  return new Intl.NumberFormat(language === "en" ? "en-US" : "zh-CN", {
    notation: value >= 10_000 ? "compact" : "standard",
    maximumFractionDigits: 1,
  }).format(value);
}
