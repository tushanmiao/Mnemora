import { Bot, UserRound } from "lucide-react";
import type { ChatMessage } from "../types/chat";
import "../styles/message-bubble.css";

type MessageBubbleProps = {
  message: ChatMessage;
};

export function MessageBubble({ message }: MessageBubbleProps) {
  const isAssistant = message.role === "assistant";

  return (
    <article className={`message-row message-row-${message.role}`}>
      <div className={`message-avatar message-avatar-${message.role}`} aria-hidden="true">
        {isAssistant ? <Bot size={18} /> : <UserRound size={17} />}
      </div>
      <div className={`message-bubble message-bubble-${message.role}`}>
        <p>{message.content}</p>
      </div>
    </article>
  );
}
