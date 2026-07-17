import { Bot, UserRound } from "lucide-react";
import type { ReactNode } from "react";
import type { MessageRole } from "../types/chat";
import "../styles/message-bubble.css";

type MessageBubbleProps = {
  role: MessageRole;
  children: ReactNode;
};

export function MessageBubble({ role, children }: MessageBubbleProps) {
  const isAssistant = role === "assistant";

  return (
    <article className={`message-row message-row-${role}`}>
      <div className={`message-avatar message-avatar-${role}`} aria-hidden="true">
        {isAssistant ? <Bot size={18} /> : <UserRound size={17} />}
      </div>
      <div className={`message-bubble message-bubble-${role}`}>{children}</div>
    </article>
  );
}
