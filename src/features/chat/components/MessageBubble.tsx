import { memo, useEffect, useRef, useState, type FormEvent } from "react";
import {
  AlertCircle,
  Bot,
  BrainCircuit,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  LoaderCircle,
  Pencil,
  Quote,
  RefreshCcw,
  Sparkles,
  Wrench,
  Trash2,
  UserRound,
  X,
} from "lucide-react";
import type { ChatMessage } from "../../../types/chat";
import { MarkdownMessage } from "./MarkdownMessage";
import { ChatAttachments } from "./ChatAttachments";
import { useStreamingMessage } from "../stores/streamingStore";
import { resolveToolApproval } from "../api/chat";
import "../styles/message-bubble.css";

const LONG_USER_MESSAGE_CHARACTERS = 420;
const LONG_USER_MESSAGE_LINES = 8;
/** 引用片段的长度上限；超长选区截断，避免把整篇回答塞回上下文。 */
const MAX_QUOTE_CHARACTERS = 2_000;
const MESSAGE_TIME_FORMATTER = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

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
};

export type MessageBubbleUiState = {
  reasoningOpen: boolean;
  userExpanded: boolean;
  editing: boolean;
  editDraft: string;
};

function formatMessageTime(timestamp: number) {
  return MESSAGE_TIME_FORMATTER.format(timestamp);
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
}: MessageBubbleProps) {
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
  const activatedSkills = message.activatedSkills ?? [];
  const toolTraces = message.toolTraces ?? [];
  const hasAttachments = attachments.length > 0;
  const hasReasoning = displayedReasoning.trim().length > 0;
  const isWaiting = isAssistant
    && message.status === "pending"
    && !hasContent
    && !hasReasoning
    && toolTraces.length === 0;
  const isStopped = message.status === "stopped";
  const isError = message.status === "error";
  const showFooter = !isStreaming && message.status !== "pending";
  const isLongUserMessage = !isAssistant && (
    Array.from(displayedContent).length > LONG_USER_MESSAGE_CHARACTERS
    || displayedContent.split(/\r?\n/).length > LONG_USER_MESSAGE_LINES
  );
  const { reasoningOpen, userExpanded, editing } = uiState;
  const editDraft = uiState.editDraft;
  const [copied, setCopied] = useState(false);
  const [resolvingApprovalId, setResolvingApprovalId] = useState<string | null>(null);
  const copyResetTimerRef = useRef<number | null>(null);
  const quoteHostRef = useRef<HTMLDivElement | null>(null);
  const [quoteAnchor, setQuoteAnchor] = useState<{
    left: number;
    top: number;
    text: string;
  } | null>(null);

  // 选中助手回答的部分文本 → 在选区下方浮出"引用提问"按钮。
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
      text: text.length > MAX_QUOTE_CHARACTERS ? `${text.slice(0, MAX_QUOTE_CHARACTERS)}…` : text,
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
    if (!isStreaming) return;
    if (isStreaming && hasReasoning && !hasContent) {
      onUiStateChange(message.id, { reasoningOpen: true });
    } else if (hasContent) {
      onUiStateChange(message.id, { reasoningOpen: false });
    }
  }, [hasContent, hasReasoning, isStreaming, message.id, onUiStateChange]);

  useEffect(() => () => {
    if (copyResetTimerRef.current !== null) window.clearTimeout(copyResetTimerRef.current);
  }, []);

  const usageParts = compactUsageParts(message.usage);
  const usageTitle = detailedUsageTitle(message.usage);

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

  const resolveApproval = async (approvalId: string, approved: boolean) => {
    if (resolvingApprovalId) return;
    setResolvingApprovalId(approvalId);
    try {
      await resolveToolApproval(approvalId, approved);
    } finally {
      setResolvingApprovalId(null);
    }
  };

  const submitEdit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const content = editDraft.trim();
    if ((!content && !hasAttachments) || actionsDisabled) return;
    onUiStateChange(message.id, {
      editing: false,
      editDraft: content,
      userExpanded: false,
    });
    if (content !== message.content.trim()) onEdit(message.id, content);
  };

  return (
    <article className={`message-row message-row-${message.role}`}>
      <div className={`message-avatar message-avatar-${message.role}`} aria-hidden="true">
        {isAssistant ? <Bot size={18} /> : <UserRound size={17} />}
      </div>
      <div className={`message-column message-column-${message.role}`}>
        <div className={`message-bubble message-bubble-${message.role}${editing ? " message-bubble-editing" : ""}`}>
          {editing ? (
            <form className="message-edit-form" onSubmit={submitEdit}>
              <textarea
                className="message-edit-textarea"
                aria-label="编辑消息"
                autoFocus
                rows={Math.min(10, Math.max(4, editDraft.split(/\r?\n/).length))}
                value={editDraft}
                onChange={(event) => onUiStateChange(message.id, { editDraft: event.target.value })}
              />
              <div className="message-edit-actions">
                <button
                  className="icon-button"
                  type="button"
                  title="取消修改"
                  aria-label="取消修改"
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
                  title="保存修改"
                  aria-label="保存修改"
                  disabled={(!editDraft.trim() && !hasAttachments) || actionsDisabled}
                >
                  <Check size={16} />
                </button>
              </div>
            </form>
          ) : isWaiting ? (
            <p className="message-status" role="status">
              <LoaderCircle className="message-spin" size={16} />
              <span>等待模型响应</span>
            </p>
          ) : (
            <>
              {isAssistant && activatedSkills.length > 0 ? (
                <div className="message-skills" aria-label="本轮启用的技能">
                  {activatedSkills.map((skill) => (
                    <span
                      key={skill.id}
                      title={`${skill.name} · v${skill.version} · ${skill.activation === "slash" ? "Slash 激活" : skill.activation === "model" ? "模型激活" : "手动激活"}`}
                    >
                      <Sparkles size={12} />
                      {skill.name}
                    </span>
                  ))}
                </div>
              ) : null}
              {isAssistant && toolTraces.length > 0 ? (
                <div className="message-tools" aria-label="工具调用轨迹">
                  {toolTraces.map((trace) => (
                    <details className={`message-tool message-tool-${trace.status}`} key={trace.callId} open={trace.status === "awaitingApproval"}>
                      <summary>
                        <Wrench size={13} />
                        <strong>{toolNameLabel(trace.name)}</strong>
                        <span>{toolStatusLabel(trace.status)}</span>
                        {trace.durationMs !== undefined ? <small>{formatCompactDuration(trace.durationMs)}</small> : null}
                      </summary>
                      <div className="message-tool-detail">
                        <code>{trace.argumentSummary}</code>
                        {trace.preview ? <p>{trace.preview}</p> : null}
                        {trace.status === "awaitingApproval" && trace.approvalId ? (
                          <div className="message-tool-approval">
                            <button
                              type="button"
                              disabled={resolvingApprovalId !== null}
                              onClick={() => void resolveApproval(trace.approvalId!, false)}
                            >
                              拒绝
                            </button>
                            <button
                              className="message-tool-approve"
                              type="button"
                              disabled={resolvingApprovalId !== null}
                              onClick={() => void resolveApproval(trace.approvalId!, true)}
                            >
                              允许本次
                            </button>
                          </div>
                        ) : null}
                      </div>
                    </details>
                  ))}
                </div>
              ) : null}
              {hasReasoning ? (
                <section className={`message-reasoning${reasoningOpen ? " is-open" : ""}`}>
                  <button
                    className="message-reasoning-toggle"
                    type="button"
                    aria-expanded={reasoningOpen}
                    onClick={() => onUiStateChange(message.id, { reasoningOpen: !reasoningOpen })}
                  >
                    <BrainCircuit size={15} />
                    <span>{isStreaming && !hasContent ? "思考中" : "思考过程"}</span>
                    <ChevronDown className="message-reasoning-chevron" size={15} />
                  </button>
                  {reasoningOpen ? (
                    <div className="message-reasoning-content">{displayedReasoning}</div>
                  ) : null}
                </section>
              ) : null}
              {hasAttachments ? (
                <ChatAttachments
                  attachments={attachments}
                  conversationId={message.conversationId}
                  variant="message"
                />
              ) : null}
              {hasContent ? (
                isAssistant ? (
                  <div
                    className="message-quote-host"
                    ref={quoteHostRef}
                    onMouseUp={handleQuoteMouseUp}
                  >
                    <MarkdownMessage content={displayedContent} streaming={isStreaming} />
                    {quoteAnchor ? (
                      <button
                        className="message-quote-fab"
                        type="button"
                        style={{ left: quoteAnchor.left, top: quoteAnchor.top }}
                        onMouseDown={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                        }}
                        onClick={() => {
                          onQuote?.(quoteAnchor.text);
                          setQuoteAnchor(null);
                          window.getSelection()?.removeAllRanges();
                        }}
                      >
                        <Quote size={13} />
                        <span>引用提问</span>
                      </button>
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
                        <span>{userExpanded ? "收起内容" : "展开全部"}</span>
                        {userExpanded ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
                      </button>
                    ) : null}
                  </>
                )
              ) : null}
              {isStreaming ? (
                <p className="message-streaming" role="status">
                  <LoaderCircle className="message-spin" size={14} />
                  <span>{hasContent ? "正在回答" : hasReasoning ? "正在思考" : "等待模型响应"}</span>
                </p>
              ) : null}
              {isStopped ? <p className="message-stopped">已停止生成</p> : null}
              {isError ? (
                <p className="message-error" role="alert">
                  <AlertCircle size={16} />
                  <span>{message.errorMessage ?? "模型请求失败，请稍后重试。"}</span>
                </p>
              ) : null}
            </>
          )}
        </div>

        {showFooter && !editing ? (
          <footer className={`message-footer message-footer-${message.role}`}>
            <time dateTime={new Date(message.updatedAt).toISOString()}>
              {formatMessageTime(message.updatedAt)}
            </time>
            <div className="message-actions" aria-label="消息操作">
              <button
                className="message-action"
                type="button"
                title={copied ? "已复制" : "复制"}
                aria-label={copied ? "已复制" : "复制消息"}
                disabled={!hasContent}
                onClick={() => void copyContent()}
              >
                {copied ? <Check size={15} /> : <Copy size={15} />}
              </button>
              <button
                className="message-action"
                type="button"
                title={isAssistant ? "修改回答" : "修改并重新发送"}
                aria-label={isAssistant ? "修改回答" : "修改并重新发送"}
                disabled={actionsDisabled || (!message.content.trim() && !hasAttachments)}
                onClick={beginEditing}
              >
                <Pencil size={15} />
              </button>
              {isAssistant ? (
                <button
                  className="message-action"
                  type="button"
                  title="重新生成"
                  aria-label="重新生成回答"
                  disabled={actionsDisabled || !canRegenerate}
                  onClick={() => onRegenerate(message.id)}
                >
                  <RefreshCcw size={15} />
                </button>
              ) : null}
              <button
                className="message-action message-action-danger"
                type="button"
                title="删除"
                aria-label="删除消息"
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

function toolNameLabel(name: string) {
  if (name === "skill") return "加载技能";
  if (name === "read_attachment_text") return "读取文本附件";
  if (name === "read_pdf_pages") return "读取 PDF 页面";
  return name;
}

function toolStatusLabel(status: string) {
  if (status === "awaitingApproval") return "等待确认";
  if (status === "running") return "执行中";
  if (status === "completed") return "已完成";
  if (status === "rejected") return "已拒绝";
  return "失败";
}

function formatCompactDuration(value: number) {
  return value < 1_000 ? `${Math.round(value)} ms` : `${(value / 1_000).toFixed(1)} s`;
}

function compactUsageParts(usage?: ChatMessage["usage"]) {
  if (!usage) return [];
  const cacheRate = usage.inputTokens
    ? (usage.cacheReadTokens ?? 0) / usage.inputTokens
    : null;
  return [
    usage.totalTokens !== undefined ? formatCompactNumber(usage.totalTokens) : null,
    usage.inputTokens !== undefined ? `↑ ${formatCompactNumber(usage.inputTokens)}` : null,
    usage.outputTokens !== undefined ? `↓ ${formatCompactNumber(usage.outputTokens)}` : null,
    cacheRate !== null && usage.cacheReadTokens ? `缓存 ${(cacheRate * 100).toFixed(0)}%` : null,
    usage.costUsd !== undefined ? `$${usage.costUsd.toFixed(usage.costUsd < 0.01 ? 4 : 3)}` : null,
    usage.timeToFirstTokenMs !== undefined ? `首字 ${formatCompactDuration(usage.timeToFirstTokenMs)}` : null,
    usage.outputTokensPerSecond !== undefined ? `${usage.outputTokensPerSecond.toFixed(1)} tok/s` : null,
    (usage.callCount ?? 1) > 1 ? `${usage.callCount} 轮` : null,
  ].filter((part): part is string => Boolean(part));
}

function detailedUsageTitle(usage?: ChatMessage["usage"]) {
  if (!usage) return "";
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

function formatCompactNumber(value: number) {
  return new Intl.NumberFormat("zh-CN", {
    notation: value >= 10_000 ? "compact" : "standard",
    maximumFractionDigits: 1,
  }).format(value);
}
