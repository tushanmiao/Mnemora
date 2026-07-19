import { AlertCircle, Bot, LoaderCircle, UserRound } from "lucide-react";
import type { ChatMessage } from "../../../types/chat";
import "../styles/message-bubble.css";

type MessageBubbleProps = {
  message: ChatMessage;
};

export function MessageBubble({ message }: MessageBubbleProps) {
  const isAssistant = message.role === "assistant";
  const isPending = message.status === "pending";
  const isStreaming = message.status === "streaming";
  const isStopped = message.status === "stopped";
  const isError = message.status === "error";
  const usageParts = message.usage
    ? [
        message.usage.inputTokens !== undefined
          ? `输入 ${message.usage.inputTokens}`
          : null,
        message.usage.outputTokens !== undefined
          ? `输出 ${message.usage.outputTokens}`
          : null,
        message.usage.totalDurationMs !== undefined
          ? `${(message.usage.totalDurationMs / 1000).toFixed(1)} 秒`
          : null,
      ].filter(Boolean)
    : [];

  return (
    <article className={`message-row message-row-${message.role}`}>
      <div className={`message-avatar message-avatar-${message.role}`} aria-hidden="true">
        {isAssistant ? <Bot size={18} /> : <UserRound size={17} />}
      </div>
      <div className={`message-bubble message-bubble-${message.role}`}>
        {isPending ? (
          <p className="message-status" role="status">
            <LoaderCircle className="message-spin" size={16} />
            <span>正在生成</span>
          </p>
        ) : (
          <>
            {message.content ? <p>{message.content}</p> : null}
            {isStreaming ? (
              <p className="message-streaming" role="status">
                <LoaderCircle className="message-spin" size={14} />
                <span>正在生成</span>
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
        {usageParts.length > 0 && (
          <div className="message-usage" title="本次模型请求用量">
            {usageParts.map((part) => <span key={part}>{part}</span>)}
          </div>
        )}
      </div>
    </article>
  );
}
