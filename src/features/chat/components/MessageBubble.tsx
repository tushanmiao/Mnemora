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
  RefreshCcw,
  Trash2,
  UserRound,
  X,
} from "lucide-react";
import type { ChatMessage } from "../../../types/chat";
import { MarkdownMessage } from "./MarkdownMessage";
import { useStreamingMessage } from "../stores/streamingStore";
import "../styles/message-bubble.css";

const LONG_USER_MESSAGE_CHARACTERS = 420;
const LONG_USER_MESSAGE_LINES = 8;
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
  const hasReasoning = displayedReasoning.trim().length > 0;
  const isWaiting = message.status === "pending" && !hasContent && !hasReasoning;
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
  const copyResetTimerRef = useRef<number | null>(null);

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

  const usageParts = message.usage
    ? [
        message.usage.inputTokens !== undefined
          ? `输入 ${message.usage.inputTokens}`
          : null,
        message.usage.outputTokens !== undefined
          ? `输出 ${message.usage.outputTokens}`
          : null,
        message.usage.reasoningTokens !== undefined
          ? `思考 ${message.usage.reasoningTokens}`
          : null,
        message.usage.totalDurationMs !== undefined
          ? `${(message.usage.totalDurationMs / 1000).toFixed(1)} 秒`
          : null,
      ].filter((part): part is string => Boolean(part))
    : [];

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

  const submitEdit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const content = editDraft.trim();
    if (!content || actionsDisabled) return;
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
                  disabled={!editDraft.trim() || actionsDisabled}
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
              {hasContent ? (
                isAssistant ? (
                  <MarkdownMessage content={displayedContent} streaming={isStreaming} />
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
                disabled={actionsDisabled || !message.content.trim()}
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
              <div className="message-usage" title="本次模型请求用量">
                {usageParts.map((part) => <span key={part}>{part}</span>)}
              </div>
            ) : null}
          </footer>
        ) : null}
      </div>
    </article>
  );
});
