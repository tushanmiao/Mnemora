import { useEffect, useRef } from "react";
import {
  BookOpenText,
  Lightbulb,
  ListTodo,
  MessageCircleQuestion,
  MessageSquarePlus,
} from "lucide-react";
import type { ChatMessage } from "../../../types/chat";
import { MessageBubble } from "./MessageBubble";
import "../styles/message-list.css";

const suggestions = [
  {
    icon: Lightbulb,
    title: "梳理一个想法",
    description: "把零散思路整理成清晰结构",
  },
  {
    icon: BookOpenText,
    title: "阅读一篇文献",
    description: "后续可以从文献库选择 PDF",
  },
  {
    icon: ListTodo,
    title: "制定学习计划",
    description: "将目标拆分为可以执行的步骤",
  },
];

type MessageListProps = {
  messages: ChatMessage[];
  hasConversation: boolean;
  suggestionsDisabled?: boolean;
  onCreateConversation: () => void;
  onSuggestionSelect: (prompt: string) => void;
};

export function MessageList({
  messages,
  hasConversation,
  suggestionsDisabled = false,
  onCreateConversation,
  onSuggestionSelect,
}: MessageListProps) {
  const listRef = useRef<HTMLElement>(null);

  useEffect(() => {
    const list = listRef.current;
    if (!list || messages.length === 0) return;

    const lastMessage = messages[messages.length - 1];
    const frameId = requestAnimationFrame(() => {
      list.scrollTo({
        top: list.scrollHeight,
        behavior: lastMessage.status === "streaming" ? "auto" : "smooth",
      });
    });
    return () => cancelAnimationFrame(frameId);
  }, [messages]);

  return (
    <section className="message-list" aria-label="消息列表" ref={listRef}>
      {!hasConversation ? (
        <div className="empty-chat-state">
          <div className="empty-chat-mark" aria-hidden="true">
            <MessageCircleQuestion size={28} />
          </div>
          <div className="empty-chat-copy">
            <span className="conversation-label">对话</span>
            <h2>还没有对话</h2>
          </div>
          <button className="empty-chat-action" type="button" onClick={onCreateConversation}>
            <MessageSquarePlus size={17} />
            <span>新建聊天</span>
          </button>
        </div>
      ) : messages.length === 0 ? (
        <div className="empty-chat-state">
          <div className="empty-chat-mark" aria-hidden="true">
            <MessageCircleQuestion size={28} />
          </div>
          <div className="empty-chat-copy">
            <span className="conversation-label">新对话</span>
            <h2>今天想研究什么？</h2>
            <p>提出一个问题，或者从下面的建议开始。</p>
          </div>

          <div className="suggestion-grid" aria-label="提问建议">
            {suggestions.map(({ icon: Icon, title, description }) => (
              <button
                className="suggestion-item"
                type="button"
                key={title}
                disabled={suggestionsDisabled}
                onClick={() => onSuggestionSelect(title)}
              >
                <Icon size={18} />
                <span>
                  <strong>{title}</strong>
                  <small>{description}</small>
                </span>
              </button>
            ))}
          </div>
        </div>
      ) : (
        <div className="message-thread">
          {messages.map((message) => (
            <MessageBubble message={message} key={message.id} />
          ))}
        </div>
      )}
    </section>
  );
}
